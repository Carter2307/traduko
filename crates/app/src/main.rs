mod app;
mod detect;
mod login;
mod mascot_view;
mod native;
mod onboarding;
mod panel;
mod settings;
mod theme;

use std::borrow::Cow;
use std::time::Duration;

use gpui::{App, AssetSource, KeyBinding, SharedString, WindowingRequest};
use traduko_blob::Mood;
use traduko_login::single_instance::{self, LockError};

use crate::app::{Running, Traduko};
use crate::onboarding::{CloseOnboarding, NextStep};
use crate::panel::{CopyResult, HidePanel, KEY_CONTEXT, SwapDirection, Translate};

// A dropped model must leave the process: see `traduko_engine::ReturnsLargeBlocks`.
#[global_allocator]
static ALLOCATOR: traduko_engine::ReturnsLargeBlocks = traduko_engine::ReturnsLargeBlocks;

// The icons the panel and the first screens draw themselves; the component
// library brings its own.
gpui_kit_assets::icon_assets!(
    PanelIcons,
    [ArrowUpDown, Copy, Check, X, CircleCheck, Languages, ShieldCheck, SpellCheck, MousePointerClick, Sparkles, Feather, Power, EyeOff]
);

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        match PanelIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => gpui_kit_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut entries = PanelIcons.list(path)?;
        entries.extend(gpui_kit_assets::Assets.list(path)?);
        Ok(entries)
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(at) = arguments.iter().position(|argument| argument == "--login-item") {
        std::process::exit(login::command_line(arguments.get(at + 1).map_or("status", String::as_str)));
    }

    // One mascot only: a start at login and a start by hand must not add up.
    let support = settings::support_dir();
    if let Err(error) = std::fs::create_dir_all(&support) {
        eprintln!("traduko: cannot create {}: {error}", support.display());
    }
    let _lock = match single_instance::acquire(&support) {
        Ok(lock) => lock,
        Err(LockError::AlreadyRunning) => return,
        Err(error) => {
            eprintln!("traduko: {error}");
            return;
        }
    };

    // Headless windowing on macOS makes the app an accessory before the run
    // loop starts: no Dock icon, no menu bar, windows still open normally.
    let application = gpui_platform::application().with_assets(Assets).with_windowing(WindowingRequest::Headless);

    // Opening the app again while it runs (Finder, Spotlight) shows the panel.
    application.on_reopen(|cx| {
        if let Some(traduko) = app::running(cx) {
            traduko.update(cx, |traduko, cx| traduko.show_panel(true, cx));
        }
    });

    application.run(move |cx: &mut App| {
        gpui_component::init(cx);
        theme::register_fonts(cx);
        theme::apply(cx.window_appearance(), cx);

        // Bound after the library's own keys, so at equal depth ours win.
        // The text area binds cmd-enter itself, hence the deeper context.
        let in_input = format!("{KEY_CONTEXT} > Input");
        cx.bind_keys([
            KeyBinding::new("cmd-enter", Translate, Some(in_input.as_str())),
            KeyBinding::new("cmd-enter", Translate, Some(KEY_CONTEXT)),
            KeyBinding::new("escape", HidePanel, Some(KEY_CONTEXT)),
            KeyBinding::new("cmd-shift-s", SwapDirection, Some(KEY_CONTEXT)),
            KeyBinding::new("cmd-shift-c", CopyResult, Some(KEY_CONTEXT)),
            KeyBinding::new("enter", NextStep, Some(onboarding::KEY_CONTEXT)),
            KeyBinding::new("escape", CloseOnboarding, Some(onboarding::KEY_CONTEXT)),
        ]);

        let traduko = match Traduko::start(support.clone(), cx) {
            Ok(traduko) => traduko,
            Err(error) => {
                eprintln!("traduko: cannot start: {error:#}");
                cx.quit();
                return;
            }
        };
        cx.set_global(Running(traduko.clone()));

        development_aids(traduko, cx);
    });
}

