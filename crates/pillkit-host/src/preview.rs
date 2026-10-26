//! `pillkit-preview`: shows the pill with built-in sample items, with no
//! server, licence or app needed. It exists to answer one question on a new
//! machine: does the widget appear in the right place? It only ever shows its
//! own items, each labelled "Preview", and never opens the IPC socket.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use pillkit_core::model::{Action, Card, Item, Theme};

use crate::controller::{Msg, MENU_PRIMARY};
use crate::paths::log;
use crate::platform::{self, CardView, Display, MenuEntry, UiCmd, UiEvent};

const MENU_QUIT: u32 = 1100;
const URL: &str = "https://github.com/tejamaddimsetty/pillkit";

fn item(id: &str, title: &str, subtitle: &str, bg: &str, fg: &str, body: &str) -> Item {
    Item {
        id: id.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        label: "Preview".into(),
        icon: Default::default(),
        theme: Theme {
            background: bg.into(),
            foreground: fg.into(),
        },
        action: Action::OpenUrl { url: URL.into() },
        card: Some(Card {
            body: body.into(),
            buttons: Vec::new(),
        }),
        weight: 1,
        duration_secs: 6,
        starts_at: None,
        ends_at: None,
    }
}

fn display(item: &Item) -> Display {
    Display {
        app_id: "preview".into(),
        item: item.clone(),
        menu: vec![
            MenuEntry::Info(format!("Preview · {}", item.title)),
            MenuEntry::Separator,
            MenuEntry::Action {
                id: MENU_PRIMARY,
                label: "Open pillkit on GitHub".into(),
            },
            MenuEntry::Action {
                id: MENU_QUIT,
                label: "Quit preview".into(),
            },
        ],
        card: CardView {
            label: "Preview".into(),
            title: item.title.clone(),
            body: item
                .card
                .as_ref()
                .map(|c| c.body.clone())
                .unwrap_or_default(),
            buttons: vec![
                (MENU_PRIMARY, "Open on GitHub".into()),
                (MENU_QUIT, "Quit preview".into()),
            ],
        },
        click_opens_menu: false,
    }
}

pub fn run() {
    log(format!(
        "pillkit preview {} started (sample items only; right-click the pill to quit)",
        env!("CARGO_PKG_VERSION")
    ));
    let items = [
        item(
            "p1",
            "Pill Kit preview",
            "Your app lives right here",
            "#E8F04A",
            "#2A2C05",
            "This is where a developer's sponsor, launch or status message appears.",
        ),
        item(
            "p2",
            "Acme Cloud",
            "Deploy in 30s. Try free.",
            "#111111",
            "#FFFFFF",
            "A sample sponsored item. Real ones are signed and labelled.",
        ),
        item(
            "p3",
            "Build passed",
            "main · 2 minutes ago",
            "#1F7A4D",
            "#FFFFFF",
            "A sample status item, the kind an app shows for itself.",
        ),
    ];
    let (tx, rx) = mpsc::channel::<Msg>();
    let (ui_tx, ui_rx) = mpsc::channel::<UiCmd>();
    std::thread::spawn(move || {
        let mut i = 0;
        let _ = ui_tx.send(UiCmd::Show(display(&items[i])));
        loop {
            match rx.recv_timeout(Duration::from_secs(6)) {
                Ok(Msg::Ui(UiEvent::Clicked)) | Ok(Msg::Ui(UiEvent::Menu(MENU_PRIMARY))) => {
                    platform::open_url(URL)
                }
                Ok(Msg::Ui(UiEvent::Menu(MENU_QUIT))) => {
                    log("preview quit by the user");
                    let _ = ui_tx.send(UiCmd::Quit);
                    return;
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {
                    i = (i + 1) % items.len();
                    let _ = ui_tx.send(UiCmd::Show(display(&items[i])));
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    });
    platform::run(ui_rx, tx);
}
