//! Coco as a whole: the mascot on the desktop, the translator panel it
//! opens, the screens of the first run, the engine behind them, and what is
//! remembered between runs.

use std::path::PathBuf;

use coco_blob::Mood;
use coco_engine::{Quality, Translator};
use gpui::{
    AnyWindowHandle, App, Bounds, Context, Entity, Styled as _, Subscription, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, point, prelude::*, px, size,
};
use gpui_component::Root;
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;

use crate::login::{self, Permission};
use crate::mascot_view::{self, MascotEvent, MascotView};
use crate::native::{self, Floating, Rect};
use crate::onboarding::{self, Onboarding, OnboardingEvent, OnboardingOptions, Step};
use crate::panel::{self, Panel, PanelEvent, PanelOptions};
use crate::settings::{self, Settings};
use crate::theme::SHELL_MARGIN;

struct PanelWindow {
    handle: AnyWindowHandle,
    view: Entity<Panel>,
    native: Retained<NSWindow>,
    visible: bool,
    _events: Subscription,
}

struct OnboardingWindow {
    handle: AnyWindowHandle,
    view: Entity<Onboarding>,
    native: Retained<NSWindow>,
    _events: Subscription,
}

pub struct Coco {
    support: PathBuf,
    settings: Settings,
    translator: Translator,
    models: PathBuf,
    installed: Vec<Quality>,
    mascot: Entity<MascotView>,
    mascot_window: Retained<NSWindow>,
    panel: Option<PanelWindow>,
    onboarding: Option<OnboardingWindow>,
    _mascot_events: Subscription,
}

impl Coco {
    /// Opens the mascot and starts everything behind it.
    pub fn start(support: PathBuf, cx: &mut App) -> anyhow::Result<Entity<Self>> {
        let settings = Settings::load(&support);
        let models = settings::models_dir();
        let installed = Translator::installed(&models);
        let translator = Translator::start(models.clone());

        let (mascot_handle, mascot) = mascot_view::open(cx, settings.mascot, Mood::Idle, settings.mascot_size.side())?;
        let mascot_window = mascot_handle
            .update(cx, |_, window, _| native::ns_window(window))?
            .ok_or_else(|| anyhow::anyhow!("the mascot has no native window"))?;

        Ok(cx.new(|cx| {
            let mascot_events = cx.subscribe(&mascot, |this: &mut Self, _, event, cx| match event {
                MascotEvent::Clicked => this.toggle_panel(cx),
                MascotEvent::Moved => this.on_mascot_moved(cx),
            });

            let mut this =
                Self { support, settings, translator, models, installed, mascot, mascot_window, panel: None, onboarding: None, _mascot_events: mascot_events };
            // The first run asks before Coco becomes a login item. A copy that
            // was one before these screens existed stays one.
            if this.settings.onboarded || this.settings.registered_install.is_some() {
                this.reconcile_login(cx);
            }
            // A run without a model has one to get before anything else.
            if !this.settings.onboarded || this.installed.is_empty() {
                this.start_onboarding(cx);
            }
            this
        }))
    }

    pub fn mascot(&self) -> &Entity<MascotView> {
        &self.mascot
    }

    /// Window number and on-screen rectangle of each window, for captures.
    pub fn windows(&self) -> Vec<(&'static str, isize, Option<Rect>)> {
        let mut windows = vec![("mascot", self.mascot_window.windowNumber(), native::capture_rect(&self.mascot_window))];
        if let Some(panel) = &self.panel {
            windows.push(("panel", panel.native.windowNumber(), native::capture_rect(&panel.native)));
        }
        if let Some(onboarding) = &self.onboarding {
            windows.push(("onboarding", onboarding.native.windowNumber(), native::capture_rect(&onboarding.native)));
        }
        windows
    }

    /// The window that is in front of the user's work: the first screens
    /// while they are up, else the panel. For captures.
    pub fn front_window(&self) -> Option<AnyWindowHandle> {
        self.onboarding.as_ref().map(|onboarding| onboarding.handle).or(self.panel.as_ref().map(|panel| panel.handle))
    }

