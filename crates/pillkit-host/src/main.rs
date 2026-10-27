//! pillkit-host: the one per-user process that owns the bar slot.
//!
//!   pillkit-host run              start (what the login entry runs)
//!   pillkit-host preview          show built-in sample items (no server needed)
//!   pillkit-host install          copy into the data dir and start at login
//!   pillkit-host uninstall        remove the login entry, binary and app cache
//!   pillkit-host render IN OUT    render an item JSON file to a PNG (design preview)
//!   pillkit-host render-card OUT  render the hover card to a PNG (design preview)
//!   pillkit-host --version

// Release builds on Windows have no console window (the host starts at login).
#![cfg_attr(
    all(windows, not(debug_assertions), not(feature = "dev-keys")),
    windows_subsystem = "windows"
)]

mod autostart;
mod controller;
mod ipc;
mod net;
mod paths;
mod platform;
mod preview;

use std::sync::mpsc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => println!("pillkit-host {}", env!("CARGO_PKG_VERSION")),
        Some("install") => {
            exit_on_err(autostart::install().map(|p| println!("installed to {}", p.display())))
        }
        Some("uninstall") => exit_on_err(autostart::uninstall()),
        Some("render") if args.len() == 3 => exit_on_err(render(&args[1], &args[2])),
        Some("render-card") if args.len() == 2 => exit_on_err(render_card(&args[1])),
        Some("preview") => preview::run(),
        // The preview build is the same binary under another name, so a
        // double-click (no arguments) on pillkit-preview.exe opens the preview.
        None if is_preview_binary() => preview::run(),
        Some("run") | None => run(),
        _ => {
            eprintln!(
                "usage: pillkit-host [run|install|uninstall|render <item.json> <out.png>|--version]"
            );
            std::process::exit(2);
        }
    }
}

fn is_preview_binary() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
        .is_some_and(|s| s.contains("preview"))
}

fn exit_on_err(r: std::io::Result<()>) {
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn render(input: &str, output: &str) -> std::io::Result<()> {
    use pillkit_core::render::{render, Fonts, Metrics, SLOT_WIDTH_PT};
    let item =
        serde_json::from_str(&std::fs::read_to_string(input)?).map_err(std::io::Error::other)?;
    let fonts = Fonts::system().ok_or_else(|| std::io::Error::other("no system font found"))?;
    let r = render(
        &item,
        &fonts,
        Metrics {
            height: 60,
            scale: 2.0,
            max_width_pt: 480.0,
            fixed_width_pt: Some(
                pillkit_core::render::slot_width_override().unwrap_or(SLOT_WIDTH_PT),
            ),
        },
    );
    std::fs::write(output, r.to_png())
}

/// Design preview of the hover card (as drawn on Windows).
fn render_card(output: &str) -> std::io::Result<()> {
    use pillkit_core::render::{render_card, CardContent, Fonts};
    let fonts = Fonts::system().ok_or_else(|| std::io::Error::other("no system font found"))?;
    let card = CardContent {
        label: "Sponsored".into(),
        title: "Acme Cloud".into(),
        body: "Deploy any repo in 30 seconds. First month free.".into(),
        links: vec![(1, "Hide".into()), (2, "Why this?".into())],
        buttons: vec![(3, "Open site".into())],
    };
    let (r, _) = render_card(&card, &fonts, 2.0, false);
    std::fs::write(output, r.to_png())
}

fn run() {
    // The host usually runs hidden, so make sure a crash leaves a trace.
    std::panic::set_hook(Box::new(|info| paths::log(format!("CRASH: {info}"))));
    let (tx, rx) = mpsc::channel();
    let (ui_tx, ui_rx) = mpsc::channel();
    if let Err(e) = ipc::listen(tx.clone()) {
        paths::log(format!("not starting: {e}"));
        std::process::exit(if e.kind() == std::io::ErrorKind::AddrInUse {
            0
        } else {
            1
        });
    }
    paths::log(format!(
        "pillkit-host {} started",
        env!("CARGO_PKG_VERSION")
    ));
    let controller = controller::Controller::new(tx.clone(), ui_tx);
    std::thread::spawn(move || controller.run(rx));
    platform::run(ui_rx, tx);
    paths::log("pillkit-host stopped");
}
