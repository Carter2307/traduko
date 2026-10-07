//! The first run, as three screens in one window: what Traduko does, the model
//! it needs, and the one thing macOS has to allow. A screen with nothing to
//! ask is left out, and the screen of the models also opens alone, later, to
//! add the other one. So does a fourth screen, which is never part of the
//! first run: the languages that can be added to French and English.
//!
//! The window is built like the panel: a shell that holds three cards. The
//! top one is Traduko itself, alive: it watches the pointer, thinks while a
//! model downloads and hops when it is there.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Bounds, Context, Div, EventEmitter, FocusHandle, Focusable,
    FontWeight, Hsla, MouseButton, Pixels, SharedString, Stateful, Task, Window, actions, canvas, div, ease_in_out,
    linear_color_stop, linear_gradient, prelude::*, px, relative, rgb,
};
use gpui_component::Icon;
use gpui_kit_assets::IconName;
use traduko_blob::{CANVAS, Mascot, Mood};
use traduko_engine::{Direction, InstallUpdate, Installed, Language, Quality, download_size, install, install_language, language_download_size, languages};

use crate::login::{self, Permission};
use crate::mascot_view;
use crate::native;
use crate::theme::{self, MONO, Palette, SHELL_PAD, SHELL_RADIUS, card, chip, grip};

actions!(traduko_onboarding, [NextStep, CloseOnboarding]);

pub const KEY_CONTEXT: &str = "TradukoOnboarding";

/// The shell, without the transparent margin around it.
pub const SHELL_WIDTH: f32 = 440.0;
pub const SHELL_HEIGHT: f32 = 640.0;

/// How often the login item is read again while macOS waits for the user:
/// the answer is given in System Settings, and nothing tells Traduko.
const ASK_AGAIN: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Models,
    /// The languages to add. Only opened alone, from the panel.
    Languages,
    Login,
}

/// The screens of a run. The first one gets the tour; later only the models
/// are left to show. The login item is brought up when there is something
/// for the user to allow, and not where it is settled or out of reach.
pub fn steps(first_run: bool, permission: Permission) -> Vec<Step> {
    match (first_run, permission.is_pending()) {
        (true, true) => vec![Step::Welcome, Step::Models, Step::Login],
        (true, false) => vec![Step::Welcome, Step::Models],
        (false, _) => vec![Step::Models],
    }
}

pub enum OnboardingEvent {
    /// Traduko's mood changed here: the mascot on the desktop follows.
    Mood(Mood),
    /// A model set or a language is on disk now.
    ModelInstalled,
    /// The user asked for the login item, and this is what macOS did.
    LoginChanged(login::Outcome),
    /// The last screen was left by its button. `open_at_login` is what the
    /// user chose, when the question was put.
    Finished { open_at_login: Option<bool> },
    /// The window was closed before the end.
    Dismissed,
    QuitRequested,
}

pub struct OnboardingOptions {
    /// The screens to go through, in order.
    pub steps: Vec<Step>,
    pub models_dir: PathBuf,
    pub installed: Installed,
    pub permission: Permission,
}

/// What a line of the lists downloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pack {
    /// French and English, both ways, in one quality.
    Set(Quality),
    /// Another language, to English and from it.
    Language(Language),
}

impl Pack {
    /// How much there is to download, in bytes.
    fn size(self) -> u64 {
        match self {
            Pack::Set(quality) => download_size(quality),
            Pack::Language(language) => language_download_size(language),
        }
    }

    fn install(self, models_dir: PathBuf) -> UnboundedReceiver<InstallUpdate> {
        match self {
            Pack::Set(quality) => install(models_dir, quality),
            Pack::Language(language) => install_language(models_dir, language),
        }
    }

    fn is_among(self, installed: &Installed) -> bool {
        match self {
            Pack::Set(quality) => installed.sets().contains(&quality),
            Pack::Language(language) => {
                let to_english = Direction::new(language, Language::ENGLISH);
                installed.translates(to_english) && installed.translates(to_english.swapped())
            }
        }
    }
}

