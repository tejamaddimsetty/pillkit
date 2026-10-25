//! macOS: a variable-width NSStatusItem in the menu bar showing the rendered
//! pill, plus a native card that opens on hover or right-click.
//!
//! Left-click runs the item's action. Hovering for half a second (or
//! right-clicking) opens the card; it closes once the pointer has left both
//! the pill and the card.

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use barkit_core::render::{render, slot_width_override, Fonts, Metrics, SLOT_WIDTHS_PT};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSBezelStyle, NSBox,
    NSBoxType, NSButton, NSColor, NSEvent, NSEventMask, NSEventType, NSFont, NSImage,
    NSImageScaling, NSLineBreakMode, NSPanel, NSPopUpMenuWindowLevel, NSScreen, NSStatusBar,
    NSStatusItem, NSTextField, NSTitlePosition, NSVariableStatusItemLength, NSView,
    NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_foundation::{
    CGFloat, MainThreadMarker, NSData, NSPoint, NSRect, NSSize, NSString, NSTimer,
};

use super::{CardView, Display, UiCmd, UiEvent};
use crate::controller::{Msg, MENU_HIDE, MENU_WHY};
use crate::paths::log;

const CARD_WIDTH: CGFloat = 300.0;
const PAD: CGFloat = 14.0;
const HOVER_OPEN: Duration = Duration::from_millis(450);
const HOVER_CLOSE: Duration = Duration::from_millis(400);

pub struct Ivars {
    cmds: Receiver<UiCmd>,
    events: Sender<Msg>,
    fonts: Option<Fonts>,
    status: RefCell<Option<Retained<NSStatusItem>>>,
    display: RefCell<Option<Display>>,
    card: RefCell<Option<Retained<NSPanel>>>,
    hover_since: Cell<Option<Instant>>,
    outside_since: Cell<Option<Instant>>,
    /// Index into SLOT_WIDTHS_PT; steps down if macOS has no room for us.
    width_step: Cell<usize>,
    shown_at: Cell<Option<Instant>>,
}

