//! Linux and everything else: not drawn yet (planned for v0.3 via
//! StatusNotifierItem and a GNOME Shell extension). The host still runs, so
//! licensing, feeds and the SDK protocol can be developed and tested here.

use std::sync::mpsc::{Receiver, Sender};

use super::UiCmd;
use crate::paths::log;

pub fn run(cmds: Receiver<UiCmd>, _events: Sender<crate::controller::Msg>) {
    log("no bar integration on this OS yet; logging what would be shown");
    for cmd in cmds {
        match cmd {
            UiCmd::Show(d) => log(format!(
                "show [{}] {} — {}",
                d.app_id, d.item.title, d.item.subtitle
            )),
            UiCmd::Hide => log("hide"),
            UiCmd::Quit => return,
        }
    }
}