#[derive(Clone, PartialEq)]
enum Fetching {
    /// Not on this Mac.
    No,
    Downloading { done: u64, total: u64 },
    Checking,
    Installed,
    Failed(SharedString),
}

struct Model {
    pack: Pack,
    state: Fetching,
    /// The download, while it runs. Dropping it stops it; what is on disk
    /// stays, and the next download goes on from there.
    job: Option<Task<()>>,
}

pub struct Onboarding {
    focus: FocusHandle,
    steps: Vec<Step>,
    at: usize,
    /// Where the line under the header was before the last change of
    /// screen, 0 to 1: it travels from there.
    line_from: f32,
    models_dir: PathBuf,
    /// The two sets, then the languages.
    models: Vec<Model>,
    permission: Permission,
    /// The user pressed Allow.
    asked: bool,
    /// macOS has not answered yet.
    asking: bool,
    watch: Option<Task<()>>,
    mascot: Mascot,
    last_tick: Instant,
    /// Where Traduko was painted last, to know where the pointer is from it.
    hero: Rc<Cell<Option<Bounds<Pixels>>>>,
    wake: Option<Task<()>>,
}

impl EventEmitter<OnboardingEvent> for Onboarding {}

impl Focusable for Onboarding {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Onboarding {
    pub fn new(options: OnboardingOptions, cx: &mut Context<Self>) -> Self {
        let model = |pack: Pack| Model { pack, state: if pack.is_among(&options.installed) { Fetching::Installed } else { Fetching::No }, job: None };
        // The better set first: it is the one to take without a reason to
        // take the other.
        let sets = [Pack::Set(Quality::Accurate), Pack::Set(Quality::Light)];
        let models = sets.into_iter().chain(languages().into_iter().map(Pack::Language)).map(model).collect();
        let mut mascot = Mascot::new(Mood::Idle).tinted(theme::palette(cx).accent.body());
        mascot.enter();

        let mut this = Self {
            focus: cx.focus_handle(),
            steps: options.steps,
            at: 0,
            line_from: 0.0,
            models_dir: options.models_dir,
            models,
            permission: Permission::Unavailable,
            asked: false,
            asking: false,
            watch: None,
            mascot,
            last_tick: Instant::now(),
            hero: Rc::new(Cell::new(None)),
            wake: None,
        };
        this.set_permission(options.permission, cx);
        this
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    fn step(&self) -> Step {
        self.steps[self.at]
    }

    fn is_last(&self) -> bool {
        self.at + 1 == self.steps.len()
    }

    /// How far through the screens this one is, 0 to 1. A screen that is
    /// alone is not on the way to anything: its line stays neutral.
    fn progress(&self) -> f32 {
        if self.steps.len() < 2 { 0.0 } else { (self.at + 1) as f32 / self.steps.len() as f32 }
    }

    fn model(&mut self, pack: Pack) -> &mut Model {
        let at = self.models.iter().position(|model| model.pack == pack).unwrap_or(0);
        &mut self.models[at]
    }

    /// Translating takes one model: the screen of the models is left with
    /// one, and the others are left freely.
    fn can_continue(&self) -> bool {
        self.step() != Step::Models || self.has_model()
    }

    fn has_model(&self) -> bool {
        self.models.iter().any(|model| model.state == Fetching::Installed)
    }

    fn go(&mut self, to: usize, cx: &mut Context<Self>) {
        self.line_from = self.progress();
        self.at = to;
        self.mascot.nudge();
        cx.notify();
    }

    fn next(&mut self, cx: &mut Context<Self>) {
        if !self.can_continue() {
            return;
        }
        if !self.is_last() {
            return self.go(self.at + 1, cx);
        }
        // Without the question, or without an answer to give, the setting
        // stays what it was.
        let put = self.steps.contains(&Step::Login) && self.permission != Permission::Unavailable;
        let open_at_login = put.then_some(self.asked || self.permission == Permission::Allowed);
        cx.emit(OnboardingEvent::Finished { open_at_login });
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.at > 0 {
            self.go(self.at - 1, cx);
        }
    }

    // ---- the models ---------------------------------------------------------

    fn download(&mut self, pack: Pack, cx: &mut Context<Self>) {
        if !matches!(self.model(pack).state, Fetching::No | Fetching::Failed(_)) {
            return;
        }
        let mut updates = pack.install(self.models_dir.clone());
        let job = cx.spawn(async move |this, cx| {
            let mut finished = false;
            while let Some(update) = updates.next().await {
                finished = matches!(update, InstallUpdate::Done | InstallUpdate::Failed(_));
                if this.update(cx, |this, cx| this.apply(pack, update, cx)).is_err() {
                    return;
                }
            }
            // A stream that just ends means the download died on its own.
            if !finished {
                let stopped = InstallUpdate::Failed("The download stopped. Try again.".into());
                this.update(cx, |this, cx| this.apply(pack, stopped, cx)).ok();
            }
        });
        let model = self.model(pack);
        model.state = Fetching::Downloading { done: 0, total: pack.size() };
        model.job = Some(job);
        cx.notify();
    }

    fn stop(&mut self, pack: Pack, cx: &mut Context<Self>) {
        let model = self.model(pack);
        // The last update may have come while the pointer went down.
        if matches!(model.state, Fetching::Downloading { .. } | Fetching::Checking) {
            model.job = None;
            model.state = Fetching::No;
            cx.notify();
        }
    }

    fn apply(&mut self, pack: Pack, update: InstallUpdate, cx: &mut Context<Self>) {
        let model = self.model(pack);
        match update {
            InstallUpdate::Downloading { done, total } => model.state = Fetching::Downloading { done, total },
            InstallUpdate::Checking => model.state = Fetching::Checking,
            InstallUpdate::Failed(why) => model.state = Fetching::Failed(why.into()),
            InstallUpdate::Done => {
                model.state = Fetching::Installed;
                self.cheer(cx);
                cx.emit(OnboardingEvent::ModelInstalled);
            }
        }
        cx.notify();
    }

    // ---- the login item -------------------------------------------------------

    fn allow_login(&mut self, cx: &mut Context<Self>) {
        if self.asking {
            return;
        }
        (self.asked, self.asking) = (true, true);
        // A system service answers, in tens of milliseconds: not on this thread.
        let answer = cx.background_spawn(async { (login::set(true), login::permission()) });
        cx.spawn(async move |this, cx| {
            let (outcome, permission) = answer.await;
            this.update(cx, |this, cx| {
                this.asking = false;
                this.set_permission(permission, cx);
                cx.emit(OnboardingEvent::LoginChanged(outcome));
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn set_permission(&mut self, permission: Permission, cx: &mut Context<Self>) {
        if permission == Permission::Allowed && self.permission.is_pending() {
            self.cheer(cx);
        }
        self.permission = permission;
        self.watch = (permission == Permission::NeedsApproval).then(|| {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(ASK_AGAIN).await;
                    let now = cx.background_executor().spawn(async { login::permission() }).await;
                    if now != Permission::NeedsApproval {
                        this.update(cx, |this, cx| this.set_permission(now, cx)).ok();
                        return;
                    }
                }
            })
        });
        cx.notify();
    }

    // ---- Traduko ---------------------------------------------------------------------

    /// A hop, then back to what the downloads call for.
    fn cheer(&mut self, cx: &mut Context<Self>) {
        self.mascot.set_mood(Mood::Happy);
        cx.emit(OnboardingEvent::Mood(Mood::Happy));
    }

    fn is_downloading(&self) -> bool {
        self.models.iter().any(|model| matches!(model.state, Fetching::Downloading { .. } | Fetching::Checking))
    }

    /// The mood that says what the downloads are doing.
    fn mood(&self) -> Mood {
        if self.is_downloading() {
            Mood::Thinking
        } else if self.models.iter().any(|model| matches!(model.state, Fetching::Failed(_))) {
            Mood::Sorry
        } else {
            Mood::Idle
        }
    }

    /// Advances Traduko to now and books its next frame: the next display
    /// refresh while something moves, a timer for the next blink otherwise.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;

        // A hop ends by itself: only then does the mood follow the downloads.
        let mood = self.mood();
        if self.mascot.mood() != Mood::Happy && self.mascot.mood() != mood {
            self.mascot.set_mood(mood);
            cx.emit(OnboardingEvent::Mood(mood));
        }
        self.mascot.set_pointer(self.hero.get().and_then(|hero| pointer_from(hero, window)));
        self.mascot.step(dt);

        if self.mascot.is_resting() {
            let delay = Duration::from_secs_f32(self.mascot.next_wake().max(0.02));
            self.wake = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            }));
        } else {
            self.wake = None;
            window.request_animation_frame();
        }
    }
}

