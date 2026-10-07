//! The translator panel: a rounded shell that holds three cards (source,
//! result, footer). Translation starts by itself a moment after typing stops.
//!
//! Each of the two text cards names its language, and that name is a menu
//! of the languages that have a model. The top one can also be left to
//! Traduko, which then reads the language off the text.

use std::time::Duration;

use futures::StreamExt as _;
use gpui::{
    Animation, AnimationExt as _, App, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, Hsla, MouseButton, SharedString, Subscription, Task, WeakEntity, Window, actions, canvas, div,
    ease_in_out, prelude::*, px, relative,
};
use gpui_component::{
    Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{InputEvent, Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
};
use gpui_kit_assets::IconName;
use traduko_blob::{Frame, Mascot, Mood};
use traduko_engine::{Direction, EnglishVariant, Installed, Language, Quality, Request, Translator, Update};

use crate::detect;
use crate::mascot_view;
use crate::settings::{Accent, MascotSize};
use crate::theme::{self, MONO, SHELL_PAD, SHELL_RADIUS, card, chip, grip, micro};

actions!(
    traduko,
    [
        Translate,
        HidePanel,
        SwapDirection,
        CopyResult,
        UseLight,
        UseAccurate,
        MascotSmall,
        MascotMedium,
        MascotLarge,
        ToggleOpenAtLogin,
        ShowModels,
        QuitTraduko
    ]
);

pub const KEY_CONTEXT: &str = "TradukoPanel";

/// The shell, without the transparent margin that holds its shadow.
pub const SHELL_WIDTH: f32 = 404.0;
pub const SHELL_HEIGHT: f32 = 548.0;
/// Transparent room around the shell for the shadow.
pub const MARGIN: f32 = theme::SHELL_MARGIN;

const DEBOUNCE: Duration = Duration::from_millis(350);

pub enum PanelEvent {
    /// A translation started.
    Working,
    Translated,
    Failed,
    /// The source text is empty again.
    Cleared,
    /// The result went to the clipboard.
    Copied,
    HideRequested,
    /// The languages, the English variant or the model changed.
    PreferencesChanged,
    MascotSizeChanged(MascotSize),
    AccentChanged(Accent),
    OpenAtLoginChanged(bool),
    /// The user wants to download a model.
    ModelsRequested,
    /// The user wants a language that is not in the menus.
    LanguagesRequested,
    QuitRequested,
}

#[derive(Clone, PartialEq)]
enum Phase {
    Idle,
    /// Typing: the translation starts when it pauses.
    Typing,
    LoadingModel,
    Translating { done: usize, total: usize },
    Done { elapsed: Duration },
    Failed(SharedString),
}

pub struct Panel {
    focus: FocusHandle,
    source: Entity<TextareaState>,
    result: Entity<TextareaState>,
    direction: Direction,
    /// Traduko reads the language off the text. Off when the user chose the
    /// language to translate from.
    detect: bool,
    /// Set when the user swapped the languages by hand: Traduko then stops
    /// choosing the direction itself until the text is cleared.
    direction_pinned: bool,
    english: EnglishVariant,
    quality: Quality,
    installed: Installed,
    mascot_size: MascotSize,
    open_at_login: bool,
    phase: Phase,
    debounce: Option<Task<()>>,
    /// The running translation. Replacing it drops the stream, which tells
    /// the engine to stop.
    job: Option<Task<()>>,
    copied: bool,
    translator: Translator,
    face: Frame,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PanelEvent> for Panel {}

impl Focusable for Panel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

pub struct PanelOptions {
    pub direction: Direction,
    pub detect: bool,
    pub english: EnglishVariant,
    pub quality: Quality,
    pub installed: Installed,
    pub mascot_size: MascotSize,
    pub open_at_login: bool,
}

impl Panel {
    pub fn new(translator: Translator, options: PanelOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // The languages of the last run may have left the disk since.
        let direction = servable(&options.installed, options.direction);
        let source = cx.new(|cx| TextareaState::new(window, cx).placeholder(source_placeholder(direction.from)));
        let result = cx.new(|cx| TextareaState::new(window, cx));

        let focus = cx.focus_handle();
        let subscriptions = vec![
            // A click on a tab, a button or the header focuses the shell:
            // hand the keyboard straight back to the text.
            cx.on_focus(&focus, window, |this, window, cx| this.focus_source(window, cx)),
            cx.subscribe_in(&source, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.on_typed(window, cx);
                }
            }),
            cx.observe_window_appearance(window, |_, window, cx| theme::apply(window.appearance(), cx)),
        ];

        Self {
            focus,
            source,
            result,
            direction,
            detect: options.detect,
            direction_pinned: false,
            english: options.english,
            quality: options.quality,
            installed: options.installed,
            mascot_size: options.mascot_size,
            open_at_login: options.open_at_login,
            phase: Phase::Idle,
            debounce: None,
            job: None,
            copied: false,
            translator,
            face: Mascot::new(Mood::Idle).frame(),
            _subscriptions: subscriptions,
        }
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// True when Traduko reads the language off the text.
    pub fn detects(&self) -> bool {
        self.detect
    }

    pub fn english(&self) -> EnglishVariant {
        self.english
    }

    pub fn quality(&self) -> Quality {
        self.quality
    }

    /// Models were downloaded: they join the menus, and `quality` is the
    /// one to use now.
    pub fn set_installed(&mut self, installed: Installed, quality: Quality, cx: &mut Context<Self>) {
        self.direction = servable(&installed, self.direction);
        self.installed = installed;
        self.quality = quality;
        cx.notify();
    }

    pub fn set_open_at_login(&mut self, on: bool, cx: &mut Context<Self>) {
        self.open_at_login = on;
        cx.notify();
    }

    pub fn focus_source(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.source.update(cx, |state, cx| state.focus(window, cx));
    }

    /// Replaces the source text and translates it at once.
    pub fn set_source(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.source.update(cx, |state, cx| state.set_value(text.to_string(), window, cx));
        self.translate_now(window, cx);
    }

    fn source_text(&self, cx: &App) -> String {
        self.source.read(cx).value().to_string()
    }

    fn result_text(&self, cx: &App) -> String {
        self.result.read(cx).value().to_string()
    }

    fn on_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The text changed: what is being translated is already out of date.
        self.job = None;
        self.phase = Phase::Typing;
        self.debounce = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            this.update_in(cx, |this, window, cx| this.translate_now(window, cx)).ok();
        }));
        cx.notify();
    }

    fn translate_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.debounce = None;
        let text = self.source_text(cx);
        if text.trim().is_empty() {
            self.job = None;
            self.direction_pinned = false;
            self.phase = Phase::Idle;
            self.result.update(cx, |state, cx| state.set_value("", window, cx));
            cx.emit(PanelEvent::Cleared);
            cx.notify();
            return;
        }

        // Typing English while the panel says French: follow the text.
        if self.detect
            && !self.direction_pinned
            && let Some(written_in) = detect::language(&text, &self.installed.languages())
        {
            let direction = reading(written_in, self.direction);
            if direction != self.direction {
                self.set_direction(direction, window, cx);
            }
        }

        let mut updates = self.translator.translate(Request {
            text,
            direction: self.direction,
            english: self.english,
            quality: self.quality,
        });
        self.phase = Phase::Translating { done: 0, total: 0 };
        cx.emit(PanelEvent::Working);
        cx.notify();

        self.job = Some(cx.spawn_in(window, async move |this, cx| {
            let mut finished = false;
            while let Some(update) = updates.next().await {
                finished = matches!(update, Update::Done { .. } | Update::Failed(_));
                if this.update_in(cx, |this, window, cx| this.apply(update, window, cx)).is_err() || finished {
                    break;
                }
            }
            // A request replaced by a newer one never gets here: its task is
            // dropped. So a stream that just ends means the engine stopped.
            if !finished {
                this.update_in(cx, |this, window, cx| this.apply(Update::Failed("The translator stopped.".into()), window, cx)).ok();
            }
        }));
    }

    fn apply(&mut self, update: Update, window: &mut Window, cx: &mut Context<Self>) {
        match update {
            Update::LoadingModel => self.phase = Phase::LoadingModel,
            Update::Partial { text, done, total } => {
                self.show_result(text, window, cx);
                self.phase = Phase::Translating { done, total };
            }
            Update::Done { text, elapsed, .. } => {
                self.show_result(text, window, cx);
                self.phase = Phase::Done { elapsed };
                cx.emit(PanelEvent::Translated);
            }
            Update::Failed(message) => {
                // What is on screen is not the translation of this text.
                self.result.update(cx, |state, cx| state.set_value("", window, cx));
                self.phase = Phase::Failed(message.into());
                cx.emit(PanelEvent::Failed);
            }
        }
        cx.notify();
    }

    /// Puts a translation in the result card without moving what the user
    /// is reading: the same text is left alone, a longer one keeps the scroll.
    fn show_result(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.result.update(cx, |state, cx| {
            if state.value().as_ref() == text.as_str() {
                return;
            }
            let offset = state.scroll_offset();
            state.set_value(text, window, cx);
            state.set_scroll_offset(offset, cx);
        });
    }

    fn set_direction(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        self.direction = direction;
        let placeholder = source_placeholder(direction.from);
        self.source.update(cx, |state, cx| state.set_placeholder(placeholder, window, cx));
        cx.emit(PanelEvent::PreferencesChanged);
    }

    /// The language to translate from: one that the user picked, or `None`
    /// to let Traduko read it off the text.
    fn choose_source(&mut self, language: Option<Language>, window: &mut Window, cx: &mut Context<Self>) {
        self.detect = language.is_none();
        self.direction_pinned = false;
        let direction = language.map_or(self.direction, |language| reading(language, self.direction));
        self.set_direction(direction, window, cx);
        self.translate_now(window, cx);
    }

    /// The language to translate to.
    fn choose_target(&mut self, language: Language, window: &mut Window, cx: &mut Context<Self>) {
        // To the language it is read in: the two change places.
        let direction = if language == self.direction.from { self.direction.swapped() } else { Direction::new(self.direction.from, language) };
        if direction != self.direction {
            self.set_direction(direction, window, cx);
            self.translate_now(window, cx);
        }
    }

    /// The languages of the menus, in the order of their own names.
    fn languages(&self) -> Vec<Language> {
        let mut languages = self.installed.languages();
        languages.sort_by_key(|language| language.native_name().to_lowercase());
        languages
    }

    /// Swaps the languages and the texts, like every translator does.
    fn swap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.result_text(cx);
        let finished = matches!(self.phase, Phase::Done { .. }) && !result.trim().is_empty();
        self.set_direction(self.direction.swapped(), window, cx);
        self.direction_pinned = true;
        // Without a finished translation there is nothing to bring up: only
        // the direction changes, and the text the user typed stays.
        if finished {
            self.source.update(cx, |state, cx| {
                state.set_value(result, window, cx);
                let end = state.text().len();
                state.set_selected_range(end..end, cx);
            });
        }
        self.result.update(cx, |state, cx| state.set_value("", window, cx));
        self.translate_now(window, cx);
    }

    fn set_english(&mut self, english: EnglishVariant, window: &mut Window, cx: &mut Context<Self>) {
        if self.english != english {
            self.english = english;
            cx.emit(PanelEvent::PreferencesChanged);
            self.translate_now(window, cx);
        }
    }

    fn set_quality(&mut self, quality: Quality, window: &mut Window, cx: &mut Context<Self>) {
        if self.quality != quality && self.installed.qualities().contains(&quality) {
            self.quality = quality;
            cx.emit(PanelEvent::PreferencesChanged);
            self.translate_now(window, cx);
        }
    }

    fn set_mascot_size(&mut self, size: MascotSize, cx: &mut Context<Self>) {
        if self.mascot_size != size {
            self.mascot_size = size;
            cx.emit(PanelEvent::MascotSizeChanged(size));
            cx.notify();
        }
    }

    /// The colour is the theme's to keep: the panel only asks for another.
    fn set_accent(&mut self, accent: Accent, cx: &mut Context<Self>) {
        if theme::palette(cx).accent != accent {
            cx.emit(PanelEvent::AccentChanged(accent));
        }
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let text = self.result_text(cx);
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = true;
        cx.emit(PanelEvent::Copied);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1400)).await;
            this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn status(&self) -> (SharedString, StatusDot) {
        match &self.phase {
            Phase::Idle => ("Ready".into(), StatusDot::Quiet),
            Phase::Typing => ("Listening".into(), StatusDot::Busy),
            Phase::LoadingModel => ("Waking the model".into(), StatusDot::Busy),
            Phase::Translating { .. } => ("Translating".into(), StatusDot::Busy),
            Phase::Done { .. } => ("Done".into(), StatusDot::Ok),
            Phase::Failed(_) => ("Something went wrong".into(), StatusDot::Quiet),
        }
    }

    /// How far the line under the header is filled, 0 to 1, or `None` while
    /// there is nothing to measure yet.
    fn progress(&self) -> Option<f32> {
        match self.phase {
            Phase::Translating { done, total } if total > 0 => Some(done as f32 / total as f32),
            Phase::Translating { .. } | Phase::LoadingModel => None,
            // At rest the line is neutral: the dot already says "done".
            _ => Some(0.0),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum StatusDot {
    Quiet,
    Busy,
    Ok,
}

/// The invitation to type, in the language that is expected.
fn source_placeholder(language: Language) -> &'static str {
    match language.code() {
        "fr" => "Écrivez ou collez du texte…",
        "es" => "Escribe o pega un texto…",
        "de" => "Text eingeben oder einfügen…",
        _ => "Type or paste some text…",
    }
}

/// The direction once a text is known to be in `language`: from it, to the
/// language that was asked for. A text that is already in that one goes the
/// other way, as when the two languages are swapped.
fn reading(language: Language, direction: Direction) -> Direction {
    if language == direction.to { direction.swapped() } else { Direction::new(language, direction.to) }
}

/// `wanted` when the models on disk can translate it, else a direction that
/// they can: French to English when it is there.
fn servable(installed: &Installed, wanted: Direction) -> Direction {
    if installed.translates(wanted) {
        return wanted;
    }
    let languages = installed.languages();
    let all = languages.iter().flat_map(|from| languages.iter().map(move |to| Direction::new(*from, *to)));
    let usual = Direction::new(Language::FRENCH, Language::ENGLISH);
    std::iter::once(usual).chain(all).find(|direction| installed.translates(*direction)).unwrap_or(wanted)
}

/// The name of a language over its card, as a menu of the others.
fn language_menu(
    id: &'static str,
    label: String,
    color: Hsla,
    items: impl Fn(PopupMenu) -> PopupMenu + 'static,
) -> impl IntoElement {
    Button::new(id).ghost().small().child(micro(label.to_uppercase(), color)).dropdown_caret(true).dropdown_menu(move |menu, _, _| items(menu.min_w(px(200.))))
}

/// A line of a menu of languages, which does `chosen` to the panel.
fn language_item(
    label: impl Into<SharedString>,
    checked: bool,
    panel: &WeakEntity<Panel>,
    chosen: impl Fn(&mut Panel, &mut Window, &mut Context<Panel>) + 'static,
) -> PopupMenuItem {
    let panel = panel.clone();
    PopupMenuItem::new(label).checked(checked).on_click(move |_, window, cx| {
        panel.update(cx, |panel, cx| chosen(panel, window, cx)).ok();
    })
}

/// A line of the menu of colours: the colour itself, then its name.
fn accent_item(accent: Accent, checked: bool, panel: &WeakEntity<Panel>) -> PopupMenuItem {
    let panel = panel.clone();
    PopupMenuItem::element(move |_, _| {
        div().flex().flex_1().items_center().gap(px(8.)).child(div().size(px(10.)).rounded_full().bg(accent.swatch())).child(accent.name())
    })
    .checked(checked)
    .on_click(move |_, _, cx| {
        panel.update(cx, |panel, cx| panel.set_accent(accent, cx)).ok();
    })
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let (status, dot) = self.status();
        let Direction { from, to } = self.direction;
        let has_result = self.result.read(cx).text().len() > 0;
        let words = self.source_text(cx).split_whitespace().count();

        // ---- header: grip, name, status -------------------------------------
        let dot_color = match dot {
            StatusDot::Quiet => p.muted,
            StatusDot::Busy => p.primary,
            StatusDot::Ok => p.ok,
        };
        let header = div()
            .id("header")
            .flex()
            .items_center()
            .justify_between()
            .h(px(46.))
            .px(px(18.))
            // The panel has no title bar: the header moves it.
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
                    .gap(px(9.))
                    .child(div().text_size(px(13.5)).text_color(p.ink.opacity(0.72)).child(status))
                    .child(div().size(px(8.)).rounded_full().bg(dot_color)),
            );

        // ---- the line under the header ----------------------------------------
        let track = div().mx(px(18.)).mb(px(8.)).h(px(2.)).rounded_full().bg(p.line).overflow_hidden();
        let line = match self.progress() {
            Some(fraction) => track.child(div().h_full().rounded_full().bg(p.primary).w(relative(fraction.clamp(0.0, 1.0)))),
            // Nothing to measure: a short segment that travels.
            None => track.child(
                div().h_full().w(relative(0.3)).rounded_full().bg(p.primary).with_animation(
                    "travel",
                    Animation::new(Duration::from_millis(1100)).repeat().with_easing(ease_in_out),
                    |segment, t| segment.ml(relative(t * 0.7)),
                ),
            ),
        };

        // ---- the two languages, each a menu ------------------------------------------
        let (languages, detect, panel) = (self.languages(), self.detect, cx.entity().downgrade());
        let colors = panel.clone();
        let more = |menu: PopupMenu, panel: &WeakEntity<Self>| {
            menu.separator().item(language_item("More languages…", false, panel, |_, _, cx| cx.emit(PanelEvent::LanguagesRequested)))
        };
        let source_label = if detect { format!("{} · auto", from.native_name()) } else { from.native_name().to_string() };
        let source_language = language_menu("source-language", source_label, p.muted, {
            let (languages, panel) = (languages.clone(), panel.clone());
            move |menu| {
                let menu = menu.item(language_item("Detect language", detect, &panel, |panel, window, cx| panel.choose_source(None, window, cx))).separator();
                let menu = languages.iter().fold(menu, |menu, language| {
                    let language = *language;
                    let checked = !detect && language == from;
                    menu.item(language_item(language.native_name().to_string(), checked, &panel, move |panel, window, cx| {
                        panel.choose_source(Some(language), window, cx)
                    }))
                });
                more(menu, &panel)
            }
        });
        let target_label = match (to == Language::ENGLISH, self.english) {
            (true, EnglishVariant::American) => "English · US".to_string(),
            (true, EnglishVariant::British) => "English · UK".to_string(),
            (false, _) => to.native_name().to_string(),
        };
        let target_language = language_menu("target-language", target_label, p.primary, move |menu| {
            let menu = languages.iter().fold(menu, |menu, language| {
                let language = *language;
                menu.item(language_item(language.native_name().to_string(), language == to, &panel, move |panel, window, cx| {
                    panel.choose_target(language, window, cx)
                }))
            });
            more(menu, &panel)
        });

        // ---- source card -----------------------------------------------------------
        let swap_hover = p.chip_hover;
        let source = card(&p)
            .flex()
            .flex_col()
            .h(px(176.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl(px(10.))
                    .pr(px(12.))
                    .pt(px(12.))
                    .child(source_language)
                    .child(
                        div()
                            .id("swap")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(28.))
                            .rounded_full()
                            .bg(p.chip)
                            .text_color(p.ink)
                            .cursor_pointer()
                            .hover(move |style| style.bg(swap_hover))
                            .on_click(cx.listener(|this, _, window, cx| this.swap(window, cx)))
                            .child(Icon::new(IconName::ArrowUpDown).size(px(14.))),
                    ),
            )
            .child(div().flex_1().min_h_0().px(px(8.)).pb(px(8.)).child(Textarea::new(&self.source).appearance(false).h_full()));

        // ---- result card --------------------------------------------------------------
        let (headline, detail): (&str, SharedString) = match &self.phase {
            _ if self.installed.is_empty() => ("I have no model yet", "Close me and click me again to download one".into()),
            Phase::Failed(message) => ("I could not translate that", message.clone()),
            Phase::LoadingModel => ("One moment, I am waking up", "The model loads once, then stays warm".into()),
            Phase::Translating { .. } => ("On it", "Reading your text".into()),
            _ => ("Ready when you are", "Type above, I translate as you go".into()),
        };
        let face = Frame { color: p.accent.body(), ..self.face.clone() };
        let empty = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .px(px(18.))
            .h_full()
            .child(
                div()
                    .flex_none()
                    .size(px(46.))
                    .rounded(px(15.))
                    .bg(p.chip)
                    .child(canvas(|_, _, _| (), move |bounds, _, window, _| mascot_view::paint(&face, bounds, window)).size_full()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(div().text_size(px(15.5)).font_weight(FontWeight::MEDIUM).text_color(p.ink).child(headline))
                    .child(div().text_size(px(13.5)).text_color(p.muted).child(detail)),
            );
        // The language stays over the card when it is empty: it is where
        // the user says what to translate to, before typing.
        let result = card(&p)
            .flex()
            .flex_col()
            .h(px(176.))
            .child(div().flex_none().pl(px(10.)).pt(px(12.)).child(div().flex().items_center().h(px(28.)).child(target_language)))
            .when(has_result, |card| {
                card.child(div().flex_1().min_h_0().px(px(8.)).pb(px(8.)).child(Textarea::new(&self.result).readonly(true).appearance(false).h_full()))
            })
            // As much room under the words as the language takes above them.
            .when(!has_result, |card| card.child(div().flex_1().min_h_0().pb(px(40.)).child(empty)));

        // ---- footer card: English variant, options, numbers, actions -----------------------
        let english_tab = |label: &'static str, variant: EnglishVariant, this: &Self| {
            let active = this.english == variant;
            div()
                .id(label)
                .flex()
                .flex_col()
                .gap(px(3.))
                .pt(px(5.))
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .text_color(if active { p.ink } else { p.muted })
                .hover(|style| style.text_color(p.ink))
                .on_click(cx.listener(move |this, _, window, cx| this.set_english(variant, window, cx)))
                .child(label)
                // The mark sits under the label and keeps its place when it
                // is off, so nothing moves when the choice changes.
                .child(div().h(px(2.)).rounded_full().bg(if active { p.primary } else { gpui::transparent_black() }))
        };

        // British or American is a question for English only. Another
        // language has nothing to choose here, and one that is reached
        // through English says so: two translations, twice the mistakes.
        let footer_left = if to == Language::ENGLISH {
            div()
                .flex()
                .items_center()
                .gap(px(18.))
                .child(english_tab("American", EnglishVariant::American, self))
                .child(english_tab("British", EnglishVariant::British, self))
        } else if self.installed.goes_through_english(self.direction) {
            div().text_size(px(13.5)).text_color(p.muted).child("Translated through English")
        } else {
            div()
        };

        let (quality, installed, open_at_login) = (self.quality, self.installed.qualities(), self.open_at_login);
        let (mascot_size, accent) = (self.mascot_size, p.accent);
        let focus = self.focus.clone();
        let options = Button::new("options")
            .ghost()
            .small()
            .label(match quality {
                Quality::Light => "Light",
                Quality::Accurate => "Accurate",
            })
            .dropdown_caret(true)
            // Upwards and right-aligned, so that it stays inside the shell.
            .dropdown_menu_with_anchor(gpui::Anchor::BottomRight, move |menu, window, cx| {
                let mut menu = menu.action_context(focus.clone()).min_w(px(220.)).label("Model");
                if installed.contains(&Quality::Light) {
                    menu = menu.menu_with_check("Light · opus-mt", quality == Quality::Light, Box::new(UseLight));
                }
                if installed.contains(&Quality::Accurate) {
                    menu = menu.menu_with_check("Accurate · opus-mt-tc-big", quality == Quality::Accurate, Box::new(UseAccurate));
                }
                // The colours are a menu of their own: listed here, they
                // would push this one out of the top of the shell.
                let colors = colors.clone();
                menu.menu("Download models…", Box::new(ShowModels))
                    .separator()
                    .label("Mascot size")
                    .menu_with_check("Small", mascot_size == MascotSize::Small, Box::new(MascotSmall))
                    .menu_with_check("Medium", mascot_size == MascotSize::Medium, Box::new(MascotMedium))
                    .menu_with_check("Large", mascot_size == MascotSize::Large, Box::new(MascotLarge))
                    .separator()
                    .submenu("Color", window, cx, move |menu, _, _| {
                        Accent::ALL.into_iter().fold(menu, |menu, choice| menu.item(accent_item(choice, choice == accent, &colors)))
                    })
                    .menu_with_check("Open at login", open_at_login, Box::new(ToggleOpenAtLogin))
                    .separator()
                    .menu("Quit Traduko", Box::new(QuitTraduko))
            });

        let unit = if words == 1 { "word" } else { "words" };
        let numbers = match &self.phase {
            Phase::Done { elapsed } => format!("{words} {unit}   {} ms", elapsed.as_millis()),
            Phase::Translating { done, total } if *total > 0 => format!("{words} {unit}   {done} / {total}"),
            _ => format!("{words} {unit}"),
        };

        let footer = card(&p)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(46.))
                    .pl(px(18.))
                    .pr(px(10.))
                    .child(footer_left)
                    .child(options),
            )
            .child(div().h(px(1.)).bg(p.line.opacity(0.6)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(56.))
                    .pl(px(18.))
                    .pr(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().size(px(6.)).rounded_full().bg(dot_color))
                            .child(div().font_family(MONO).text_size(px(12.5)).text_color(p.muted).child(numbers)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                chip("copy", &p)
                                    .px(px(14.))
                                    .when(!has_result, |chip| chip.opacity(0.45))
                                    .on_click(cx.listener(|this, _, _, cx| this.copy(cx)))
                                    .child(Icon::new(if self.copied { IconName::Check } else { IconName::Copy }).size(px(14.)))
                                    .child(if self.copied { "Copied" } else { "Copy" }),
                            )
                            .child(
                                chip("close", &p)
                                    .w(px(32.))
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(PanelEvent::HideRequested)))
                                    .child(Icon::new(IconName::X).size(px(14.))),
                            ),
                    ),
            );

        let shell = div()
            .id("traduko-panel")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Translate, window, cx| this.translate_now(window, cx)))
            .on_action(cx.listener(|_, _: &HidePanel, _, cx| cx.emit(PanelEvent::HideRequested)))
            .on_action(cx.listener(|this, _: &SwapDirection, window, cx| this.swap(window, cx)))
            .on_action(cx.listener(|this, _: &CopyResult, _, cx| this.copy(cx)))
            .on_action(cx.listener(|this, _: &UseLight, window, cx| this.set_quality(Quality::Light, window, cx)))
            .on_action(cx.listener(|this, _: &UseAccurate, window, cx| this.set_quality(Quality::Accurate, window, cx)))
            .on_action(cx.listener(|this, _: &MascotSmall, _, cx| this.set_mascot_size(MascotSize::Small, cx)))
            .on_action(cx.listener(|this, _: &MascotMedium, _, cx| this.set_mascot_size(MascotSize::Medium, cx)))
            .on_action(cx.listener(|this, _: &MascotLarge, _, cx| this.set_mascot_size(MascotSize::Large, cx)))
            .on_action(cx.listener(|this, _: &ToggleOpenAtLogin, _, cx| {
                this.open_at_login = !this.open_at_login;
                cx.emit(PanelEvent::OpenAtLoginChanged(this.open_at_login));
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &ShowModels, _, cx| cx.emit(PanelEvent::ModelsRequested)))
            .on_action(cx.listener(|_, _: &QuitTraduko, _, cx| cx.emit(PanelEvent::QuitRequested)))
            .w(px(SHELL_WIDTH))
            .h(px(SHELL_HEIGHT))
            .flex()
            .flex_col()
            .p(px(SHELL_PAD))
            .bg(p.shell)
            .rounded(px(SHELL_RADIUS))
            .font_family(theme::SANS)
            .text_color(p.ink)
            .shadow(theme::shell_shadow(&p))
            .child(header)
            .child(line)
            .child(div().flex().flex_col().gap(px(8.)).child(source).child(result).child(footer));

        div().size_full().flex().items_center().justify_center().child(shell)
    }
}