declare_class!(
    pub struct Delegate;

    unsafe impl ClassType for Delegate {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "BarkitHostDelegate";
    }

    impl DeclaredClass for Delegate {
        type Ivars = Ivars;
    }

    unsafe impl Delegate {
        #[method(tick:)]
        fn tick(&self, _timer: &NSTimer) {
            self.on_tick();
            self.check_fits();
            self.track_hover();
        }

        #[method(clicked:)]
        fn clicked(&self, _sender: &AnyObject) {
            self.on_click();
        }

        #[method(cardButton:)]
        fn card_button(&self, sender: &NSButton) {
            let tag = unsafe { sender.tag() };
            self.close_card();
            let _ = self.ivars().events.send(Msg::Ui(UiEvent::Menu(tag as u32)));
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, ivars: Ivars) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        unsafe { msg_send_id![super(this), init] }
    }

    fn mtm(&self) -> MainThreadMarker {
        MainThreadMarker::from(self)
    }

    fn on_tick(&self) {
        loop {
            match self.ivars().cmds.try_recv() {
                Ok(UiCmd::Show(d)) => self.show(d),
                Ok(UiCmd::Hide) => self.hide(),
                Ok(UiCmd::Quit) | Err(TryRecvError::Disconnected) => {
                    let app = NSApplication::sharedApplication(self.mtm());
                    unsafe { app.terminate(None) };
                    return;
                }
                Err(TryRecvError::Empty) => return,
            }
        }
    }

    fn status_item(&self) -> Retained<NSStatusItem> {
        let mut slot = self.ivars().status.borrow_mut();
        if let Some(item) = slot.as_ref() {
            return item.clone();
        }
        let mtm = self.mtm();
        let item = unsafe {
            NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength)
        };
        if let Some(button) = unsafe { item.button(mtm) } {
            unsafe {
                button.setTarget(Some(self));
                button.setAction(Some(sel!(clicked:)));
                button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
                button.setImageScaling(NSImageScaling::NSImageScaleNone);
            }
        }
        *slot = Some(item.clone());
        item
    }

    /// The menu bar's real height. `NSStatusBar.thickness` stays 22 pt even
    /// on Macs with a notch, where the bar is about 37 pt tall.
    fn menu_bar_height(&self) -> CGFloat {
        let thickness = unsafe { NSStatusBar::systemStatusBar().thickness() };
        let Some(screen) = NSScreen::mainScreen(self.mtm()) else {
            return thickness;
        };
        let notch = unsafe { screen.safeAreaInsets() }.top;
        let frame = screen.frame();
        let visible = screen.visibleFrame();
        let reserved =
            (frame.origin.y + frame.size.height) - (visible.origin.y + visible.size.height);
        let h = notch
            .max(if reserved < 60.0 { reserved } else { 0.0 })
            .max(thickness);
        h.min(48.0)
    }

    fn show(&self, d: Display) {
        let Some(fonts) = self.ivars().fonts.as_ref() else {
            log("no font available; cannot draw");
            return;
        };
        let mtm = self.mtm();
        let scale = NSScreen::mainScreen(mtm)
            .map(|s| s.backingScaleFactor())
            .unwrap_or(2.0) as f32;
        let bar = self.menu_bar_height() as f32;
        // As tall as fits with 2 pt of air above and below: 29 pt in a 33 pt
        // bar, 33 pt in a 37 pt notch bar, 22 pt in a classic 24 pt bar.
        let height_pt = (bar - 4.0).clamp(22.0, 34.0);
        let r = render(
            &d.item,
            fonts,
            Metrics {
                height: (height_pt * scale).round() as u32,
                scale,
                max_width_pt: 480.0,
                fixed_width_pt: Some(self.slot_width()),
            },
        );

        let data = NSData::with_bytes(&r.to_png());
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
            return;
        };
        unsafe {
            image.setSize(NSSize::new(
                r.width as f64 / scale as f64,
                r.height as f64 / scale as f64,
            ))
        };

        let item = self.status_item();
        if let Some(button) = unsafe { item.button(mtm) } {
            unsafe {
                button.setImage(Some(&image));
                button.setTitle(&NSString::from_str(""));
            }
            let bf = button.frame();
            let wf = button.window().map(|w| w.frame());
            log(format!(
                "geometry: bar {bar} pt, pill {}x{} pt, button {}x{} pt, status window {:?}",
                r.width as f32 / scale,
                r.height as f32 / scale,
                bf.size.width,
                bf.size.height,
                wf.map(|f| (f.size.width, f.size.height))
            ));
        }
        unsafe { item.setVisible(true) };
        self.ivars().shown_at.set(Some(Instant::now()));
        let card_open = self.ivars().card.borrow().is_some();
        *self.ivars().display.borrow_mut() = Some(d);
        if card_open {
            // Keep an open card in step with the item now in the bar.
            self.open_card();
        }
    }

    fn hide(&self) {
        self.close_card();
        if let Some(item) = self.ivars().status.borrow().as_ref() {
            unsafe { item.setVisible(false) };
        }
        *self.ivars().display.borrow_mut() = None;
    }

    fn on_click(&self) {
        let mtm = self.mtm();
        let Some(d) = self.ivars().display.borrow().clone() else {
            return;
        };
        let right = NSApplication::sharedApplication(mtm)
            .currentEvent()
            .map(|e| unsafe { e.r#type() } == NSEventType::RightMouseUp)
            .unwrap_or(false);
        if right || d.click_opens_menu {
            self.open_card();
        } else {
            self.close_card();
            let _ = self.ivars().events.send(Msg::Ui(UiEvent::Clicked));
        }
    }

    fn slot_width(&self) -> f32 {
        slot_width_override()
            .unwrap_or(SLOT_WIDTHS_PT[self.ivars().width_step.get().min(SLOT_WIDTHS_PT.len() - 1)])
    }

    /// macOS quietly hides status items that don't fit beside the notch.
    /// If ours was hidden, step down to the next narrower width and redraw.
    fn check_fits(&self) {
        let Some(shown) = self.ivars().shown_at.get() else {
            return;
        };
        if shown.elapsed() < Duration::from_secs(1) || slot_width_override().is_some() {
            return;
        }
        self.ivars().shown_at.set(None);
        let status = self.ivars().status.borrow().clone();
        let Some(window) = status
            .and_then(|s| unsafe { s.button(self.mtm()) })
            .and_then(|b| b.window())
        else {
            return;
        };
        let visible = window
            .occlusionState()
            .contains(NSWindowOcclusionState::Visible);
        let step = self.ivars().width_step.get();
        if !visible && step + 1 < SLOT_WIDTHS_PT.len() {
            self.ivars().width_step.set(step + 1);
            log(format!(
                "menu bar has no room for {} pt; trying {} pt",
                SLOT_WIDTHS_PT[step],
                SLOT_WIDTHS_PT[step + 1]
            ));
            if let Some(d) = self.ivars().display.borrow().clone() {
                self.show(d);
            }
        }
    }

    /// Screen rect of the pill (the status item's window).
    fn pill_frame(&self) -> Option<NSRect> {
        let status = self.ivars().status.borrow().clone()?;
        let button = unsafe { status.button(self.mtm()) }?;
        let window = button.window()?;
        Some(window.frame())
    }

    fn track_hover(&self) {
        if self.ivars().display.borrow().is_none() {
            return;
        }
        let mouse = unsafe { NSEvent::mouseLocation() };
        let inside = |r: NSRect| {
            mouse.x >= r.origin.x
                && mouse.x <= r.origin.x + r.size.width
                && mouse.y >= r.origin.y - 2.0
                && mouse.y <= r.origin.y + r.size.height
        };
        let on_pill = self.pill_frame().is_some_and(inside);
        let card = self.ivars().card.borrow().clone();
        let on_card = card.as_ref().is_some_and(|c| inside(c.frame()));
        let now = Instant::now();

        if on_pill || on_card {
            self.ivars().outside_since.set(None);
        }
        if card.is_none() {
            if on_pill {
                let since = self.ivars().hover_since.get().unwrap_or(now);
                self.ivars().hover_since.set(Some(since));
                if now - since >= HOVER_OPEN {
                    self.open_card();
                }
            } else {
                self.ivars().hover_since.set(None);
            }
        } else if !on_pill && !on_card {
            let since = self.ivars().outside_since.get().unwrap_or(now);
            self.ivars().outside_since.set(Some(since));
            if now - since >= HOVER_CLOSE {
                self.close_card();
            }
        }
    }

    fn close_card(&self) {
        if let Some(panel) = self.ivars().card.borrow_mut().take() {
            panel.orderOut(None);
        }
        self.ivars().hover_since.set(None);
        self.ivars().outside_since.set(None);
    }

    fn open_card(&self) {
        let Some(d) = self.ivars().display.borrow().clone() else {
            return;
        };
        let Some(pill) = self.pill_frame() else {
            return;
        };
        if let Some(old) = self.ivars().card.borrow_mut().take() {
            old.orderOut(None);
        }
        let panel = self.build_card(&d.card);
        let size = panel.frame().size;

        // Under the pill, left-aligned with it, kept on screen.
        let mut origin = NSPoint::new(pill.origin.x, pill.origin.y - size.height - 6.0);
        if let Some(screen) = NSScreen::mainScreen(self.mtm()) {
            let v = screen.visibleFrame();
            origin.x = origin
                .x
                .min(v.origin.x + v.size.width - size.width - 8.0)
                .max(v.origin.x + 8.0);
        }
        unsafe {
            panel.setFrameOrigin(origin);
            panel.orderFrontRegardless();
        }
        *self.ivars().card.borrow_mut() = Some(panel);
    }

    fn build_card(&self, card: &CardView) -> Retained<NSPanel> {
        let mtm = self.mtm();
        let inner = CARD_WIDTH - PAD * 2.0;

        // Build top-down, then flip into AppKit's bottom-up coordinates.
        let mut views: Vec<(Retained<NSView>, CGFloat, CGFloat, CGFloat, CGFloat)> = Vec::new(); // view, x, top, w, h
        let mut y = PAD;

        let header = label(
            mtm,
            if card.label.is_empty() {
                " "
            } else {
                &card.label
            },
            12.0,
            false,
            unsafe { NSColor::secondaryLabelColor() },
        );
        views.push((
            Retained::into_super(Retained::into_super(header)),
            PAD,
            y,
            120.0,
            16.0,
        ));
        let why = link_button(mtm, "Why this?", self, MENU_WHY);
        let hide = link_button(mtm, "Hide", self, MENU_HIDE);
        let dot = label(mtm, "·", 12.0, false, unsafe {
            NSColor::secondaryLabelColor()
        });
        let (why_w, hide_w) = (
            unsafe { why.fittingSize() }.width,
            unsafe { hide.fittingSize() }.width,
        );
        let right = CARD_WIDTH - PAD;
        views.push((
            Retained::into_super(Retained::into_super(why)),
            right - why_w,
            y - 1.0,
            why_w,
            18.0,
        ));
        views.push((
            Retained::into_super(Retained::into_super(dot)),
            right - why_w - 10.0,
            y,
            8.0,
            16.0,
        ));
        views.push((
            Retained::into_super(Retained::into_super(hide)),
            right - why_w - 12.0 - hide_w,
            y - 1.0,
            hide_w,
            18.0,
        ));
        y += 16.0 + 10.0;

        let title = label(mtm, &card.title, 15.0, true, unsafe {
            NSColor::labelColor()
        });
        views.push((
            Retained::into_super(Retained::into_super(title)),
            PAD,
            y,
            inner,
            20.0,
        ));
        y += 20.0 + 4.0;

        if !card.body.is_empty() {
            let body = unsafe {
                NSTextField::wrappingLabelWithString(&NSString::from_str(&card.body), mtm)
            };
            unsafe {
                body.setFont(Some(&*NSFont::systemFontOfSize(13.0)));
                body.setTextColor(Some(&*NSColor::labelColor()));
                body.setPreferredMaxLayoutWidth(inner);
                body.setMaximumNumberOfLines(4);
            }
            let h = unsafe { body.fittingSize() }.height.max(17.0);
            views.push((
                Retained::into_super(Retained::into_super(body)),
                PAD,
                y,
                inner,
                h,
            ));
            y += h + 12.0;
        }

        if !card.buttons.is_empty() {
            let mut x = PAD;
            for (i, (id, text)) in card.buttons.iter().take(3).enumerate() {
                let b = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &NSString::from_str(text),
                        Some(self),
                        Some(sel!(cardButton:)),
                        mtm,
                    )
                };
                unsafe {
                    b.setBezelStyle(NSBezelStyle::Push);
                    b.setTag(*id as isize);
                    if i == 0 {
                        // Primary: accent-tinted like a default button.
                        b.setKeyEquivalent(&NSString::from_str("\r"));
                    }
                }
                let w = unsafe { b.fittingSize() }.width.max(80.0);
                views.push((Retained::into_super(Retained::into_super(b)), x, y, w, 28.0));
                x += w + 8.0;
            }
            y += 28.0;
        }
        let height = y + PAD;

        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(CARD_WIDTH, height));
        let panel = unsafe {
            NSPanel::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                rect,
                NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                NSBackingStoreType::NSBackingStoreBuffered,
                false,
            )
        };
        unsafe {
            panel.setLevel(NSPopUpMenuWindowLevel);
            panel.setOpaque(false);
            panel.setBackgroundColor(Some(&*NSColor::clearColor()));
            panel.setHasShadow(true);
            panel.setHidesOnDeactivate(false);
            panel.setFloatingPanel(true);
            panel.setReleasedWhenClosed(false);
        }

        let bg = unsafe { NSBox::initWithFrame(mtm.alloc(), rect) };
        unsafe {
            bg.setBoxType(NSBoxType::NSBoxCustom);
            bg.setTitlePosition(NSTitlePosition::NSNoTitle);
            bg.setCornerRadius(12.0);
            bg.setBorderWidth(0.5);
            bg.setBorderColor(&NSColor::separatorColor());
            bg.setFillColor(&NSColor::windowBackgroundColor());
            bg.setContentViewMargins(NSSize::new(0.0, 0.0));
        }
        let content =
            unsafe { bg.contentView() }.unwrap_or_else(|| Retained::into_super(bg.clone()));
        for (view, x, top, w, h) in views {
            unsafe {
                view.setFrame(NSRect::new(
                    NSPoint::new(x, height - top - h),
                    NSSize::new(w, h),
                ));
                content.addSubview(&view);
            }
        }
        panel.setContentView(Some(&bg));
        panel
    }
}