/// The pointer as Traduko sees it from where it is painted, in its own units
/// from its centre, while the pointer is over the window. Traduko is interested
/// up to 6 units away, which is about its own height here, so what is
/// farther than its body is brought closer: it follows the pointer over the
/// whole window.
fn pointer_from(hero: Bounds<Pixels>, window: &Window) -> Option<[f32; 2]> {
    const BODY: f32 = 1.3;
    const NEARER: f32 = 0.3;
    // Asked from AppKit, like the mascot does: gpui only knows the pointer
    // from the events of this window, and so never sees it leave.
    let frame = native::frame(native::ns_window(window)?.as_ref());
    let pointer = native::pointer();
    let across = pointer.0 - frame.x;
    // The frame counts upwards from its bottom, the view downwards from its top.
    let down = frame.y + frame.h - pointer.1;
    if !(0.0..=frame.w).contains(&across) || !(0.0..=frame.h).contains(&down) {
        return None;
    }
    let scale = hero.size.width.as_f32() / (2.0 * CANVAS);
    let x = (across as f32 - hero.center().x.as_f32()) / scale;
    let y = (down as f32 - hero.center().y.as_f32()) / scale;
    let distance = x.hypot(y);
    if distance <= BODY {
        return Some([x, y]);
    }
    let seen = (BODY + (distance - BODY) * NEARER) / distance;
    Some([x * seen, y * seen])
}