    pub fn panel(&self) -> Option<&Entity<Panel>> {
        self.panel.as_ref().map(|panel| &panel.view)
    }

    pub fn toggle_panel(&mut self, cx: &mut Context<Self>) {
        if self.panel.as_ref().is_some_and(|panel| panel.visible) {
            self.hide_panel(cx);
        } else {
            self.show_panel(true, cx);
        }
    }

    /// Shows the panel next to the mascot. `take_keyboard` is false only for
    /// captures made while somebody is typing elsewhere.
    pub fn show_panel(&mut self, take_keyboard: bool, cx: &mut Context<Self>) {
        // While the first screens are up, they are what a click on Coco is for.
        if self.onboarding.is_some() {
            return self.raise_onboarding(None, take_keyboard, cx);
        }
        // Nothing to translate with: the screen that downloads a model.
        if self.installed.is_empty() {
            return self.start_onboarding(cx);
        }
        if self.panel.is_none() {
            match self.open_panel(cx) {
                Ok(panel) => self.panel = Some(panel),
                Err(error) => {
                    eprintln!("coco: cannot open the panel: {error:#}");
                    self.mascot.update(cx, |mascot, cx| mascot.set_mood(Mood::Sorry, cx));
                    return;
                }
            }
        }
        let (place, side) = self.panel_place();
        let Some(panel) = self.panel.as_mut() else { return };
        panel.visible = true;
        let (handle, view, window) = (panel.handle, panel.view.clone(), panel.native.clone());

        // Load the model while the user starts typing.
        let (direction, quality) = {
            let panel = view.read(cx);
            (panel.direction(), panel.quality())
        };
        self.translator.warm_up(direction, quality);
        self.mascot.update(cx, |mascot, cx| {
            mascot.look_towards(Some([side * 0.85, -0.15]), cx);
            mascot.set_breathing(true, cx);
        });

        // AppKit reports moves and focus back to gpui at once, so these calls
        // must run outside of any gpui update: a task does that.
        cx.spawn(async move |_, cx| {
            native::set_origin(&window, place.x, place.y);
            if take_keyboard {
                handle
                    .update(cx, |_, window, cx| {
                        window.activate_window();
                        view.update(cx, |panel, cx| panel.focus_source(window, cx));
                    })
                    .ok();
            } else {
                native::show(&window);
            }
        })
        .detach();
    }

    pub fn hide_panel(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.panel.as_mut() else { return };
        panel.visible = false;
        let window = panel.native.clone();
        cx.spawn(async move |_, _| native::hide(&window)).detach();
        self.mascot.update(cx, |mascot, cx| {
            mascot.look_towards(None, cx);
            mascot.set_breathing(false, cx);
        });
    }

    /// Opens the screens of the first run once macOS has said whether the
    /// login item is still to be allowed. After the first run only the
    /// screen of the models is left to show.
    fn start_onboarding(&mut self, cx: &mut Context<Self>) {
        let first_run = !self.settings.onboarded;
        let asking = cx.background_spawn(async { login::permission() });
        cx.spawn(async move |this, cx| {
            let permission = asking.await;
            this.update(cx, |this, cx| this.show_onboarding(onboarding::steps(first_run, permission), permission, cx)).ok();
        })
        .detach();
    }

