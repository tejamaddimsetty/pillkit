//! Windows: puts the pill in the taskbar.
//!
//! Three modes, tried in order (override with `PILLKIT_WIN_MODE`):
//!
//! 1. `embedded`: our layered window becomes a child of the taskbar
//!    (`Shell_TrayWnd`), so it moves, hides and stacks with it. On Windows 11
//!    it sits in the empty left side when icons are centred (the weather
//!    widget's spot); otherwise just left of the tray. On Windows 10 we narrow
//!    the task-button strip to make room.
//! 2. `overlay`: a top-most, no-activate window placed over the same spot,
//!    re-asserted on every tick. Used if embedding fails.
//! 3. `tray`: a notification-area icon with a tooltip. Always works.
//!
//! No code is injected into Explorer, so antivirus and SmartScreen stay quiet.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use pillkit_core::render::{
    render, render_card, CardContent, Fonts, Hit, Metrics, Rendered, SLOT_WIDTH_PT,
};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    BOOL, COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP,
    NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, QUNS_BUSY, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{Display, MenuEntry, UiCmd, UiEvent};
use crate::controller::{Msg, MENU_HIDE, MENU_WHY};
use crate::paths::log;

const TIMER_POLL: usize = 1;
const TICK_MS: u32 = 200;
/// Placement is re-checked every this many ticks (2 s).
const LAYOUT_EVERY: u32 = 10;
const HOVER_OPEN_MS: u128 = 450;
const HOVER_CLOSE_MS: u128 = 400;
const WM_TRAY: u32 = WM_APP + 1;
const MARGIN_PT: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Embedded,
    Overlay,
    Tray,
}

struct State {
    cmds: Receiver<UiCmd>,
    events: Sender<Msg>,
    fonts: Option<Fonts>,
    hwnd: HWND,
    mode: Mode,
    forced: Option<Mode>,
    display: Option<Display>,
    rendered: Option<Rc<Rendered>>,
    rendered_width_pt: f32,
    /// The hover card: its window, click areas and screen rect while shown.
    card_hwnd: HWND,
    card_hits: Vec<Hit>,
    card_rect: Option<RECT>,
    hover_since: Option<std::time::Instant>,
    outside_since: Option<std::time::Instant>,
    tray_added: bool,
    /// The tray fallback's icon, drawn from the current item (not a stock icon).
    tray_icon: Option<(String, HICON)>,
    ticks: u32,
    win10_shrunk: Option<(HWND, i32)>,
    last_placed: Option<(Mode, RECT)>,
    /// What the window last showed (bitmap and position), to skip redraws.
    last_blit: Option<(usize, i32, i32)>,
    placement_note: &'static str,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// The "TaskbarCreated" message id, kept outside STATE so the window
/// procedure can check it without borrowing.
static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Runs `f` on the window state, or does nothing if the state is already in
/// use. Windows sends messages to our window *during* calls like SetParent and
/// SetWindowPos that we make while holding the state; those nested messages
/// must not borrow it again (that panicked, and a panic in a window procedure
/// aborts the process).
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| match s.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().map(f),
        Err(_) => None,
    })
}

pub fn shell_open(url: &str) {
    // Its own thread with COM set up, as ShellExecute expects; whatever it
    // waits on (the browser starting, Explorer) never touches the pill.
    let url = url.to_string();
    std::thread::spawn(move || unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    });
}

pub fn run(cmds: Receiver<UiCmd>, events: Sender<Msg>) {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("PillKitHostWidget");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            class,
            w!("pillkit"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            instance,
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                log(format!("could not create the widget window: {e}"));
                return;
            }
        };

        let card_class = w!("PillKitCard");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(card_wndproc),
            hInstance: instance.into(),
            lpszClassName: card_class,
            hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
            ..Default::default()
        });
        let card_hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            card_class,
            w!("Pill Kit card"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            instance,
            None,
        )
        .unwrap_or_default();
        start_taskbar_scanner();

        let forced = match std::env::var("PILLKIT_WIN_MODE").as_deref() {
            Ok("embedded") => Some(Mode::Embedded),
            Ok("overlay") => Some(Mode::Overlay),
            Ok("tray") => Some(Mode::Tray),
            _ => None,
        };
        let state = State {
            cmds,
            events,
            fonts: Fonts::system(),
            hwnd,
            mode: forced.unwrap_or(Mode::Embedded),
            forced,
            display: None,
            rendered: None,
            rendered_width_pt: 0.0,
            card_hwnd,
            card_hits: Vec::new(),
            card_rect: None,
            hover_since: None,
            outside_since: None,
            tray_added: false,
            tray_icon: None,
            ticks: 0,
            win10_shrunk: None,
            last_placed: None,
            last_blit: None,
            placement_note: "",
        };
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            std::sync::atomic::Ordering::Relaxed,
        );
        STATE.with(|s| *s.borrow_mut() = Some(state));
        SetTimer(hwnd, TIMER_POLL, TICK_MS, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        with_state(|s| {
            remove_tray(s);
            restore_win10(s);
        });
    }
}

