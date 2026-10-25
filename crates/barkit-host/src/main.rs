//! barkit-host: the one per-user process that owns the bar slot.
//!
//!   barkit-host run              start (what the login entry runs)
//!   barkit-host install          copy into the data dir and start at login
//!   barkit-host uninstall        remove the login entry, binary and app cache
//!   barkit-host render IN OUT    render an item JSON file to a PNG (design preview)
//!   barkit-host --version

mod autostart;
mod controller;
mod ipc;
mod net;
mod paths;
mod platform;

use std::sync::mpsc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => println!("barkit-host {}", env!("CARGO_PKG_VERSION")),
        Some("install") => {
            exit_on_err(autostart::install().map(|p| println!("installed to {}", p.display())))
        }
        Some("uninstall") => exit_on_err(autostart::uninstall()),
        Some("render") if args.len() == 3 => exit_on_err(render(&args[1], &args[2])),
        Some("run") | None => run(),
        _ => {
            eprintln!(
                "usage: barkit-host [run|install|uninstall|render <item.json> <out.png>|--version]"
            );
            std::process::exit(2);
        }
    }
}

fn exit_on_err(r: std::io::Result<()>) {
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn render(input: &str, output: &str) -> std::io::Result<()> {
    use barkit_core::render::{render, Fonts, Metrics, SLOT_WIDTH_PT};
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
                barkit_core::render::slot_width_override().unwrap_or(SLOT_WIDTH_PT),
            ),
        },
    );
    std::fs::write(output, r.to_png())
}

fn run() {
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
    paths::log(format!("barkit-host {} started", env!("CARGO_PKG_VERSION")));
    let controller = controller::Controller::new(tx.clone(), ui_tx);
    std::thread::spawn(move || controller.run(rx));
    platform::run(ui_rx, tx);
    paths::log("barkit-host stopped");
}