/// Ways to drive the app without a hand on the mouse, for captures:
/// `TRADUKO_DEMO=1` plays every mood in turn, `TRADUKO_PANEL=1` opens the
/// panel without taking the keyboard, `TRADUKO_TEXT=...` types that text into
/// it, `TRADUKO_CLICK="x,y;x,y"` then clicks those points of the panel window
/// (inside the app only: the real pointer does not move),
/// `TRADUKO_QUIT_AFTER=<seconds>` ends the run. While the first screens are up
/// they stand in for the panel: `TRADUKO_PANEL=1` shows them without the
/// keyboard and the clicks go to them. The windows are listed again after the
/// last click.
fn development_aids(traduko: gpui::Entity<Traduko>, cx: &mut App) {
    let variable = |name: &str| std::env::var(name).ok();
    let report = |traduko: &Traduko| {
        for (name, number, rect) in traduko.windows() {
            let rect = rect.map_or(String::new(), |r| format!(" at {:.0},{:.0},{:.0},{:.0}", r.x, r.y, r.w, r.h));
            println!("{name} window {number}{rect}");
        }
    };
    if ["TRADUKO_DEMO", "TRADUKO_PANEL", "TRADUKO_FPS", "TRADUKO_QUIT_AFTER"].iter().any(|name| variable(name).is_some()) {
        report(traduko.read(cx));
    }

    if variable("TRADUKO_DEMO").is_some() {
        let mascot = traduko.read(cx).mascot().clone();
        cx.spawn(async move |cx| {
            let script = [(1.2, Mood::Thinking), (2.2, Mood::Happy), (2.6, Mood::Sorry), (1.6, Mood::Waking), (1.6, Mood::Idle)];
            loop {
                for (wait, mood) in script {
                    cx.background_executor().timer(Duration::from_secs_f32(wait)).await;
                    mascot.update(cx, |view, cx| view.set_mood(mood, cx));
                }
            }
        })
        .detach();
    }

    if variable("TRADUKO_PANEL").is_some() {
        let text = variable("TRADUKO_TEXT");
        let clicks: Vec<(f32, f32)> = variable("TRADUKO_CLICK")
            .map(|list| list.split(';').filter_map(|point| point.split_once(',')).filter_map(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?))).collect())
            .unwrap_or_default();
        let traduko = traduko.clone();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_millis(600)).await;
            traduko.update(cx, |traduko, cx| traduko.show_panel(false, cx));
            cx.background_executor().timer(Duration::from_millis(400)).await;
            let (panel, window) = traduko.read_with(cx, |traduko, _| {
                report(traduko);
                (traduko.panel().cloned(), traduko.front_window())
            });
            if let (Some(text), Some(panel), Some(window)) = (text, panel, window) {
                window.update(cx, |_, window, cx| panel.update(cx, |panel, cx| panel.set_source(&text, window, cx))).ok();
            }
            for click in &clicks {
                cx.background_executor().timer(Duration::from_millis(1500)).await;
                // The window in front now: a click may have changed it.
                let Some(window) = traduko.read_with(cx, |traduko, _| traduko.front_window()) else {
                    continue;
                };
                let position = gpui::point(gpui::px(click.0), gpui::px(click.1));
                let modifiers = gpui::Modifiers::default();
                let left = gpui::MouseButton::Left;
                window
                    .update(cx, |_, window, cx| {
                        window.dispatch_event(gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent { position, pressed_button: None, modifiers }), cx);
                        window.dispatch_event(
                            gpui::PlatformInput::MouseDown(gpui::MouseDownEvent { button: left, position, modifiers, click_count: 1, first_mouse: false }),
                            cx,
                        );
                        window.dispatch_event(gpui::PlatformInput::MouseUp(gpui::MouseUpEvent { button: left, position, modifiers, click_count: 1 }), cx);
                    })
                    .ok();
                println!("clicked {},{}", click.0, click.1);
            }
            // A click may have opened a window: say where it is, to capture it.
            if !clicks.is_empty() {
                cx.background_executor().timer(Duration::from_millis(600)).await;
                traduko.read_with(cx, |traduko, _| report(traduko));
            }
        })
        .detach();
    }

    if let Some(seconds) = variable("TRADUKO_QUIT_AFTER").and_then(|value| value.parse::<f32>().ok()) {
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs_f32(seconds)).await;
            cx.update(|cx| cx.quit());
        })
        .detach();
    }
}