/// The colours of the tiles that carry an icon.
#[derive(Clone, Copy)]
enum Tint {
    Orange,
    Blue,
    Violet,
    Green,
}

impl Tint {
    /// Top and bottom: a tile is lit from above. The same on light and dark.
    fn colors(self) -> (Hsla, Hsla) {
        let (top, bottom) = match self {
            Tint::Orange => (0xff9a5e, 0xf45a1c),
            Tint::Blue => (0x62b0ff, 0x2b7bf0),
            Tint::Violet => (0xb79cff, 0x7b52ee),
            Tint::Green => (0x63dc96, 0x1fb46a),
        };
        (rgb(top).into(), rgb(bottom).into())
    }
}

fn tile(icon: IconName, tint: Tint) -> Div {
    let (top, bottom) = tint.colors();
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(40.))
        .rounded(px(13.))
        .bg(linear_gradient(180., linear_color_stop(top, 0.), linear_color_stop(bottom, 1.)))
        .text_color(gpui::white())
        .child(Icon::new(icon).size(px(19.)))
}

/// One line of a list: a tile, a title over a detail, and a control or
/// nothing at the right.
fn row(tile: Div, title: impl IntoElement, detail: impl IntoElement, control: Option<AnyElement>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(16.))
        .py(px(13.))
        .child(tile)
        .child(div().flex().flex_col().flex_1().min_w_0().gap(px(2.)).child(title).child(detail))
        .children(control)
}