/// A panic inside a window procedure aborts the process; catch it, log it,
/// and let Windows handle the message normally.
fn guarded(name: &str, f: impl FnOnce() -> LRESULT, fallback: impl FnOnce() -> LRESULT) -> LRESULT {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => {
            log(format!("recovered from an error in {name}; carrying on"));
            fallback()
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    guarded(
        "the pill window",
        || unsafe { wndproc_inner(hwnd, msg, wp, lp) },
        || unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    )
}

unsafe extern "system" fn card_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    guarded(
        "the card window",
        || unsafe { card_wndproc_inner(hwnd, msg, wp, lp) },
        || unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    )
}

unsafe fn wndproc_inner(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wp.0 == TIMER_POLL => {
            on_tick();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            on_click(false);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            on_click(true);
            LRESULT(0)
        }
        WM_TRAY => {
            match (lp.0 as u32) & 0xFFFF {
                x if x == WM_LBUTTONUP => on_click(false),
                x if x == WM_RBUTTONUP || x == WM_CONTEXTMENU => on_click(true),
                _ => {}
            }
            LRESULT(0)
        }
        WM_DPICHANGED | WM_SETTINGCHANGE | WM_DISPLAYCHANGE => {
            with_state(|s| {
                s.rendered = None;
                layout(s);
            });
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => {
            // Explorer restarted: the old taskbar (and our parent) is gone.
            let created = TASKBAR_CREATED.load(std::sync::atomic::Ordering::Relaxed);
            if created != 0 && msg == created {
                log("taskbar was recreated; re-attaching");
                with_state(|s| {
                    s.tray_added = false;
                    s.win10_shrunk = None;
                    s.mode = s.forced.unwrap_or(Mode::Embedded);
                    s.rendered = None;
                    s.last_blit = None;
                    layout(s);
                });
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }
}

fn on_tick() {
    let quit = with_state(|s| {
        let mut changed = false;
        loop {
            match s.cmds.try_recv() {
                Ok(UiCmd::Show(d)) => {
                    s.display = Some(d);
                    s.rendered = None;
                    changed = true;
                }
                Ok(UiCmd::Hide) => {
                    s.display = None;
                    s.rendered = None;
                    changed = true;
                }
                Ok(UiCmd::Quit) | Err(TryRecvError::Disconnected) => return true,
                Err(TryRecvError::Empty) => break,
            }
        }
        s.ticks = s.ticks.wrapping_add(1);
        // Re-check placement every 2 s: alignment, auto-hide, monitors and
        // Explorer's own relayouts can all move the free space.
        if changed || s.ticks % LAYOUT_EVERY == 0 || (s.mode == Mode::Overlay && s.ticks % 2 == 0) {
            layout(s);
            if changed && s.card_rect.is_some() {
                show_card(s); // keep an open card in step with the item
            }
        }
        track_hover(s);
        false
    })
    .unwrap_or(true);
    if quit {
        unsafe { PostQuitMessage(0) };
    }
}

fn on_click(right: bool) {
    let Some((events, display, hwnd)) =
        with_state(|s| (s.events.clone(), s.display.clone(), s.hwnd))
    else {
        return;
    };
    let Some(d) = display else { return };
    let in_tray = with_state(|s| s.mode == Mode::Tray || s.tray_added).unwrap_or(false);
    if right || d.click_opens_menu {
        if in_tray || with_state(show_card) != Some(true) {
            if let Some(id) = popup_menu(hwnd, &d.menu) {
                let _ = events.send(Msg::Ui(UiEvent::Menu(id)));
            }
        }
    } else {
        with_state(hide_card);
        let _ = events.send(Msg::Ui(UiEvent::Clicked));
    }
}

fn popup_menu(hwnd: HWND, entries: &[MenuEntry]) -> Option<u32> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for e in entries {
            let _ = match e {
                MenuEntry::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()),
                MenuEntry::Info(t) => {
                    AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, &HSTRING::from(t.as_str()))
                }
                MenuEntry::Action { id, label } => AppendMenuW(
                    menu,
                    MF_STRING,
                    (*id as usize) + 1,
                    &HSTRING::from(label.as_str()),
                ),
            };
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        let id = chosen.0 as u32;
        (id > 0).then(|| id - 1)
    }
}

