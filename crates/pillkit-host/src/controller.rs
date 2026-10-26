//! The host's brain. Owns every registered app, its licence, feed and local
//! items, decides what goes in the slot, and tells the platform layer to draw it.
//! Runs on its own thread; the OS event loop keeps the main thread.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use pillkit_core::envelope::{open_feed, open_license, parse_public_key};
use pillkit_core::model::{Action, Feed, Item, License};
use pillkit_core::policy;
use pillkit_core::rotation::Rotator;
use pillkit_core::VerifyingKey;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ipc::{send, Outbox};
use crate::net;
use crate::paths::{data_dir, log, now};
use crate::platform::{CardView, Display, MenuEntry, UiCmd, UiEvent};

pub enum Msg {
    Connected {
        conn_id: u64,
        out: Outbox,
    },
    Disconnected {
        conn_id: u64,
    },
    Request {
        conn_id: u64,
        body: Value,
    },
    License {
        app_id: String,
        result: Result<String, String>,
    },
    Feed {
        app_id: String,
        result: Result<String, String>,
    },
    Ui(UiEvent),
}

const LICENSE_REFRESH_SECS: i64 = 24 * 3600;
const EVENT_FLUSH_SECS: i64 = 300;
const HIDE_SECS: i64 = 12 * 3600;
const MAX_LOCAL_ITEMS: usize = 20;
const WHY_URL: &str = "https://pillkit.dev/why";

/// What we keep on disk per app so the widget survives the app (and the host)
/// restarting, and keeps working offline until the licence's refresh deadline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Cached {
    app_id: String,
    key: String,
    bundle_id: String,
    server: String,
    license_token: Option<String>,
    feed_token: Option<String>,
}

struct App {
    cached: Cached,
    license: Option<License>,
    feed: Option<Feed>,
    local: Vec<Item>,
    conns: HashSet<u64>,
    license_fetched_at: i64,
    /// While unlicensed: when to ask again, backing off 15 s, 30 s, ... 10 min.
    license_retry_at: i64,
    license_failures: u32,
    next_feed_at: i64,
    fetching_license: bool,
    fetching_feed: bool,
    last_error: Option<String>,
    events: HashMap<(String, &'static str), u32>,
}

impl App {
    fn new(cached: Cached) -> App {
        App {
            cached,
            license: None,
            feed: None,
            local: Vec::new(),
            conns: HashSet::new(),
            license_fetched_at: 0,
            license_retry_at: 0,
            license_failures: 0,
            next_feed_at: 0,
            fetching_license: false,
            fetching_feed: false,
            last_error: None,
            events: HashMap::new(),
        }
    }

    fn licensed(&self, t: i64) -> bool {
        self.license.as_ref().is_some_and(|l| t < l.exp)
    }

    fn allowed_prefixes(&self) -> &[String] {
        self.feed
            .as_ref()
            .map(|f| f.allowed_url_prefixes.as_slice())
            .unwrap_or(&[])
    }