fn title(text: impl Into<SharedString>, p: &Palette) -> Div {
    div().text_size(px(14.5)).font_weight(FontWeight::MEDIUM).text_color(p.ink).child(text.into())
}

fn detail(text: impl Into<SharedString>, color: Hsla) -> Div {
    div().text_size(px(13.)).line_height(px(18.)).text_color(color).child(text.into())
}

/// The mark at the right of a line that is settled.
fn settled(text: &'static str, p: &Palette) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .text_size(px(13.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.ok)
        .child(Icon::new(IconName::CircleCheck).size(px(16.)))
        .child(text)
        .into_any_element()
}

/// The lines of a list, with a hairline between two of them.
fn lines(rows: Vec<Div>, p: &Palette) -> Div {
    let mut lines = div().flex().flex_col();
    for (at, row) in rows.into_iter().enumerate() {
        if at > 0 {
            lines = lines.child(div().h(px(1.)).bg(p.line.opacity(0.6)));
        }
        lines = lines.child(row);
    }
    lines
}

/// Bytes as the megabytes that the Finder shows.
pub fn megabytes(bytes: u64) -> u64 {
    (bytes as f64 / 1e6).round() as u64
}

impl Onboarding {
    fn features(&self, p: &Palette) -> Vec<Div> {
        let feature = |icon, tint, name: &'static str, about: &'static str| row(tile(icon, tint), title(name, p), detail(about, p.muted), None);
        vec![
            feature(IconName::Languages, Tint::Orange, "Translation as you type", "Write in a language I have: I work out which."),
            feature(IconName::ShieldCheck, Tint::Blue, "Nothing leaves this Mac", "The model runs here. No account, no network."),
            feature(IconName::SpellCheck, Tint::Violet, "American or British", "Color or colour, truck or lorry: you choose."),
            feature(IconName::MousePointerClick, Tint::Green, "Always on your desktop", "Click me to translate, drag me out of the way."),
        ]
    }