fn find_taskbar() -> Option<HWND> {
    unsafe {
        FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null())
            .ok()
            .filter(|h| !h.is_invalid())
    }
}

fn rect_of(h: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(h, &mut r).ok()? };
    Some(r)
}

fn child(parent: HWND, class: PCWSTR) -> Option<HWND> {
    unsafe {
        FindWindowExW(parent, None, class, PCWSTR::null())
            .ok()
            .filter(|h| !h.is_invalid())
    }
}

fn windows_build() -> u32 {
    let mut buf = [0u16; 32];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if r.is_err() {
        return 0;
    }
    String::from_utf16_lossy(&buf)
        .trim_end_matches('\0')
        .parse()
        .unwrap_or(0)
}

fn fullscreen_app_running() -> bool {
    unsafe {
        SHQueryUserNotificationState()
            .map(|s| {
                s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE
            })
            .unwrap_or(false)
    }
}

/// What the background scanner last saw on the taskbar.
enum Scan {
    /// No result yet: keep the pill hidden rather than guess.
    Pending,
    /// UI Automation can't read the taskbar: placement is estimated.
    Unavailable,
    Ready(Vec<(i32, i32)>),
}

static SCAN: std::sync::Mutex<Scan> = std::sync::Mutex::new(Scan::Pending);
/// Set by the window thread while a pill should be on the taskbar.
static SCAN_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Reads the taskbar's item positions every 1.5 s on its own thread.
///
/// This must never run on the window thread: our pill is a child of
/// Explorer's taskbar, so Windows ties our input queue to Explorer's. A
/// blocking cross-process UI Automation call from that thread can deadlock
/// with Explorer (for example while it delivers a click to the pill) and
/// freeze the whole desktop until it times out.
fn start_taskbar_scanner() {
    std::thread::spawn(|| unsafe {
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
        };
        use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: windows::core::Result<IUIAutomation> =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER);
        let Ok(uia) = uia else {
            log("UI Automation unavailable; placement will be estimated");
            *SCAN.lock().unwrap_or_else(|e| e.into_inner()) = Scan::Unavailable;
            return;
        };
        let mut failures = 0;
        loop {
            if !SCAN_WANTED.load(std::sync::atomic::Ordering::Relaxed) {
                // Nothing on the taskbar (no item, tray mode, or a full-screen
                // app): don't touch Explorer at all.
                std::thread::sleep(std::time::Duration::from_millis(1000));
                continue;
            }
            if let Some(tb) = find_taskbar() {
                if let Some(r) = rect_of(tb) {
                    let found = taskbar_items(&uia, tb, r);
                    let mut scan = SCAN.lock().unwrap_or_else(|e| e.into_inner());
                    match found {
                        Some(v) => {
                            failures = 0;
                            *scan = Scan::Ready(v);
                        }
                        None => {
                            failures += 1;
                            if failures >= 3 && matches!(*scan, Scan::Pending) {
                                log("UI Automation can't read the taskbar; placement will be estimated");
                                *scan = Scan::Unavailable;
                            }
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(3000));
        }
    });
}

fn scan_items() -> Vec<(i32, i32)> {
    match &*SCAN.lock().unwrap_or_else(|e| e.into_inner()) {
        Scan::Ready(v) => v.clone(),
        _ => Vec::new(),
    }
}

/// Screen x-ranges of the taskbar's own items (Start, Search, app buttons,
/// widgets, tray icons, clock), as reported by UI Automation. Merged and
/// sorted. None if UI Automation can't read the taskbar.
fn taskbar_items(
    uia: &windows::Win32::UI::Accessibility::IUIAutomation,
    taskbar: HWND,
    tb: RECT,
) -> Option<Vec<(i32, i32)>> {
    use windows::Win32::UI::Accessibility::*;
    let tb_w = tb.right - tb.left;
    let mut ranges = Vec::new();
    unsafe {
        let root = uia.ElementFromHandle(taskbar).ok()?;
        let all = root
            .FindAll(TreeScope_Descendants, &uia.CreateTrueCondition().ok()?)
            .ok()?;
        for i in 0..all.Length().ok()? {
            let Ok(e) = all.GetElement(i) else { continue };
            let Ok(ct) = e.CurrentControlType() else {
                continue;
            };
            let interactive = [
                UIA_ButtonControlTypeId,
                UIA_ListItemControlTypeId,
                UIA_MenuItemControlTypeId,
                UIA_EditControlTypeId,
                UIA_SplitButtonControlTypeId,
                UIA_ImageControlTypeId,
                UIA_TextControlTypeId,
            ];
            if !interactive.contains(&ct) {
                continue;
            }
            if e.CurrentIsOffscreen().map(|b| b.as_bool()).unwrap_or(false) {
                continue;
            }
            let Ok(r) = e.CurrentBoundingRectangle() else {
                continue;
            };
            let w = r.right - r.left;
            // Skip empty boxes, anything outside the bar, and containers that
            // span most of it.
            if w <= 0 || r.bottom <= tb.top || r.top >= tb.bottom || w > tb_w * 2 / 5 {
                continue;
            }
            ranges.push((r.left.max(tb.left), r.right.min(tb.right)));
        }
    }
    if ranges.is_empty() {
        return None;
    }
    ranges.sort();
    let mut merged: Vec<(i32, i32)> = Vec::new();
    for (l, r) in ranges {
        match merged.last_mut() {
            Some(last) if l <= last.1 + 2 => last.1 = last.1.max(r),
            _ => merged.push((l, r)),
        }
    }
    Some(merged)
}

/// Taskbar items from the scanner: Err while the first scan is pending,
/// Ok(None) if UI Automation is unavailable.
fn occupied() -> Result<Option<Vec<(i32, i32)>>, ()> {
    match &*SCAN.lock().unwrap_or_else(|e| e.into_inner()) {
        Scan::Pending => Err(()),
        Scan::Unavailable => Ok(None),
        Scan::Ready(v) => Ok(Some(v.clone())),
    }
}

/// Finds free space for a `w`x`h` pill, in screen coordinates, without
/// covering anything already on the taskbar. Prefers the empty stretch next
/// to the weather widget, then the stretch just left of the tray.
fn target_rect(s: &mut State, taskbar: HWND, w: i32, h: i32, scale: f32) -> Option<RECT> {
    let tb = rect_of(taskbar)?;
    if (tb.bottom - tb.top) > (tb.right - tb.left) {
        return None; // Side-docked taskbars (Windows 10) have no room for a pill.
    }
    let margin = (MARGIN_PT * scale) as i32;
    let y = tb.top + ((tb.bottom - tb.top) - h) / 2;
    let rect = |x: i32| RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };

    if let Ok(Some(mut items)) = occupied() {
        // Never treat our own pill as something in the way.
        if let Some(own) = rect_of(s.hwnd) {
            items.retain(|&(l, r)| (l - own.left).abs() > 2 || (r - own.right).abs() > 2);
        }
        // Gaps between the items, plus the ends of the bar.
        let mut gaps = Vec::new();
        let mut cursor = tb.left;
        for &(l, r) in &items {
            if l > cursor {
                gaps.push((cursor, l));
            }
            cursor = cursor.max(r);
        }
        if tb.right > cursor {
            gaps.push((cursor, tb.right));
        }
        // Stay where we are while that spot is still free, so the pill doesn't
        // hop sides every time an app opens and the gaps change size.
        if let Some((_, prev)) = s.last_placed {
            let same_size = prev.right - prev.left == w;
            let inside = prev.left >= tb.left + margin && prev.right <= tb.right - margin;
            let clear = items
                .iter()
                .all(|&(l, r)| r + margin <= prev.left || l >= prev.right + margin);
            if same_size && inside && clear {
                return Some(rect(prev.left));
            }
        }
        let fits = |g: &&(i32, i32)| g.1 - g.0 >= w + 2 * margin;
        let mid = (tb.left + tb.right) / 2;
        if let Some(g) = gaps.iter().filter(fits).find(|g| g.0 < mid) {
            s.placement_note = "first free space from the left (beside the weather widget)";
            return Some(rect(g.0 + margin));
        }
        if let Some(g) = gaps.iter().filter(fits).rev().find(|g| g.1 > mid) {
            s.placement_note = "free space left of the tray";
            return Some(rect(g.1 - w - margin));
        }
        s.placement_note = "no free space this wide";
        return None;
    }

    // UI Automation unavailable: fall back to an estimate.
    let tray_left = child(taskbar, w!("TrayNotifyWnd"))
        .and_then(rect_of)
        .map(|r| r.left)
        .unwrap_or(tb.right - (320.0 * scale) as i32);
    s.placement_note = "estimated (UI Automation unavailable)";
    Some(rect(tray_left - w - margin))
}

