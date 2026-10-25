//! The only OS-specific part of the host: putting a bitmap in the bar and
//! reporting clicks. Each platform runs its own event loop on the main thread
//! and polls `UiCmd`s from the controller.

use std::sync::mpsc::{Receiver, Sender};

use barkit_core::model::Item;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "macos", windows)))]
mod other;
#[cfg(windows)]
mod windows;

/// Native menu entries; Windows uses these until it gets the rich card.
#[cfg_attr(target_os = "macos", allow(dead_code))]
#[derive(Debug, Clone)]
pub enum MenuEntry {
    Info(String),
    Action { id: u32, label: String },
    Separator,
}

/// The rich card shown on hover / right-click (the "popover").
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Default)]
pub struct CardView {
    /// "Sponsored", "Ad", or empty for status items.
    pub label: String,
    pub title: String,
    pub body: String,
    /// (event id, label): the first one is drawn as the primary button.
    pub buttons: Vec<(u32, String)>,
}

#[derive(Debug, Clone)]
pub struct Display {
    #[allow(dead_code)]
    pub app_id: String,
    pub item: Item,
    /// Shown on right-click, and on left-click when `click_opens_menu`.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub menu: Vec<MenuEntry>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub card: CardView,
    pub click_opens_menu: bool,
}

pub enum UiCmd {
    Show(Display),
    Hide,
    Quit,
}

#[derive(Debug, Clone, Copy)]
pub enum UiEvent {
    Clicked,
    Menu(u32),
}

/// Blocks on the OS event loop until `UiCmd::Quit`.
pub fn run(cmds: Receiver<UiCmd>, events: Sender<crate::controller::Msg>) {
    #[cfg(target_os = "macos")]
    macos::run(cmds, events);
    #[cfg(windows)]
    windows::run(cmds, events);
    #[cfg(not(any(target_os = "macos", windows)))]
    other::run(cmds, events);
}

/// Opens an already-allowlisted URL with the OS default handler. No shell is
/// involved, so the URL can't smuggle in extra commands.
pub fn open_url(url: &str) {
    crate::paths::log(format!("opening {url}"));
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
    #[cfg(windows)]
    windows::shell_open(url);
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