    /// The lines of the languages, in the order of the engine's table.
    fn language_rows(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<Div> {
        let languages: Vec<Pack> = self.models.iter().map(|model| model.pack).filter(|pack| matches!(pack, Pack::Language(_))).collect();
        languages.into_iter().map(|pack| self.model_row(pack, p, cx)).collect()
    }

    fn model_row(&self, pack: Pack, p: &Palette, cx: &mut Context<Self>) -> Div {
        // Its place in the list tells its controls from those of the others.
        let at = self.models.iter().position(|model| model.pack == pack).unwrap_or(0);
        let (name, about, icon, tint): (SharedString, SharedString, IconName, Tint) = match pack {
            Pack::Set(Quality::Accurate) => ("Accurate".into(), "The most faithful translations.".into(), IconName::Sparkles, Tint::Orange),
            Pack::Set(Quality::Light) => ("Light".into(), "Smaller and quicker to download.".into(), IconName::Feather, Tint::Blue),
            Pack::Language(language) => {
                let tints = [Tint::Blue, Tint::Violet, Tint::Green, Tint::Orange];
                (language.native_name().to_string().into(), format!("{}, to English and back.", language.name()).into(), IconName::Languages, tints[at % tints.len()])
            }
        };
        let state = self.models[at].state.clone();

        let get = |label: &'static str, cx: &mut Context<Self>| {
            chip(("get", at), p).flex_none().px(px(14.)).on_click(cx.listener(move |this, _, _, cx| this.download(pack, cx))).child(label).into_any_element()
        };
        let stop = |cx: &mut Context<Self>| {
            chip(("stop", at), p)
                .flex_none()
                .w(px(32.))
                .on_click(cx.listener(move |this, _, _, cx| this.stop(pack, cx)))
                .child(Icon::new(IconName::X).size(px(14.)))
                .into_any_element()
        };
        // Three lines whatever the state, so that nothing jumps when a
        // download starts or ends: the name, a sentence or the bar that
        // stands in for it, and a line of numbers.
        let sentence = |color: Hsla| detail(about.clone(), color).into_any_element();
        let numbers = |text: String| div().font_family(MONO).text_size(px(12.)).line_height(px(18.)).text_color(p.muted).child(text).into_any_element();
        let bar = |fill: AnyElement| div().h(px(18.)).flex().items_center().child(div().w_full().h(px(3.)).rounded_full().bg(p.line).overflow_hidden().child(fill)).into_any_element();

        let size = megabytes(pack.size());
        let (middle, lower, control): (AnyElement, AnyElement, AnyElement) = match state {
            Fetching::No => (sentence(p.muted), numbers(format!("{size} MB")), get("Download", cx)),
            Fetching::Installed => (sentence(p.muted), numbers("On this Mac".into()), settled("Installed", p)),
            Fetching::Failed(why) => (detail(why, p.ink).into_any_element(), numbers(format!("{size} MB")), get("Try again", cx)),
            Fetching::Downloading { done, total } => {
                let fraction = if total == 0 { 0.0 } else { (done as f32 / total as f32).clamp(0.0, 1.0) };
                let fill = div().h_full().rounded_full().bg(p.primary).w(relative(fraction)).into_any_element();
                (bar(fill), numbers(format!("{} of {} MB", megabytes(done), megabytes(total))), stop(cx))
            }
            Fetching::Checking => {
                // Nothing to measure: a short segment that travels.
                let fill = div()
                    .h_full()
                    .w(relative(0.3))
                    .rounded_full()
                    .bg(p.primary)
                    .with_animation(("checking", at), Animation::new(Duration::from_millis(1100)).repeat().with_easing(ease_in_out), |segment, t| {
                        segment.ml(relative(t * 0.7))
                    })
                    .into_any_element();
                (bar(fill), numbers("Checking the download".into()), stop(cx))
            }
        };
        let lower = div().flex().flex_col().child(middle).child(lower);

        let recommended = div()
            .px(px(7.))
            .h(px(18.))
            .flex()
            .items_center()
            .rounded_full()
            .bg(p.primary.opacity(0.13))
            .text_size(px(11.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.primary)
            .child("Recommended");
        let heading = div().flex().items_center().gap(px(8.)).child(title(name, p)).when(pack == Pack::Set(Quality::Accurate), |heading| heading.child(recommended));
        row(tile(icon, tint), heading, lower, Some(control))
    }

    fn login_rows(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<Div> {
        let (about, control): (&'static str, Option<AnyElement>) = match self.permission {
            Permission::NotAsked => {
                let allow = chip("allow", p)
                    .flex_none()
                    .px(px(14.))
                    .when(self.asking, |chip| chip.opacity(0.45))
                    .on_click(cx.listener(|this, _, _, cx| this.allow_login(cx)))
                    .child("Allow");
                ("macOS adds me to your login items.", Some(allow.into_any_element()))
            }
            Permission::Allowed => ("I'll be here after a restart.", Some(settled("Allowed", p))),
            Permission::NeedsApproval => {
                let open = chip("open-settings", p)
                    .flex_none()
                    .px(px(14.))
                    .on_click(cx.listener(|_, _, _, cx| cx.background_spawn(async { login::open_system_settings() }).detach()))
                    .child("Open Settings");
                ("Switch Traduko on in System Settings, under Login Items.", Some(open.into_any_element()))
            }
            Permission::Unavailable => ("Works once I'm in your Applications folder.", None),
        };
        vec![
            row(tile(IconName::Power, Tint::Green), title("Open at login", p), detail(about, p.muted), control),
            row(
                tile(IconName::EyeOff, Tint::Violet),
                title("Nothing else to allow", p),
                detail("I don't read your screen, your keys or your files.", p.muted),
                None,
            ),
        ]
    }
}

impl Render for Onboarding {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(window, cx);
        let p = theme::palette(cx);
        let step = self.step();
        let several = self.steps.len() > 1;

        // ---- header: grip, name, where we are, close ----------------------------
        // On the shell, where a chip would not show: the colour of a card.
        let close_hover = p.chip_hover;
        let close = div()
            .id("close")
            .flex()
            .items_center()
            .justify_center()
            .size(px(28.))
            .rounded_full()
            .bg(p.card)
            .text_color(p.ink)
            .cursor_pointer()
            .hover(move |style| style.bg(close_hover))
            // A press here is not a grab of the header.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|_, _, _, cx| cx.emit(OnboardingEvent::Dismissed)))
            .child(Icon::new(IconName::X).size(px(14.)));
        let header = div()
            .id("header")
            .flex()
            .items_center()
            .justify_between()
            .h(px(46.))
            .pl(px(18.))
            .pr(px(10.))
            // The window has no title bar: the header moves it.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(grip(&p))
                    .child(div().text_size(px(16.)).font_weight(FontWeight::MEDIUM).text_color(p.ink).child("Traduko")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .when(several, |right| {
                        right.child(
                            div().text_size(px(13.5)).text_color(p.ink.opacity(0.72)).child(format!("Step {} of {}", self.at + 1, self.steps.len())),
                        )
                    })
                    .child(close),
            );

        // ---- the line under the header: how far through the screens ----------------
        let (from, to) = (self.line_from, self.progress());
        let line = div().mx(px(18.)).mb(px(8.)).h(px(2.)).rounded_full().bg(p.line).overflow_hidden().child(
            div().h_full().rounded_full().bg(p.primary).with_animation(
                ("screen", self.at),
                Animation::new(Duration::from_millis(280)).with_easing(ease_in_out),
                move |fill, t| fill.w(relative(from + (to - from) * t)),
            ),
        );

        // ---- Traduko, and what this screen is about -------------------------------------
        let (headline, about): (&str, &str) = match step {
            Step::Welcome => ("Bonjour, I'm Traduko", "I translate as you type, right here on your desktop."),
            Step::Models => ("Download a model", "A model does the translating: these two know French and English. It comes from Hugging Face once, then I work offline."),
            Step::Languages => ("Add a language", "Each one is two small models, to English and from it. Between two of them, I go through English."),
            Step::Login => ("Keep me around", "With your permission I open by myself when you log in, so I'm here after a restart."),
        };
        let (frame, seen) = (self.mascot.frame(), self.hero.clone());
        let traduko = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                seen.set(Some(bounds));
                mascot_view::paint(&frame, bounds, window);
            },
        )
        .flex_none()
        // More room on the screens with a shorter list.
        .size(px(match step {
            Step::Welcome => 100.,
            Step::Languages => 84.,
            Step::Models | Step::Login => 124.,
        }));
        // The languages are more than the window is tall: their list takes
        // the room and scrolls, and Traduko keeps to what it needs.
        let scrolls = step == Step::Languages;
        let hero = card(&p)
            .flex()
            .flex_col()
            .map(|hero| if scrolls { hero.flex_none().pt(px(4.)) } else { hero.flex_1().min_h_0() })
            .items_center()
            .justify_center()
            .px(px(28.))
            // Traduko's square has room above the body for its hop: the same
            // below the text keeps the two in the middle of the card.
            .pb(px(12.))
            .child(traduko)
            .child(div().mt(px(4.)).text_size(px(23.)).font_weight(FontWeight::SEMIBOLD).text_color(p.ink).child(headline))
            .child(div().mt(px(6.)).max_w(px(332.)).text_center().text_size(px(14.)).line_height(px(20.)).text_color(p.muted).child(about));