/// Windows 10: narrow the task-button strip so it doesn't run under the pill.
fn shrink_win10(s: &mut State, taskbar: HWND, pill_left_screen: i32) {
    if windows_build() >= 22000 {
        return;
    }
    let Some(rebar) = child(taskbar, w!("ReBarWindow32")) else {
        return;
    };
    let Some(tasks) = child(rebar, w!("MSTaskSwWClass")) else {
        return;
    };
    let (Some(rr), Some(tr)) = (rect_of(rebar), rect_of(tasks)) else {
        return;
    };
    let want = pill_left_screen - tr.left - 4;
    if want > 0 && tr.right > pill_left_screen {
        unsafe {
            let _ = MoveWindow(
                tasks,
                tr.left - rr.left,
                tr.top - rr.top,
                want,
                tr.bottom - tr.top,
                true,
            );
        }
        if s.win10_shrunk.is_none() {
            s.win10_shrunk = Some((tasks, tr.right - tr.left));
        }
    }
}

fn restore_win10(s: &mut State) {
    if let Some((tasks, width)) = s.win10_shrunk.take() {
        if let Some(r) = rect_of(tasks) {
            unsafe {
                let _ = MoveWindow(tasks, 0, 0, width, r.bottom - r.top, true);
            }
        }
    }
}