    fn show_onboarding(&mut self, steps: Vec<Step>, permission: Permission, cx: &mut Context<Self>) {
        let take_keyboard = takes_keyboard();
        if self.onboarding.is_some() {
            return self.raise_onboarding(None, take_keyboard, cx);
        }
        match self.open_onboarding(steps, permission, cx) {
            Ok(onboarding) => self.onboarding = Some(onboarding),
            Err(error) => {
                eprintln!("coco: cannot open the first screens: {error:#}");
                return;
            }
        }
        self.hide_panel(cx);
        // In the middle of the display the pointer is on, a little above
        // the centre, where macOS puts a window that wants an answer.
        let (width, height) = (
            f64::from(onboarding::SHELL_WIDTH + 2.0 * SHELL_MARGIN),
            f64::from(onboarding::SHELL_HEIGHT + 2.0 * SHELL_MARGIN),
        );
        let area = native::visible_area_at(native::pointer()).unwrap_or(Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 });
        let place = Rect { x: area.x + (area.w - width) / 2.0, y: area.y + (area.h - height) * 0.56, w: width, h: height };
        self.raise_onboarding(Some(place.kept_inside(&area)), take_keyboard, cx);
    }

    /// Brings the first screens in front, at `place` when they just opened.
    fn raise_onboarding(&self, place: Option<Rect>, take_keyboard: bool, cx: &mut Context<Self>) {
        let Some(onboarding) = &self.onboarding else { return };
        let (handle, view, window) = (onboarding.handle, onboarding.view.clone(), onboarding.native.clone());
        // Outside of any gpui update, as for the panel.
        cx.spawn(async move |_, cx| {
            if let Some(place) = place {
                native::set_origin(&window, place.x, place.y);
            }
            if take_keyboard {
                handle
                    .update(cx, |_, window, cx| {
                        window.activate_window();
                        view.update(cx, |onboarding, cx| onboarding.focus(window, cx));
                    })
                    .ok();
            } else {
                native::show(&window);
            }
        })
        .detach();
    }

    fn open_onboarding(&mut self, steps: Vec<Step>, permission: Permission, cx: &mut Context<Self>) -> anyhow::Result<OnboardingWindow> {
        let window_size = size(px(onboarding::SHELL_WIDTH + 2.0 * SHELL_MARGIN), px(onboarding::SHELL_HEIGHT + 2.0 * SHELL_MARGIN));
        let options = OnboardingOptions { steps, models_dir: self.models.clone(), installed: self.installed.clone(), permission };

        let mut view = None;
        let handle = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(120.), px(120.)), window_size))),
                titlebar: None,
                window_background: WindowBackgroundAppearance::Transparent,
                // Like the panel: over the user's windows, so that an app
                // without a Dock icon cannot lose it behind them.
                kind: WindowKind::PopUp,
                focus: true,
                show: false,
                is_resizable: false,
                is_minimizable: false,
                app_owns_titlebar_drag: true,
                // Coco moves in it while the app is not the active one.
                inactive_frame_interval: None,
                ..Default::default()
            },
            |window, cx| {
                native::float(window, Floating::Panel);
                let onboarding = cx.new(|cx| Onboarding::new(options, cx));
                view = Some(onboarding.clone());
                cx.new(|cx| Root::new(onboarding, window, cx).bg(gpui::transparent_black()))
            },
        )?;
        let view = view.ok_or_else(|| anyhow::anyhow!("the first screens were not built"))?;
        let native = handle
            .update(cx, |_, window, cx| {
                crate::theme::apply(window.appearance(), cx);
                native::ns_window(window)
            })?
            .ok_or_else(|| anyhow::anyhow!("the first screens have no native window"))?;
        let events = cx.subscribe(&view, |this, _, event, cx| this.on_onboarding_event(event, cx));

        Ok(OnboardingWindow { handle: handle.into(), view, native, _events: events })
    }

    fn close_onboarding(&mut self, cx: &mut Context<Self>) {
        let Some(onboarding) = self.onboarding.take() else { return };
        // The view goes with its window, and a download in progress with
        // the view: what it had fetched stays on disk for the next one.
        cx.spawn(async move |_, cx| {
            onboarding.handle.update(cx, |_, window, _| window.remove_window()).ok();
        })
        .detach();
        let mood = if self.installed.is_empty() { Mood::Sorry } else { Mood::Idle };
        self.mascot.update(cx, |mascot, cx| mascot.set_mood(mood, cx));
    }

    fn on_onboarding_event(&mut self, event: &OnboardingEvent, cx: &mut Context<Self>) {
        match event {
            OnboardingEvent::Mood(mood) => self.mascot.update(cx, |mascot, cx| mascot.set_mood(*mood, cx)),
            OnboardingEvent::ModelInstalled => {
                self.installed = Translator::installed(&self.models);
                let (installed, quality) = (self.installed.clone(), self.settings.quality(&self.installed));
                if let Some(panel) = &self.panel {
                    panel.view.update(cx, |panel, cx| panel.set_installed(installed, quality, cx));
                }
            }
            OnboardingEvent::LoginChanged(outcome) => self.on_login_outcome(outcome.clone(), cx),
            OnboardingEvent::Finished { open_at_login } => {
                if let Some(on) = open_at_login {
                    self.settings.open_at_login = *on;
                }
                let first_run = !self.settings.onboarded;
                self.settings.onboarded = true;
                self.save();
                self.close_onboarding(cx);
                if first_run {
                    self.reconcile_login(cx);
                }
                self.show_panel(takes_keyboard(), cx);
            }
            OnboardingEvent::QuitRequested => cx.quit(),
            OnboardingEvent::Dismissed => {
                self.close_onboarding(cx);
                // Opened from the panel's menu: back to the panel.
                if self.settings.onboarded && !self.installed.is_empty() {
                    self.show_panel(takes_keyboard(), cx);
                }
            }
        }
    }

    fn open_panel(&mut self, cx: &mut Context<Self>) -> anyhow::Result<PanelWindow> {
        let window_size = size(px(panel::SHELL_WIDTH + 2.0 * panel::MARGIN), px(panel::SHELL_HEIGHT + 2.0 * panel::MARGIN));
        let translator = self.translator.clone();
        let options = PanelOptions {
            direction: self.settings.direction(),
            english: self.settings.english(),
            quality: self.settings.quality(&self.installed),
            installed: self.installed.clone(),
            mascot_size: self.settings.mascot_size,
            open_at_login: self.settings.open_at_login,
        };

        let mut view = None;
        let handle = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(120.), px(120.)), window_size))),
                titlebar: None,
                window_background: WindowBackgroundAppearance::Transparent,
                // A non-activating panel: it takes the keyboard without
                // pulling the whole app in front of the user's work.
                kind: WindowKind::PopUp,
                focus: true,
                show: false,
                is_resizable: false,
                is_minimizable: false,
                app_owns_titlebar_drag: true,
                inactive_frame_interval: None,
                ..Default::default()
            },
            |window, cx| {
                native::float(window, Floating::Panel);
                let panel = cx.new(|cx| Panel::new(translator, options, window, cx));
                view = Some(panel.clone());
                cx.new(|cx| Root::new(panel, window, cx).bg(gpui::transparent_black()))
            },
        )?;
        let view = view.ok_or_else(|| anyhow::anyhow!("the panel was not built"))?;
        // The appearance may have changed since the app started.
        let native = handle
            .update(cx, |_, window, cx| {
                crate::theme::apply(window.appearance(), cx);
                native::ns_window(window)
            })?
            .ok_or_else(|| anyhow::anyhow!("the panel has no native window"))?;
        let events = cx.subscribe(&view, |this, _, event, cx| this.on_panel_event(event, cx));

        Ok(PanelWindow { handle: handle.into(), view, native, visible: false, _events: events })
    }

    /// Where the panel window goes, and on which side of the mascot it is
    /// (-1 left, +1 right).
    fn panel_place(&self) -> (Rect, f32) {
        let mascot = native::frame(&self.mascot_window);
        let area = native::visible_area_at(mascot.center()).unwrap_or(Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 });
        place_panel(
            mascot,
            area,
            (panel::SHELL_WIDTH + 2.0 * panel::MARGIN) as f64,
            (panel::SHELL_HEIGHT + 2.0 * panel::MARGIN) as f64,
            panel::MARGIN as f64,
        )
    }

    fn on_mascot_moved(&mut self, cx: &mut Context<Self>) {
        let frame = native::frame(&self.mascot_window);
        self.settings.mascot = Some((frame.x, frame.y));
        self.save();
        if self.panel.as_ref().is_some_and(|panel| panel.visible) {
            self.show_panel(false, cx);
        }
    }

    fn on_panel_event(&mut self, event: &PanelEvent, cx: &mut Context<Self>) {
        let mood = |this: &mut Self, mood, cx: &mut Context<Self>| this.mascot.update(cx, |mascot, cx| mascot.set_mood(mood, cx));
        match event {
            PanelEvent::Working => mood(self, Mood::Thinking, cx),
            PanelEvent::Translated => mood(self, Mood::Happy, cx),
            PanelEvent::Failed => mood(self, Mood::Sorry, cx),
            PanelEvent::Cleared => mood(self, Mood::Idle, cx),
            PanelEvent::Copied => self.mascot.update(cx, |mascot, cx| mascot.nudge(cx)),
            PanelEvent::HideRequested => self.hide_panel(cx),
            PanelEvent::QuitRequested => cx.quit(),
            PanelEvent::ModelsRequested => self.show_onboarding(vec![Step::Models], Permission::Unavailable, cx),
            PanelEvent::PreferencesChanged => {
                if let Some(panel) = self.panel.as_ref().map(|panel| panel.view.read(cx)) {
                    self.settings.french_to_english = panel.direction() == coco_engine::Direction::FrToEn;
                    self.settings.british = panel.english() == coco_engine::EnglishVariant::British;
                    // Keep the wish for a model that is not installed yet.
                    if panel.quality() != self.settings.quality(&self.installed) {
                        self.settings.accurate = panel.quality() == Quality::Accurate;
                    }
                    self.save();
                }
            }
            PanelEvent::MascotSizeChanged(size) => {
                self.settings.mascot_size = *size;
                self.save();
                let side = size.side();
                // The mascot reports its new place once resized: that saves
                // it and moves the panel beside it.
                self.mascot.update(cx, |mascot, cx| mascot.set_side(side, cx));
            }
            PanelEvent::OpenAtLoginChanged(on) => {
                let on = *on;
                self.settings.open_at_login = on;
                self.save();
                let changing = cx.background_spawn(async move { login::set(on) });
                cx.spawn(async move |this, cx| {
                    let outcome = changing.await;
                    this.update(cx, |this, cx| this.on_login_outcome(outcome, cx)).ok();
                })
                .detach();
            }
        }
    }

    /// Brings the login item in step with the setting, as every start does.
    fn reconcile_login(&self, cx: &mut Context<Self>) {
        let (wanted, registered) = (self.settings.open_at_login, self.settings.registered_install.clone());
        let checking = cx.background_spawn(async move { login::reconcile(wanted, registered) });
        cx.spawn(async move |this, cx| {
            let outcome = checking.await;
            this.update(cx, |this, cx| this.on_login_outcome(outcome, cx)).ok();
        })
        .detach();
    }

    fn on_login_outcome(&mut self, mut outcome: login::Outcome, cx: &mut Context<Self>) {
        if let Some(note) = outcome.note.take() {
            eprintln!("coco: open at login: {note}");
        }
        if outcome.apply(&mut self.settings) {
            self.save();
            let on = self.settings.open_at_login;
            if let Some(panel) = &self.panel {
                panel.view.update(cx, |panel, cx| panel.set_open_at_login(on, cx));
            }
        }
    }

    fn save(&self) {
        if let Err(error) = self.settings.save(&self.support) {
            eprintln!("coco: cannot save the settings: {error}");
        }
    }
}

