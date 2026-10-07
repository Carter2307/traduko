//! Colours, fonts and sizes of Traduko's windows, for light and dark, and the
//! pieces both the panel and the onboarding are made of.

use std::borrow::Cow;

use gpui::{
    App, BoxShadow, Div, ElementId, FontWeight, Global, Hsla, SharedString, Stateful, WindowAppearance, div, point,
    prelude::*, px, rgb, rgba,
};
use gpui_component::{Theme, ThemeMode};

pub const SANS: &str = "Geist";
pub const MONO: &str = "Geist Mono";

/// Corner radius of the shell, and of the cards inside it. The cards sit
/// `SHELL_PAD` inside the shell, so their corners follow the same curve.
pub const SHELL_RADIUS: f32 = 34.0;
pub const SHELL_PAD: f32 = 10.0;
pub const CARD_RADIUS: f32 = SHELL_RADIUS - SHELL_PAD;
/// Transparent room around a shell, in its window, for the shadow.
pub const SHELL_MARGIN: f32 = 36.0;

#[derive(Clone, Copy)]
pub struct Palette {
    pub shell: Hsla,
    pub card: Hsla,
    pub ink: Hsla,
    pub muted: Hsla,
    pub line: Hsla,
    pub chip: Hsla,
    pub chip_hover: Hsla,
    pub primary: Hsla,
    pub ok: Hsla,
    pub shadow: Hsla,
}

impl Global for Palette {}

impl Palette {
    fn light() -> Self {
        Self {
            shell: rgb(0xf2f2f2).into(),
            card: rgb(0xffffff).into(),
            ink: rgb(0x121214).into(),
            // Dark enough for small text on the white cards (4.9 to 1).
            muted: rgb(0x707075).into(),
            line: rgb(0xe4e4e6).into(),
            chip: rgb(0xf2f2f3).into(),
            chip_hover: rgb(0xe8e8ea).into(),
            primary: rgb(0xf45a1c).into(),
            ok: rgb(0x1fb46a).into(),
            shadow: rgba(0x00000026).into(),
        }
    }

    fn dark() -> Self {
        Self {
            shell: rgb(0x1c1c1e).into(),
            card: rgb(0x28282a).into(),
            ink: rgb(0xf4f4f5).into(),
            muted: rgb(0x8e8e93).into(),
            line: rgb(0x38383b).into(),
            chip: rgb(0x343437).into(),
            chip_hover: rgb(0x404044).into(),
            primary: rgb(0xff6a2b).into(),
            ok: rgb(0x34c77b).into(),
            shadow: rgba(0x00000066).into(),
        }
    }
}

pub fn palette(cx: &App) -> Palette {
    *cx.global::<Palette>()
}

/// Small capitals in the monospace face: the labels and the numbers.
pub fn micro(text: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .font_family(MONO)
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(10.5))
        .text_color(color)
        .child(text.into())
}

pub fn card(p: &Palette) -> Div {
    div().bg(p.card).rounded(px(CARD_RADIUS)).overflow_hidden()
}

/// A round control on the chip colour.
pub fn chip(id: impl Into<ElementId>, p: &Palette) -> Stateful<Div> {
    let hover = p.chip_hover;
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(32.))
        .rounded_full()
        .bg(p.chip)
        .text_color(p.ink)
        .text_size(px(13.5))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
}

/// The four bars at the left of a header: a window without a title bar is
/// moved by its header.
pub fn grip(p: &Palette) -> Div {
    div().flex().gap(px(3.)).children((0..4).map(|_| div().w(px(3.)).h(px(9.)).rounded(px(1.)).bg(p.muted.opacity(0.7))))
}

/// The shadow of a shell. Kept short of `SHELL_MARGIN`, or it ends on a
/// straight line at the edge of the window.
pub fn shell_shadow(p: &Palette) -> Vec<BoxShadow> {
    vec![
        BoxShadow { color: p.shadow, offset: point(px(0.), px(12.)), blur_radius: px(20.), spread_radius: px(-6.), inset: false },
        BoxShadow { color: p.shadow.opacity(0.08), offset: point(px(0.), px(1.)), blur_radius: px(3.), spread_radius: px(0.), inset: false },
    ]
}

pub fn register_fonts(cx: &mut App) {
    let fonts: [&'static [u8]; 5] = [
        include_bytes!("../../../assets/fonts/Geist-Regular.otf"),
        include_bytes!("../../../assets/fonts/Geist-Medium.otf"),
        include_bytes!("../../../assets/fonts/Geist-SemiBold.otf"),
        include_bytes!("../../../assets/fonts/GeistMono-Regular.otf"),
        include_bytes!("../../../assets/fonts/GeistMono-Medium.otf"),
    ];
    cx.text_system()
        .add_fonts(fonts.into_iter().map(Cow::Borrowed).collect())
        .expect("register the embedded fonts");
}

/// Follows the system: call at start-up and whenever the appearance changes.
/// `TRADUKO_APPEARANCE=light|dark` forces one side, to capture both.
pub fn apply(appearance: WindowAppearance, cx: &mut App) {
    let mode = match std::env::var("TRADUKO_APPEARANCE").as_deref() {
        Ok("light") => ThemeMode::Light,
        Ok("dark") => ThemeMode::Dark,
        _ => ThemeMode::from(appearance),
    };
    let palette = if mode.is_dark() { Palette::dark() } else { Palette::light() };
    cx.set_global(palette);

    // The text areas come from the component library and read its theme.
    // `change` resets every colour, so ours are written again after it.
    Theme::change(mode, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = SANS.into();
        theme.font_size = px(15.);
        theme.radius = px(12.);
        theme.radius_lg = px(16.);
        theme.background = palette.card;
        theme.foreground = palette.ink;
        theme.muted_foreground = palette.muted;
        theme.border = palette.line;
        theme.input = palette.line;
        theme.ring = palette.primary;
        theme.caret = palette.primary;
        theme.selection = palette.primary.opacity(0.24);
        theme.primary = palette.primary;
        theme.primary_foreground = rgb(0xffffff).into();
        theme.accent = palette.chip;
        theme.accent_foreground = palette.ink;
        theme.secondary = palette.chip;
        theme.secondary_hover = palette.chip_hover;
        theme.secondary_active = palette.chip_hover;
        theme.secondary_foreground = palette.ink;
        theme.popover = palette.card;
        theme.popover_foreground = palette.ink;
    });
}