        // ---- the list ----------------------------------------------------------------------
        let rows = match step {
            Step::Welcome => self.features(&p),
            Step::Models => vec![self.model_row(Pack::Set(Quality::Accurate), &p, cx), self.model_row(Pack::Set(Quality::Light), &p, cx)],
            Step::Languages => self.language_rows(&p, cx),
            Step::Login => self.login_rows(&p, cx),
        };
        let list = if scrolls {
            card(&p).id("languages").flex_1().min_h_0().overflow_y_scroll().child(lines(rows, &p)).into_any_element()
        } else {
            card(&p).flex_none().child(lines(rows, &p)).into_any_element()
        };

        // ---- footer: back, and the way on ---------------------------------------------------
        let can_continue = self.can_continue();
        let onward = match (self.is_last(), several) {
            (false, _) => "Continue",
            (true, true) => "Start translating",
            (true, false) => "Done",
        };
        let next: Stateful<Div> = div()
            .id("next")
            .flex()
            .items_center()
            .justify_center()
            .h(px(36.))
            .px(px(18.))
            .rounded_full()
            .bg(p.primary)
            .text_color(gpui::white())
            .text_size(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .child(onward);
        let next = if can_continue {
            next.cursor_pointer().hover(|style| style.opacity(0.9)).on_click(cx.listener(|this, _, _, cx| this.next(cx)))
        } else {
            next.opacity(0.4)
        };
        // Without a model the panel does not open, and the way to quit is
        // in the panel's menu: so it is here too, where nothing leads back.
        let quit = div()
            .id("quit")
            .px(px(8.))
            .text_size(px(13.))
            .text_color(p.muted)
            .cursor_pointer()
            .hover(|style| style.text_color(p.ink))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(OnboardingEvent::QuitRequested)))
            .child("Quit Traduko");
        let footer = card(&p).flex().flex_none().items_center().justify_between().h(px(56.)).pl(px(12.)).pr(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .when(self.at > 0, |left| left.child(chip("back", &p).px(px(14.)).on_click(cx.listener(|this, _, _, cx| this.back(cx))).child("Back")))
                .when(self.at == 0 && !self.has_model(), |left| left.child(quit)),
        );
        let footer = footer.child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                // A button that does nothing owes a word about why, unless
                // a download is already saying it.
                .when(!can_continue && !self.is_downloading(), |right| {
                    right.child(div().text_size(px(13.)).text_color(p.muted).child("One model is enough"))
                })
                .child(next),
        );

        let shell = div()
            .id("traduko-onboarding")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NextStep, _, cx| this.next(cx)))
            .on_action(cx.listener(|_, _: &CloseOnboarding, _, cx| cx.emit(OnboardingEvent::Dismissed)))
            // Traduko looks at the pointer: every move is a new frame.
            .on_mouse_move(cx.listener(|_, _, _, cx| cx.notify()))
            .w(px(SHELL_WIDTH))
            .h(px(SHELL_HEIGHT))
            .flex()
            .flex_col()
            .p(px(SHELL_PAD))
            .rounded(px(SHELL_RADIUS))
            .font_family(theme::SANS)
            .text_color(p.ink)
            .child(header)
            .child(line)
            .child(div().flex().flex_col().flex_1().min_h_0().gap(px(8.)).child(hero).child(list).child(footer));

        div().size_full().flex().items_center().justify_center().child(shell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_run_gets_the_tour_and_later_ones_only_the_models() {
        assert_eq!(steps(true, Permission::NotAsked), [Step::Welcome, Step::Models, Step::Login]);
        assert_eq!(steps(false, Permission::NotAsked), [Step::Models]);
    }

    #[test]
    fn the_login_item_is_brought_up_only_when_there_is_something_to_allow() {
        assert_eq!(steps(true, Permission::NeedsApproval).last(), Some(&Step::Login));
        // Already on, or a copy that cannot be a login item at all.
        assert_eq!(steps(true, Permission::Allowed), [Step::Welcome, Step::Models]);
        assert_eq!(steps(true, Permission::Unavailable), [Step::Welcome, Step::Models]);
    }

    #[test]
    fn sizes_are_given_in_whole_megabytes_as_the_finder_counts_them() {
        assert_eq!(megabytes(download_size(Quality::Accurate)), 923);
        assert_eq!(megabytes(download_size(Quality::Light)), 602);
        assert_eq!(megabytes(0), 0);
    }
}