fn layout(s: &mut State) {
    let Some(display) = s.display.clone() else {
        SCAN_WANTED.store(false, std::sync::atomic::Ordering::Relaxed);
        s.last_blit = None;
        unsafe {
            let _ = ShowWindow(s.hwnd, SW_HIDE);
        }
        remove_tray(s);
        restore_win10(s);
        return;
    };
    let Some(taskbar) = find_taskbar() else {
        s.mode = Mode::Tray;
        return show_tray(s, &display);
    };
    if s.mode == Mode::Tray {
        SCAN_WANTED.store(false, std::sync::atomic::Ordering::Relaxed);
        return show_tray(s, &display);
    }
    SCAN_WANTED.store(true, std::sync::atomic::Ordering::Relaxed);

    let scale = unsafe { GetDpiForWindow(taskbar) } as f32 / 96.0;
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let tb = match rect_of(taskbar) {
        Some(r) => r,
        None => return,
    };
    let bar_h = (tb.bottom - tb.top) as f32;
    let pill_h = (bar_h * 0.72).min(40.0 * scale).max(20.0 * scale).round() as u32;

    // Until the scanner has looked at the taskbar, stay hidden rather than
    // guess (a guess can land on Start or Search).
    if s.mode != Mode::Tray && occupied().is_err() {
        unsafe {
            let _ = ShowWindow(s.hwnd, SW_HIDE);
        }
        return;
    }
    // Full width first, then narrower, so the pill never covers anything.
    let full = pillkit_core::render::slot_width_override().unwrap_or(SLOT_WIDTH_PT);
    let mut placed = None;
    // Last steps: a compact one-line pill, then just the icon (a square).
    let icon_only = pill_h as f32 / scale;
    let mut sizes = pillkit_core::render::slot_widths(icon_only);
    sizes[0] = full;
    for width_pt in sizes.into_iter().map(|w| w.min(full)) {
        if s.rendered.as_ref().map(|r| r.height) != Some(pill_h) || s.rendered_width_pt != width_pt
        {
            let Some(fonts) = s.fonts.as_ref() else {
                log("no font available; falling back to the tray");
                s.mode = Mode::Tray;
                return show_tray(s, &display);
            };
            s.rendered = Some(Rc::new(render(
                &display.item,
                fonts,
                Metrics {
                    height: pill_h,
                    scale,
                    max_width_pt: 480.0,
                    fixed_width_pt: Some(width_pt),
                },
            )));
            s.rendered_width_pt = width_pt;
        }
        let r = s.rendered.clone().expect("just rendered");
        if let Some(t) = target_rect(s, taskbar, r.width as i32, r.height as i32, scale) {
            placed = Some(t);
            break;
        }
    }
    let r = s.rendered.clone().expect("just rendered");
    let r = r.as_ref();
    let (w, h) = (r.width as i32, r.height as i32);

    let Some(target) = placed else {
        if !s.tray_added {
            log(format!(
                "no room on the taskbar ({}); using the tray until there is. taskbar {:?}; items {:?}; scale {scale}",
                s.placement_note,
                (tb.left, tb.top, tb.right, tb.bottom),
                scan_items()
            ));
        }
        // Temporary: keep re-measuring and move back as soon as it fits.
        s.last_blit = None;
        hide_card(s);
        return show_tray(s, &display);
    };
    remove_tray(s);

    if s.last_placed
        .map(|(m, r)| (m, r.left, r.top, r.right, r.bottom))
        != Some((s.mode, target.left, target.top, target.right, target.bottom))
    {
        log(format!(
            "placing pill: mode {:?}, at ({}, {}) size {}x{}; {}; taskbar ({}, {})-({}, {}); items {:?}; scale {scale}; build {}",
            s.mode, target.left, target.top, w, h, s.placement_note, tb.left, tb.top, tb.right, tb.bottom,
            scan_items(), windows_build()
        ));
        s.last_placed = Some((s.mode, target));
    }
    let fullscreen = fullscreen_app_running();
    SCAN_WANTED.store(!fullscreen, std::sync::atomic::Ordering::Relaxed);
    // Same bitmap at the same spot: nothing to redraw.
    let key = (
        Rc::as_ptr(s.rendered.as_ref().expect("rendered")) as usize,
        target.left,
        target.top,
    );
    let unchanged = s.last_blit == Some(key);
    unsafe {
        match s.mode {
            Mode::Embedded => {
                if GetParent(s.hwnd).ok() != Some(taskbar) {
                    let style = GetWindowLongPtrW(s.hwnd, GWL_STYLE);
                    SetWindowLongPtrW(
                        s.hwnd,
                        GWL_STYLE,
                        (style & !(WS_POPUP.0 as isize)) | WS_CHILD.0 as isize,
                    );
                    if SetParent(s.hwnd, taskbar).is_err()
                        || GetParent(s.hwnd).ok() != Some(taskbar)
                    {
                        log("could not embed in the taskbar; switching to overlay mode");
                        SetWindowLongPtrW(s.hwnd, GWL_STYLE, style);
                        unembed(s.hwnd);
                        s.mode = Mode::Overlay;
                        return layout(s);
                    }
                    log("embedded in the taskbar");
                }
                // Child coordinates are relative to the taskbar.
                let pos = POINT {
                    x: target.left - tb.left,
                    y: target.top - tb.top,
                };
                if !unchanged && !blit(s.hwnd, r, pos, true) {
                    log("layered child window not supported; switching to overlay mode");
                    unembed(s.hwnd);
                    s.mode = Mode::Overlay;
                    return layout(s);
                }
                s.last_blit = Some(key);
                shrink_win10(s, taskbar, target.left);
                let _ = ShowWindow(s.hwnd, SW_SHOWNOACTIVATE);
            }
            Mode::Overlay => {
                if fullscreen {
                    let _ = ShowWindow(s.hwnd, SW_HIDE);
                    return;
                }
                if !unchanged
                    && !blit(
                        s.hwnd,
                        r,
                        POINT {
                            x: target.left,
                            y: target.top,
                        },
                        false,
                    )
                {
                    log("overlay window could not be drawn; switching to the tray");
                    let _ = ShowWindow(s.hwnd, SW_HIDE);
                    s.mode = Mode::Tray;
                    return show_tray(s, &display);
                }
                s.last_blit = Some(key);
                // The taskbar raises itself when clicked; push back on top.
                let _ = SetWindowPos(
                    s.hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
                let _ = ShowWindow(s.hwnd, SW_SHOWNOACTIVATE);
            }
            Mode::Tray => unreachable!(),
        }
    }
}

/// Turns our window back into a normal top-level popup after embedding failed.
/// Without this it stays a WS_CHILD with no parent, which Windows never shows.
unsafe fn unembed(hwnd: HWND) {
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    SetWindowLongPtrW(
        hwnd,
        GWL_STYLE,
        (style & !(WS_CHILD.0 as isize)) | WS_POPUP.0 as isize,
    );
    let _ = SetParent(hwnd, None);
    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
}

/// Pushes the premultiplied bitmap into the layered window at `pos`.
fn blit(hwnd: HWND, r: &Rendered, pos: POINT, child: bool) -> bool {
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(screen);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: r.width as i32,
                biHeight: -(r.height as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let ok = match CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) if !bits.is_null() => {
                let pixels = r.to_bgra_premultiplied();
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
                let old = SelectObject(mem, bmp);
                let size = SIZE {
                    cx: r.width as i32,
                    cy: r.height as i32,
                };
                let src = POINT::default();
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                // For a child window the destination is set with SetWindowPos.
                let dst = if child {
                    None
                } else {
                    Some(&pos as *const POINT)
                };
                let res = UpdateLayeredWindow(
                    hwnd,
                    screen,
                    dst,
                    Some(&size),
                    mem,
                    Some(&src),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                );
                if let Err(e) = &res {
                    log(format!("UpdateLayeredWindow failed (child: {child}): {e}"));
                }
                if child && res.is_ok() {
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOP,
                        pos.x,
                        pos.y,
                        size.cx,
                        size.cy,
                        SWP_NOACTIVATE,
                    );
                }
                SelectObject(mem, old);
                let _ = DeleteObject(bmp);
                res.is_ok()
            }
            _ => false,
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        ok
    }
}