fn label(
    mtm: MainThreadMarker,
    text: &str,
    size: CGFloat,
    bold: bool,
    color: Retained<NSColor>,
) -> Retained<NSTextField> {
    let l = unsafe { NSTextField::labelWithString(&NSString::from_str(text), mtm) };
    unsafe {
        let font = if bold {
            NSFont::boldSystemFontOfSize(size)
        } else {
            NSFont::systemFontOfSize(size)
        };
        l.setFont(Some(&font));
        l.setTextColor(Some(&*color));
        l.setLineBreakMode(NSLineBreakMode::NSLineBreakByTruncatingTail);
    }
    l
}

fn link_button(
    mtm: MainThreadMarker,
    text: &str,
    target: &Delegate,
    id: u32,
) -> Retained<NSButton> {
    let b = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(text),
            Some(target),
            Some(sel!(cardButton:)),
            mtm,
        )
    };
    unsafe {
        b.setBordered(false);
        b.setTag(id as isize);
        b.setFont(Some(&*NSFont::systemFontOfSize(12.0)));
        b.setContentTintColor(Some(&*NSColor::secondaryLabelColor()));
    }
    b
}

pub fn run(cmds: Receiver<UiCmd>, events: Sender<Msg>) {
    let mtm = MainThreadMarker::new().expect("the platform loop must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // Accessory: no Dock icon, no app menu, just the status item.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let delegate = Delegate::new(
        mtm,
        Ivars {
            cmds,
            events,
            fonts: Fonts::system(),
            status: RefCell::new(None),
            display: RefCell::new(None),
            card: RefCell::new(None),
            hover_since: Cell::new(None),
            outside_since: Cell::new(None),
            width_step: Cell::new(0),
            shown_at: Cell::new(None),
        },
    );
    let target: &AnyObject = &delegate;
    let _timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            0.1,
            target,
            sel!(tick:),
            None,
            true,
        )
    };
    unsafe { app.run() };
}
