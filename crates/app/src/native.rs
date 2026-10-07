//! Raw AppKit calls for what gpui does not expose: window level, Spaces,
//! shadow, glass, moving and hiding a window.
//!
//! gpui hands out its NSView through `raw-window-handle`; the NSWindow is
//! that view's window. Everything here runs on the main thread, which is
//! where gpui runs every window callback.

use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSGlassEffectView, NSScreen, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
    NSWindowCollectionBehavior, NSWindowOrderingMode,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Above normal windows, below the menu bar and menus. gpui's pop-up kind
/// uses level 101, which would put Traduko over every menu.
const LEVEL_FLOATING: isize = 3;
/// One step higher, for the mascot: the panel's transparent shadow margin
/// overlaps it, and a click there must still reach the mascot.
const LEVEL_MASCOT: isize = 4;

pub fn ns_window(window: &gpui::Window) -> Option<Retained<NSWindow>> {
    // `gpui::Window` has its own `window_handle`, so name the trait.
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: gpui gives its live NSView and we are on the main thread.
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

#[derive(Clone, Copy, PartialEq)]
pub enum Floating {
    /// Never takes the keyboard: a click on it leaves the user's typing
    /// where it was.
    Mascot,
    /// Takes the keyboard when shown.
    Panel,
}

/// Floats the window over normal windows, on every Space and over
/// full-screen apps, and keeps it visible while another app is active.
pub fn float(window: &gpui::Window, kind: Floating) {
    let Some(win) = ns_window(window) else { return };
    // Both windows are transparent and draw their own shape: the system
    // shadow would outline their invisible rectangle.
    win.setHasShadow(false);
    win.setLevel(if kind == Floating::Mascot { LEVEL_MASCOT } else { LEVEL_FLOATING });
    win.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    win.setHidesOnDeactivate(false);
    let is_panel: bool = unsafe { msg_send![&*win, isKindOfClass: objc2::class!(NSPanel)] };
    if is_panel {
        let _: () = unsafe { msg_send![&*win, setBecomesKeyOnlyIfNeeded: kind == Floating::Mascot] };
    }
}

/// Puts the glass of macOS under what gpui draws: what is behind the window
/// shows through it, blurred. The glass is `margin` inside the window and its
/// corners have `radius`, like the shell that is drawn on it.
pub fn glass(window: &gpui::Window, margin: f64, radius: f64) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(content) = ns_window(window).and_then(|win| win.contentView()) else { return };
    let bounds = content.bounds();
    let frame = NSRect::new(NSPoint::new(margin, margin), NSSize::new(bounds.size.width - 2.0 * margin, bounds.size.height - 2.0 * margin));
    // Liquid Glass came with macOS 26. Before it, the blur of a menu.
    let view: Retained<NSView> = if AnyClass::get(c"NSGlassEffectView").is_some() {
        let glass = NSGlassEffectView::initWithFrame(mtm.alloc(), frame);
        glass.setCornerRadius(radius);
        Retained::into_super(glass)
    } else {
        let blur = NSVisualEffectView::initWithFrame(mtm.alloc(), frame);
        blur.setMaterial(NSVisualEffectMaterial::Popover);
        blur.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        // Traduko is never the active app for long: the blur must not fade.
        blur.setState(NSVisualEffectState::Active);
        blur.setWantsLayer(true);
        if let Some(layer) = blur.layer() {
            let _: () = unsafe { msg_send![&*layer, setCornerRadius: radius] };
            let _: () = unsafe { msg_send![&*layer, setMasksToBounds: true] };
        }
        Retained::into_super(blur)
    };
    content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None);
}

/// Forces light or dark on the whole app, glass included.
pub fn set_appearance(dark: bool) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let name = unsafe { if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } };
    NSApplication::sharedApplication(mtm).setAppearance(NSAppearance::appearanceNamed(name).as_deref());
}

/// The window's rectangle the way `screencapture -R` wants it: origin at the
/// top-left corner of the main display, y downwards.
pub fn capture_rect(win: &NSWindow) -> Option<Rect> {
    let mtm = MainThreadMarker::new()?;
    let main_height = NSScreen::screens(mtm).iter().next()?.frame().size.height;
    let f = frame(win);
    Some(Rect { y: main_height - f.y - f.h, ..f })
}

pub fn show(win: &NSWindow) {
    win.orderFrontRegardless();
}

pub fn hide(win: &NSWindow) {
    win.orderOut(None);
}

/// A rectangle in AppKit screen coordinates: origin at the bottom-left
/// corner of the main display, y grows upwards, units are points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn of(r: NSRect) -> Self {
        Self { x: r.origin.x, y: r.origin.y, w: r.size.width, h: r.size.height }
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// The same size, moved as little as possible to fit inside `area`.
    pub fn kept_inside(&self, area: &Rect) -> Rect {
        let x = self.x.min(area.x + area.w - self.w).max(area.x);
        let y = self.y.min(area.y + area.h - self.h).max(area.y);
        Rect { x, y, ..*self }
    }
}

pub fn frame(win: &NSWindow) -> Rect {
    Rect::of(win.frame())
}

pub fn set_origin(win: &NSWindow, x: f64, y: f64) {
    win.setFrameOrigin(NSPoint::new(x, y));
}

pub fn set_frame(win: &NSWindow, r: Rect) {
    win.setFrame_display(NSRect::new(NSPoint::new(r.x, r.y), objc2_foundation::NSSize::new(r.w, r.h)), true);
}

/// The pointer, in AppKit screen coordinates. gpui only gives positions
/// relative to a window, which is useless while that window is moving.
pub fn pointer() -> (f64, f64) {
    let p: NSPoint = unsafe { msg_send![objc2::class!(NSEvent), mouseLocation] };
    (p.x, p.y)
}

/// The usable area (no menu bar, no Dock) of the display that holds `point`,
/// or of the nearest display when none does: a mascot parked half off an
/// edge still belongs to the screen it hangs from.
pub fn visible_area_at(point: (f64, f64)) -> Option<Rect> {
    let mtm = MainThreadMarker::new()?;
    let away = |screen: &Retained<NSScreen>| {
        let f = Rect::of(screen.frame());
        let dx = (f.x - point.0).max(point.0 - (f.x + f.w)).max(0.0);
        let dy = (f.y - point.1).max(point.1 - (f.y + f.h)).max(0.0);
        dx.hypot(dy)
    };
    let screen = NSScreen::screens(mtm).iter().min_by(|a, b| away(a).total_cmp(&away(b)))?;
    Some(Rect::of(screen.visibleFrame()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_is_pushed_back_inside_its_area() {
        let area = Rect { x: 0.0, y: 0.0, w: 1000.0, h: 800.0 };
        let off_right = Rect { x: 980.0, y: -30.0, w: 100.0, h: 100.0 };
        assert_eq!(off_right.kept_inside(&area), Rect { x: 900.0, y: 0.0, w: 100.0, h: 100.0 });
        let inside = Rect { x: 10.0, y: 20.0, w: 100.0, h: 100.0 };
        assert_eq!(inside.kept_inside(&area), inside);
    }
}