/// A 32x32 icon of the item (its PNG or monogram on its colour), for the tray.
fn item_icon(item: &pillkit_core::model::Item, fonts: &Fonts) -> Option<HICON> {
    let r = render(
        item,
        fonts,
        Metrics {
            height: 32,
            scale: 1.0,
            max_width_pt: 32.0,
            fixed_width_pt: Some(32.0),
        },
    );
    let (w, h) = (r.width as i32, r.height as i32);
    let pixels = r.to_bgra_straight();
    let mask = vec![0u8; (((w + 15) / 16) * 2 * h) as usize];
    unsafe {
        let color = CreateBitmap(w, h, 1, 32, Some(pixels.as_ptr() as *const _));
        let mask = CreateBitmap(w, h, 1, 1, Some(mask.as_ptr() as *const _));
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color);
        let _ = DeleteObject(mask);
        icon
    }
}

fn tray_data(s: &State, tip: &str) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: s.hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_TIP | NIF_ICON,
        uCallbackMessage: WM_TRAY,
        hIcon: s
            .tray_icon
            .as_ref()
            .map(|(_, i)| *i)
            .unwrap_or_else(|| unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() }),
        ..Default::default()
    };
    for (dst, src) in nid.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
        *dst = src;
    }
    nid
}

fn show_tray(s: &mut State, d: &Display) {
    unsafe {
        let _ = ShowWindow(s.hwnd, SW_HIDE);
    }
    let mut tip = d.item.title.clone();
    if !d.item.subtitle.is_empty() {
        tip = format!("{tip} — {}", d.item.subtitle);
    }
    if !d.item.label.is_empty() {
        tip = format!("{} · {tip}", d.item.label);
    }
    if s.tray_icon.as_ref().map(|(id, _)| id.as_str()) != Some(d.item.id.as_str()) {
        if let Some(icon) = s.fonts.as_ref().and_then(|f| item_icon(&d.item, f)) {
            if let Some((_, old)) = s.tray_icon.replace((d.item.id.clone(), icon)) {
                unsafe {
                    let _ = DestroyIcon(old);
                }
            }
        }
    }
    let nid = tray_data(s, &tip);
    let ok: BOOL =
        unsafe { Shell_NotifyIconW(if s.tray_added { NIM_MODIFY } else { NIM_ADD }, &nid) };
    if ok.as_bool() {
        s.tray_added = true;
    }
}