/// False in a capture run (`COCO_PANEL=1`), which leaves the keyboard to
/// whoever is typing: a window that opens by itself must not take it.
fn takes_keyboard() -> bool {
    std::env::var_os("COCO_PANEL").is_none()
}

/// Keeps the app's one `Coco` alive for as long as the app runs.
pub struct Running(pub Entity<Coco>);

impl gpui::Global for Running {}

pub fn running(cx: &App) -> Option<Entity<Coco>> {
    cx.try_global::<Running>().map(|running| running.0.clone())
}

/// Puts the panel beside the mascot: on the side that has room, growing
/// upwards when the mascot is in the lower half of the screen. The window is
/// `margin` larger than the visible shell on every side, and that margin may
/// overlap the mascot. Screen coordinates, y upwards.
fn place_panel(mascot: Rect, area: Rect, width: f64, height: f64, margin: f64) -> (Rect, f32) {
    const GAP: f64 = 6.0;
    // Empty band between the mascot window's edge and the body inside it:
    // a seventh of the window, whatever its size.
    let inset = mascot.w / 7.0;

    let left = mascot.x + inset - GAP + margin - width;
    let right = mascot.x + mascot.w - inset + GAP - margin;
    let room_left = left + margin >= area.x;
    let room_right = right + width - margin <= area.x + area.w;
    let on_right_half = mascot.center().0 > area.x + area.w / 2.0;
    let go_left = if on_right_half { room_left || !room_right } else { !room_right && room_left };
    let x = if go_left { left } else { right };

    let in_lower_half = mascot.center().1 < area.y + area.h / 2.0;
    let y = if in_lower_half {
        mascot.y + inset - margin
    } else {
        mascot.y + mascot.h - inset + margin - height
    };

    // The shell must stay on screen; its shadow margin may hang over the edge.
    let shell_area = Rect { x: area.x - margin + 8.0, y: area.y - margin + 8.0, w: area.w + 2.0 * margin - 16.0, h: area.h + 2.0 * margin - 16.0 };
    let window = Rect { x, y, w: width, h: height }.kept_inside(&shell_area);
    (window, if go_left { -1.0 } else { 1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect { x: 0.0, y: 0.0, w: 1470.0, h: 920.0 };
    const W: f64 = 476.0;
    const H: f64 = 620.0;
    const M: f64 = 36.0;

    fn mascot(x: f64, y: f64) -> Rect {
        Rect { x, y, w: 112.0, h: 112.0 }
    }

    #[test]
    fn a_mascot_in_the_bottom_right_corner_gets_the_panel_on_its_left_growing_up() {
        let m = mascot(1330.0, 28.0);
        let (panel, side) = place_panel(m, AREA, W, H, M);
        assert_eq!(side, -1.0);
        let shell_right = panel.x + W - M;
        assert!(shell_right < m.x + 16.0, "the shell overlaps the mascot body");
        assert!(panel.y + M >= AREA.y + 8.0, "the shell goes under the screen");
        assert!((panel.y + M - (m.y + 16.0)).abs() < 9.0, "bottoms are not aligned");
    }

    #[test]
    fn a_mascot_in_the_top_left_corner_gets_the_panel_on_its_right_growing_down() {
        let m = mascot(20.0, 780.0);
        let (panel, side) = place_panel(m, AREA, W, H, M);
        assert_eq!(side, 1.0);
        assert!(panel.x + M > m.x + m.w - 16.0);
        assert!(panel.y + H - M <= AREA.y + AREA.h - 8.0 + 0.001, "the shell goes over the top");
    }

    #[test]
    fn the_panel_keeps_clear_of_the_body_at_every_mascot_size() {
        for side in [84.0, 112.0, 148.0] {
            let m = Rect { x: 1470.0 - 28.0 - side, y: 28.0, w: side, h: side };
            let (panel, direction) = place_panel(m, AREA, W, H, M);
            assert_eq!(direction, -1.0);
            let body_left = m.x + side / 7.0;
            let shell_right = panel.x + W - M;
            assert!(shell_right < body_left, "size {side}: the shell overlaps the body");
            assert!(body_left - shell_right < 10.0, "size {side}: the panel is too far from the body");
        }
    }

    #[test]
    fn the_shell_always_stays_on_screen() {
        for (x, y) in [(0.0, 0.0), (1358.0, 0.0), (0.0, 808.0), (1358.0, 808.0), (700.0, 400.0)] {
            let (panel, _) = place_panel(mascot(x, y), AREA, W, H, M);
            assert!(panel.x + M >= AREA.x && panel.x + W - M <= AREA.x + AREA.w, "x out at {x},{y}: {panel:?}");
            assert!(panel.y + M >= AREA.y && panel.y + H - M <= AREA.y + AREA.h, "y out at {x},{y}: {panel:?}");
        }
    }
}
