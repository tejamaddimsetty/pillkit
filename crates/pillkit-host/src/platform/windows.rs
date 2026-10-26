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

use pillkit_core::render::{render, Fonts, Metrics, Rendered, SLOT_WIDTH_PT};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    BOOL, COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
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
use crate::controller::Msg;
use crate::paths::log;

const TIMER_POLL: usize = 1;
const WM_TRAY: u32 = WM_APP + 1;
const MARGIN_PT: f32 = 8.0;

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
    tray_added: bool,
    ticks: u32,
    win10_shrunk: Option<(HWND, i32)>,
    last_placed: Option<(Mode, RECT)>,
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
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

pub fn run(cmds: Receiver<UiCmd>, events: Sender<Msg>) {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
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
            tray_added: false,
            ticks: 0,
            win10_shrunk: None,
            last_placed: None,
        };
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            std::sync::atomic::Ordering::Relaxed,
        );
        STATE.with(|s| *s.borrow_mut() = Some(state));
        SetTimer(hwnd, TIMER_POLL, 250, None);

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

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
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
        if changed || s.ticks % 8 == 0 || s.mode == Mode::Overlay {
            layout(s);
        }
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
    if right || d.click_opens_menu {
        if let Some(id) = popup_menu(hwnd, &d.menu) {
            let _ = events.send(Msg::Ui(UiEvent::Menu(id)));
        }
    } else {
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

fn reg_dword(root: HKEY, key: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut data = 0u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            root,
            key,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    r.is_ok().then_some(data)
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

const ADVANCED: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced");

fn fullscreen_app_running() -> bool {
    unsafe {
        SHQueryUserNotificationState()
            .map(|s| {
                s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE
            })
            .unwrap_or(false)
    }
}

/// Where the pill goes, in screen coordinates, for a pill of `w`×`h` pixels.
fn target_rect(taskbar: HWND, w: i32, h: i32, scale: f32) -> Option<RECT> {
    let tb = rect_of(taskbar)?;
    let vertical = (tb.bottom - tb.top) > (tb.right - tb.left);
    if vertical {
        return None; // Side-docked taskbars (Windows 10) have no room for a pill.
    }
    let margin = (MARGIN_PT * scale) as i32;
    let y = tb.top + ((tb.bottom - tb.top) - h) / 2;
    let tray_left = child(taskbar, w!("TrayNotifyWnd"))
        .and_then(rect_of)
        .map(|r| r.left)
        .unwrap_or(tb.right - (320.0 * scale) as i32);

    let x = if windows_build() >= 22000 {
        let centred = reg_dword(HKEY_CURRENT_USER, ADVANCED, w!("TaskbarAl")).unwrap_or(1) == 1;
        let widgets_on = reg_dword(HKEY_CURRENT_USER, ADVANCED, w!("TaskbarDa")).unwrap_or(1) == 1;
        if centred {
            // The weather widget's slot; sit beside it if it's switched on.
            tb.left
                + margin
                + if widgets_on {
                    (180.0 * scale) as i32
                } else {
                    0
                }
        } else {
            tray_left - w - margin
        }
    } else {
        tray_left - w - margin
    };
    Some(RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    })
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
        return show_tray(s, &display);
    }

    let scale = unsafe { GetDpiForWindow(taskbar) } as f32 / 96.0;
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let tb = match rect_of(taskbar) {
        Some(r) => r,
        None => return,
    };
    let bar_h = (tb.bottom - tb.top) as f32;
    let pill_h = (bar_h * 0.72).min(40.0 * scale).max(20.0 * scale).round() as u32;

    if s.rendered.as_ref().map(|r| r.height) != Some(pill_h) {
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
                fixed_width_pt: Some(
                    pillkit_core::render::slot_width_override().unwrap_or(SLOT_WIDTH_PT),
                ),
            },
        )));
    }
    let r = s.rendered.clone().expect("just rendered");
    let r = r.as_ref();
    let (w, h) = (r.width as i32, r.height as i32);

    let Some(target) = target_rect(taskbar, w, h, scale) else {
        s.mode = Mode::Tray;
        return show_tray(s, &display);
    };
    remove_tray(s);

    if s.last_placed
        .map(|(m, r)| (m, r.left, r.top, r.right, r.bottom))
        != Some((s.mode, target.left, target.top, target.right, target.bottom))
    {
        log(format!(
            "placing pill: mode {:?}, at ({}, {}) size {}x{}; taskbar ({}, {})-({}, {}); scale {scale}; build {}",
            s.mode, target.left, target.top, w, h, tb.left, tb.top, tb.right, tb.bottom, windows_build()
        ));
        s.last_placed = Some((s.mode, target));
    }
    let fullscreen = fullscreen_app_running();
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
                if !blit(s.hwnd, r, pos, true) {
                    log("layered child window not supported; switching to overlay mode");
                    unembed(s.hwnd);
                    s.mode = Mode::Overlay;
                    return layout(s);
                }
                shrink_win10(s, taskbar, target.left);
                let _ = ShowWindow(s.hwnd, SW_SHOWNOACTIVATE);
            }
            Mode::Overlay => {
                if fullscreen {
                    let _ = ShowWindow(s.hwnd, SW_HIDE);
                    return;
                }
                if !blit(
                    s.hwnd,
                    r,
                    POINT {
                        x: target.left,
                        y: target.top,
                    },
                    false,
                ) {
                    log("overlay window could not be drawn; switching to the tray");
                    let _ = ShowWindow(s.hwnd, SW_HIDE);
                    s.mode = Mode::Tray;
                    return show_tray(s, &display);
                }
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

fn tray_data(s: &State, tip: &str) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: s.hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_TIP | NIF_ICON,
        uCallbackMessage: WM_TRAY,
        hIcon: unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() },
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