fn remove_tray(s: &mut State) {
    if s.tray_added {
        let nid = tray_data(s, "");
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
        s.tray_added = false;
    }
}

// ---------------------------------------------------------------- hover card

/// Windows' own light/dark setting for apps.
fn dark_mode() -> bool {
    let mut data = 1u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    r.is_ok() && data == 0
}

/// Shows (or refreshes) the card above the pill. Returns false if there's
/// nothing to show it for.
fn show_card(s: &mut State) -> bool {
    let (Some(d), Some(fonts)) = (s.display.as_ref(), s.fonts.as_ref()) else {
        return false;
    };
    if s.mode == Mode::Tray || s.tray_added || s.card_hwnd.is_invalid() {
        return false;
    }
    let Some(pill) = rect_of(s.hwnd) else {
        return false;
    };
    let scale = unsafe { GetDpiForWindow(s.hwnd) } as f32 / 96.0;
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let content = CardContent {
        label: d.card.label.clone(),
        title: d.card.title.clone(),
        body: d.card.body.clone(),
        links: vec![(MENU_HIDE, "Hide".into()), (MENU_WHY, "Why this?".into())],
        buttons: d.card.buttons.clone(),
    };
    let (r, hits) = render_card(&content, fonts, scale, dark_mode());
    let (w, h) = (r.width as i32, r.height as i32);

    // Above the pill (below it if the taskbar is at the top), on screen.
    let tb = find_taskbar().and_then(rect_of).unwrap_or(pill);
    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let x = (pill.left + (pill.right - pill.left) / 2 - w / 2).clamp(0, (screen_w - w).max(0));
    let y = if tb.top > 0 { tb.top - h } else { tb.bottom };
    if !blit(s.card_hwnd, &r, POINT { x, y }, false) {
        return false;
    }
    unsafe {
        let _ = SetWindowPos(
            s.card_hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let _ = ShowWindow(s.card_hwnd, SW_SHOWNOACTIVATE);
    }
    if s.card_rect.is_none() {
        log(format!("card shown at ({x}, {y}) size {w}x{h}"));
    }
    s.card_hits = hits;
    s.card_rect = Some(RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    });
    s.outside_since = None;
    true
}