    /// Everything this app may show right now.
    fn items(&self, t: i64) -> Vec<Item> {
        let Some(lic) = self.license.as_ref().filter(|l| t < l.exp) else {
            return Vec::new();
        };
        let mut items: Vec<Item> = Vec::new();
        if let Some(feed) = self.feed.as_ref().filter(|f| t < f.exp) {
            items.extend(feed.items.iter().cloned());
        }
        items.extend(self.local.iter().cloned());
        items.truncate(lic.entitlements.max_items.max(1) as usize);
        items
            .into_iter()
            .map(|mut i| {
                if lic.entitlements.watermark {
                    i.label = "Pill Kit dev".into();
                }
                policy::sanitize(i)
            })
            .collect()
    }
}

pub struct Controller {
    key: Option<VerifyingKey>,
    tx: Sender<Msg>,
    ui: Sender<UiCmd>,
    apps: HashMap<String, App>,
    conn_out: HashMap<u64, Outbox>,
    conn_app: HashMap<u64, String>,
    rotator: Rotator,
    current: Option<(String, Item)>,
    current_until: i64,
    hidden_until: i64,
    last_flush: i64,
}

pub fn public_key() -> Option<VerifyingKey> {
    // Debug builds may point at a local dev server with its own key.
    if cfg!(debug_assertions) {
        if let Ok(k) = std::env::var("PILLKIT_PUBLIC_KEY") {
            return parse_public_key(&k);
        }
        if let Ok(k) = std::fs::read_to_string(data_dir().join("dev-public-key")) {
            return parse_public_key(&k);
        }
    }
    option_env!("PILLKIT_PUBLIC_KEY").and_then(parse_public_key)
}

fn cache_dir() -> PathBuf {
    let d = data_dir().join("apps");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Controller {
    pub fn new(tx: Sender<Msg>, ui: Sender<UiCmd>) -> Controller {
        let key = public_key();
        if key.is_none() {
            log("no server public key configured: every licence will be rejected");
        }
        let mut c = Controller {
            key,
            tx,
            ui,
            apps: HashMap::new(),
            conn_out: HashMap::new(),
            conn_app: HashMap::new(),
            rotator: Rotator::default(),
            current: None,
            current_until: 0,
            hidden_until: 0,
            last_flush: now(),
        };
        c.load_cache();
        c
    }

    fn load_cache(&mut self) {
        let Ok(entries) = std::fs::read_dir(cache_dir()) else {
            return;
        };
        for e in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(e.path()) else {
                continue;
            };
            let Ok(cached) = serde_json::from_str::<Cached>(&text) else {
                continue;
            };
            if !safe_id(&cached.app_id) {
                continue;
            }
            let mut app = App::new(cached.clone());
            let t = now();
            if let (Some(k), Some(tok)) = (&self.key, &cached.license_token) {
                app.license = open_license(tok, k, &cached.app_id, t).ok();
            }
            if let (Some(k), Some(tok)) = (&self.key, &cached.feed_token) {
                app.feed = open_feed(tok, k, &cached.app_id, t).ok();
            }
            log(format!(
                "restored {} (licensed: {})",
                cached.app_id,
                app.licensed(t)
            ));
            self.apps.insert(cached.app_id.clone(), app);
        }
    }

    fn save(&self, app_id: &str) {
        if let Some(app) = self.apps.get(app_id) {
            let path = cache_dir().join(format!("{app_id}.json"));
            if let Ok(text) = serde_json::to_string_pretty(&app.cached) {
                let _ = std::fs::write(path, text);
            }
        }
    }

    pub fn run(mut self, rx: Receiver<Msg>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(msg) => {
                    if !self.handle(msg) {
                        let _ = self.ui.send(UiCmd::Quit);
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.tick();
        }
    }

    /// Returns false when the host should exit.
    fn handle(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Connected { conn_id, out } => {
                self.conn_out.insert(conn_id, out);
            }
            Msg::Disconnected { conn_id } => {
                self.conn_out.remove(&conn_id);
                if let Some(app_id) = self.conn_app.remove(&conn_id) {
                    if let Some(app) = self.apps.get_mut(&app_id) {
                        app.conns.remove(&conn_id);
                    }
                }
            }
            Msg::Request { conn_id, body } => return self.request(conn_id, body),
            Msg::License { app_id, result } => self.on_license(&app_id, result),
            Msg::Feed { app_id, result } => self.on_feed(&app_id, result),
            Msg::Ui(ev) => self.on_ui(ev),
        }
        true
    }

    fn reply(&self, conn_id: u64, v: Value) {
        if let Some(out) = self.conn_out.get(&conn_id) {
            send(out, &v);
        }
    }

    fn request(&mut self, conn_id: u64, body: Value) -> bool {
        let op = body.get("op").and_then(Value::as_str).unwrap_or("");
        let s = |k: &str| {
            body.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        match op {
            "hello" => self.reply(conn_id, json!({"ok": true, "version": env!("CARGO_PKG_VERSION"), "protocol": pillkit_core::PROTOCOL_VERSION})),
            "register" => {
                let (app_id, key, bundle_id) = (s("app_id"), s("key"), s("bundle_id"));
                let server = Some(s("server")).filter(|x| !x.is_empty()).unwrap_or_else(|| net::DEFAULT_SERVER.into());
                if !safe_id(&app_id) || key.is_empty() {
                    self.reply(conn_id, json!({"ok": false, "error": "app_id_and_key_required"}));
                    return true;
                }
                if !net::server_allowed(&server) {
                    self.reply(conn_id, json!({"ok": false, "error": "server_must_be_https"}));
                    return true;
                }
                let app = self.apps.entry(app_id.clone()).or_insert_with(|| App::new(Cached::default()));
                let changed = app.cached.key != key || app.cached.server != server || app.cached.bundle_id != bundle_id;
                app.cached.app_id = app_id.clone();
                app.cached.key = key;
                app.cached.server = server;
                app.cached.bundle_id = bundle_id;
                if changed {
                    app.license_fetched_at = 0;
                    app.next_feed_at = 0;
                }
                if !app.licensed(now()) {
                    // An app just connected: ask for its licence right away.
                    app.license_retry_at = 0;
                }
                app.conns.insert(conn_id);
                let licensed = app.licensed(now());
                self.conn_app.insert(conn_id, app_id.clone());
                log(format!("{app_id}: registered by an app (server {})", self.apps[&app_id].cached.server));
                self.save(&app_id);
                self.reply(conn_id, json!({"ok": true, "licensed": licensed}));
            }
            "show" | "clear" | "unregister" | "status" => {
                let Some(app_id) = self.conn_app.get(&conn_id).cloned() else {
                    self.reply(conn_id, json!({"ok": false, "error": "register_first"}));
                    return true;
                };
                match op {
                    "show" => {
                        let item = body.get("item").cloned().map(serde_json::from_value::<Item>);
                        match item {
                            Some(Ok(item)) if !item.id.is_empty() && !item.title.is_empty() => {
                                let app = self.apps.get_mut(&app_id).expect("registered");
                                app.local.retain(|i| i.id != item.id);
                                app.local.insert(0, item);
                                app.local.truncate(MAX_LOCAL_ITEMS);
                                let licensed = app.licensed(now());
                                self.reply(conn_id, json!({"ok": true, "licensed": licensed}));
                            }
                            _ => self.reply(conn_id, json!({"ok": false, "error": "item_needs_id_and_title"})),
                        }
                    }
                    "clear" => {
                        if let Some(app) = self.apps.get_mut(&app_id) {
                            app.local.clear();
                        }
                        self.reply(conn_id, json!({"ok": true}));
                    }
                    "status" => {
                        let t = now();
                        let app = &self.apps[&app_id];
                        self.reply(
                            conn_id,
                            json!({"ok": true, "licensed": app.licensed(t), "plan": app.license.as_ref().map(|l| l.plan.clone()),
                                   "feed_items": app.feed.as_ref().map(|f| f.items.len()).unwrap_or(0),
                                   "local_items": app.local.len(), "error": app.last_error}),
                        );
                    }
                    _ => {
                        self.apps.remove(&app_id);
                        self.rotator.remove_app(&app_id);
                        let _ = std::fs::remove_file(cache_dir().join(format!("{app_id}.json")));
                        self.conn_app.retain(|_, a| a != &app_id);
                        self.reply(conn_id, json!({"ok": true}));
                    }
                }
            }
            "shutdown" => {
                self.reply(conn_id, json!({"ok": true}));
                log("shutdown requested by an SDK");
                return false;
            }
            _ => self.reply(conn_id, json!({"ok": false, "error": "unknown_op"})),
        }
        true
    }

    fn on_license(&mut self, app_id: &str, result: Result<String, String>) {
        let key = self.key;
        let Some(app) = self.apps.get_mut(app_id) else {
            return;
        };
        app.fetching_license = false;
        app.license_fetched_at = now();
        match result.and_then(|tok| {
            let k = key.ok_or("no public key")?;
            open_license(&tok, &k, app_id, now())
                .map(|l| (tok, l))
                .map_err(|e| e.to_string())
        }) {
            Ok((tok, lic)) => {
                log(format!(
                    "{app_id}: licence ok ({}), refresh by {}",
                    lic.plan, lic.exp
                ));
                app.license = Some(lic);
                app.cached.license_token = Some(tok);
                app.license_failures = 0;
                app.last_error = None;
                app.next_feed_at = 0;
            }
            Err(e) => {
                app.license_failures += 1;
                let wait = (15i64 << (app.license_failures - 1).min(6)).min(600);
                app.license_retry_at = now() + wait;
                log(format!(
                    "{app_id}: licence refresh failed: {e} (trying again in {wait} s)"
                ));
                // A definite "no" from the server switches the widget off now;
                // network trouble keeps the cached licence until its deadline.
                if e.starts_with("401") || e.starts_with("403") || e.starts_with("404") {
                    app.license = None;
                    app.cached.license_token = None;
                }
                app.last_error = Some(e);
            }
        }
        self.save(app_id);
    }

    fn on_feed(&mut self, app_id: &str, result: Result<String, String>) {
        let key = self.key;
        let Some(app) = self.apps.get_mut(app_id) else {
            return;
        };
        app.fetching_feed = false;
        match result.and_then(|tok| {
            let k = key.ok_or("no public key")?;
            open_feed(&tok, &k, app_id, now())
                .map(|f| (tok, f))
                .map_err(|e| e.to_string())
        }) {
            Ok((tok, feed)) => {
                app.next_feed_at = now() + feed.poll_secs.clamp(10, 3600) as i64;
                log(format!("{app_id}: feed ok, {} items", feed.items.len()));
                app.feed = Some(feed);
                app.cached.feed_token = Some(tok);
            }
            Err(e) => {
                log(format!("{app_id}: feed failed: {e}"));
                if e.starts_with("401") || e.starts_with("403") {
                    app.feed = None;
                    app.cached.feed_token = None;
                    app.license_fetched_at = 0;
                }
                app.next_feed_at = now() + 120;
                app.last_error = Some(e);
            }
        }
        self.save(app_id);
    }

    fn tick(&mut self) {
        let t = now();
        for (app_id, app) in self.apps.iter_mut() {
            let c = app.cached.clone();
            let near_deadline = app.license.as_ref().is_none_or(|l| l.exp - t < 2 * 86400);
            let licence_due = if app.licensed(t) {
                t - app.license_fetched_at > LICENSE_REFRESH_SECS
                    || (near_deadline && t - app.license_fetched_at > 600)
            } else {
                t >= app.license_retry_at
            };
            if !app.fetching_license && licence_due {
                app.fetching_license = true;
                log(format!("{app_id}: requesting a licence from {}", c.server));
                let (tx, id) = (self.tx.clone(), app_id.clone());
                std::thread::spawn(move || {
                    let result = net::fetch_license(&c.server, &c.app_id, &c.key, &c.bundle_id);
                    let _ = tx.send(Msg::License { app_id: id, result });
                });
            } else if app.licensed(t) && !app.fetching_feed && t >= app.next_feed_at {
                app.fetching_feed = true;
                let (tx, id) = (self.tx.clone(), app_id.clone());
                std::thread::spawn(move || {
                    let result = net::fetch_feed(&c.server, &c.app_id, &c.key);
                    let _ = tx.send(Msg::Feed { app_id: id, result });
                });
            }
            self.rotator.set_items(app_id, app.items(t));
        }

        if t - self.last_flush >= EVENT_FLUSH_SECS {
            self.flush_events();
            self.last_flush = t;
        }

        if t < self.hidden_until {
            if self.current.take().is_some() {
                let _ = self.ui.send(UiCmd::Hide);
            }
            return;
        }

        // Drop the current item early if its app lost the right to show it.
        let still_valid = self.current.as_ref().is_some_and(|(app_id, item)| {
            self.apps
                .get(app_id)
                .is_some_and(|a| a.items(t).iter().any(|i| i.id == item.id))
        });
        if still_valid && t < self.current_until {
            return;
        }
        match self.rotator.next(t) {
            Some(slot) => {
                let same = self
                    .current
                    .as_ref()
                    .is_some_and(|(a, i)| a == &slot.app_id && i == &slot.item);
                self.current_until = t + slot.item.duration_secs as i64;
                if let Some(app) = self.apps.get_mut(&slot.app_id) {
                    *app.events
                        .entry((slot.item.id.clone(), "impression"))
                        .or_default() += 1;
                }
                if !same {
                    let _ = self
                        .ui
                        .send(UiCmd::Show(self.display(&slot.app_id, &slot.item)));
                }
                self.current = Some((slot.app_id, slot.item));
            }
            None => {
                if self.current.take().is_some() {
                    let _ = self.ui.send(UiCmd::Hide);
                }
            }
        }
    }

    fn display(&self, app_id: &str, item: &Item) -> Display {
        let mut menu = Vec::new();
        let header = if item.label.is_empty() {
            item.title.clone()
        } else {
            format!("{} · {}", item.label, item.title)
        };
        menu.push(MenuEntry::Info(header));
        if let Some(card) = &item.card {
            if !card.body.is_empty() {
                menu.push(MenuEntry::Info(card.body.clone()));
            }
            for (i, b) in card.buttons.iter().enumerate() {
                menu.push(MenuEntry::Action {
                    id: i as u32,
                    label: b.label.clone(),
                });
            }
        }
        menu.push(MenuEntry::Separator);
        menu.push(MenuEntry::Action {
            id: MENU_HIDE,
            label: "Hide for 12 hours".into(),
        });
        menu.push(MenuEntry::Action {
            id: MENU_WHY,
            label: "Why am I seeing this?".into(),
        });

        let mut buttons = Vec::new();
        let primary = match &item.action {
            Action::OpenUrl { .. } => Some("Open site"),
            Action::DeepLink { .. } => Some("Open in app"),
            Action::Callback { .. } => Some("Open"),
            Action::Card | Action::None => None,
        };
        if let Some(p) = primary {
            buttons.push((MENU_PRIMARY, p.to_string()));
        }
        let card = item.card.clone().unwrap_or_default();
        buttons.extend(
            card.buttons
                .iter()
                .enumerate()
                .map(|(i, b)| (i as u32, b.label.clone())),
        );
        let body = if card.body.is_empty() {
            item.subtitle.clone()
        } else {
            card.body.clone()
        };
        Display {
            app_id: app_id.to_string(),
            item: item.clone(),
            menu,
            card: CardView {
                label: item.label.clone(),
                title: item.title.clone(),
                body,
                buttons,
            },
            click_opens_menu: matches!(item.action, Action::Card | Action::None),
        }
    }

    fn on_ui(&mut self, ev: UiEvent) {
        let Some((app_id, item)) = self.current.clone() else {
            return;
        };
        match ev {
            UiEvent::Clicked => {
                if let Some(app) = self.apps.get_mut(&app_id) {
                    *app.events.entry((item.id.clone(), "click")).or_default() += 1;
                }
                self.run_action(&app_id, &item, &item.action);
            }
            UiEvent::Menu(MENU_HIDE) => {
                self.hidden_until = now() + HIDE_SECS;
                log("hidden by the user for 12 hours");
            }
            UiEvent::Menu(MENU_PRIMARY) => {
                if let Some(app) = self.apps.get_mut(&app_id) {
                    *app.events.entry((item.id.clone(), "click")).or_default() += 1;
                }
                self.run_action(&app_id, &item, &item.action);
            }
            UiEvent::Menu(MENU_WHY) => {
                crate::platform::open_url(&format!("{WHY_URL}?app={app_id}"))
            }
            UiEvent::Menu(i) => {
                if let Some(b) = item.card.as_ref().and_then(|c| c.buttons.get(i as usize)) {
                    if let Some(app) = self.apps.get_mut(&app_id) {
                        *app.events.entry((item.id.clone(), "click")).or_default() += 1;
                    }
                    self.run_action(&app_id, &item, &b.action.clone());
                }
            }
        }
    }

    fn run_action(&self, app_id: &str, item: &Item, action: &Action) {
        let Some(app) = self.apps.get(app_id) else {
            return;
        };
        match action {
            Action::OpenUrl { .. } | Action::DeepLink { .. } => {
                match policy::resolve_url(action, app.allowed_prefixes()) {
                    Some(url) => crate::platform::open_url(url),
                    None => log(format!(
                        "{app_id}: blocked a URL that is not on the app's allowlist"
                    )),
                }
            }
            Action::Callback { name } => {
                for conn in &app.conns {
                    self.reply(
                        *conn,
                        json!({"event": "callback", "item_id": item.id, "name": name}),
                    );
                }
            }
            Action::Card | Action::None => {}
        }
    }

    fn flush_events(&mut self) {
        for app in self.apps.values_mut() {
            if app.events.is_empty() {
                continue;
            }
            let events: Vec<Value> = app
                .events
                .drain()
                .filter(|((id, _), _)| !id.starts_with("local"))
                .map(|((id, ty), n)| json!({"item_id": id, "type": ty, "count": n}))
                .collect();
            let c = app.cached.clone();
            std::thread::spawn(move || {
                if let Err(e) = net::post_events(&c.server, &c.app_id, &c.key, &events) {
                    log(format!("{}: events not sent: {e}", c.app_id));
                }
            });
        }
    }
}

pub const MENU_HIDE: u32 = 1000;
pub const MENU_WHY: u32 = 1001;
pub const MENU_PRIMARY: u32 = 1002;