fn hide_card(s: &mut State) {
    if s.card_rect.take().is_some() {
        unsafe {
            let _ = ShowWindow(s.card_hwnd, SW_HIDE);
        }
        log("card hidden");
    }
    s.card_hits.clear();
    s.hover_since = None;
    s.outside_since = None;
}

/// Opens the card after a short hover on the pill and closes it once the
/// pointer has left both the pill and the card for a moment.
fn track_hover(s: &mut State) {
    if s.display.is_none() || s.mode == Mode::Tray || s.tray_added {
        hide_card(s);
        return;
    }
    let mut pt = POINT::default();
    if unsafe { GetCursorPos(&mut pt) }.is_err() {
        return;
    }
    let inside = |r: RECT| pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom;
    let pill_visible = unsafe { IsWindowVisible(s.hwnd) }.as_bool();
    let on_pill = pill_visible && rect_of(s.hwnd).is_some_and(inside);
    let on_card = s.card_rect.is_some_and(inside);
    let now = std::time::Instant::now();
    if on_pill || on_card {
        s.outside_since = None;
    }
    if s.card_rect.is_none() {
        if on_pill {
            let since = *s.hover_since.get_or_insert(now);
            if since.elapsed().as_millis() >= HOVER_OPEN_MS {
                show_card(s);
            }
        } else {
            s.hover_since = None;
        }
    } else if !on_pill && !on_card {
        let since = *s.outside_since.get_or_insert(now);
        if since.elapsed().as_millis() >= HOVER_CLOSE_MS {
            hide_card(s);
        }
    }
}

unsafe fn card_wndproc_inner(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Clicking the card must not steal focus from the user's window.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONUP => {
            let x = (lp.0 & 0xFFFF) as i16 as f32;
            let y = ((lp.0 >> 16) & 0xFFFF) as i16 as f32;
            let hit = with_state(|s| {
                let id = s.card_hits.iter().find(|h| h.contains(x, y)).map(|h| h.id);
                if id.is_some() {
                    hide_card(s);
                }
                id.map(|id| (id, s.events.clone()))
            })
            .flatten();
            if let Some((id, events)) = hit {
                let _ = events.send(Msg::Ui(UiEvent::Menu(id)));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
