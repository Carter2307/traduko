//! Colours, fonts and sizes of Traduko's windows, for light and dark, and the
//! pieces both the panel and the onboarding are made of.

use std::borrow::Cow;

use gpui::{
    App, Div, ElementId, FontWeight, Global, Hsla, MouseButton, SharedString, Stateful, WindowAppearance, div,
    prelude::*, px, rgb, rgba,
};
use gpui_component::{Icon, Theme, ThemeMode};
use gpui_kit_assets::IconName;

use crate::settings::Accent;

pub const SANS: &str = "Geist";
pub const MONO: &str = "Geist Mono";

/// Corner radius of the shell, and of the cards inside it. The cards sit
/// `SHELL_PAD` inside the shell, so their corners follow the same curve.
pub const SHELL_RADIUS: f32 = 34.0;
pub const SHELL_PAD: f32 = 10.0;
pub const CARD_RADIUS: f32 = SHELL_RADIUS - SHELL_PAD;
/// Transparent room around a shell, in its window.
pub const SHELL_MARGIN: f32 = 36.0;

#[derive(Clone, Copy)]
pub struct Palette {
    /// The shell is the glass of macOS (see `native::glass`), and the cards
    /// let it show, like the chips and the lines on them. gpui adds up the
    /// opacity of what it draws on a transparent window: anything else that
    /// is see-through under a card would make the card opaque.
    pub card: Hsla,
    /// A menu opens over text: nothing shows through it.
    pub menu: Hsla,
    pub ink: Hsla,
    pub muted: Hsla,
    pub line: Hsla,
    pub chip: Hsla,
    pub chip_hover: Hsla,
    /// The colour the user chose, and `primary` is what it gives here.
    pub accent: Accent,
    pub primary: Hsla,
    pub ok: Hsla,
}

impl Global for Palette {}

impl Accent {
    /// On the light cards, then on the dark ones, where it is lighter to
    /// stand out as much. All of them carry white text, and none is the
    /// green of `ok` or the gray of a Traduko that is sorry.
    fn hex(self) -> (u32, u32) {
        match self {
            Accent::Orange => (0xf45a1c, 0xff6a2b),
            Accent::Pink => (0xe8408a, 0xff5fa2),
            Accent::Violet => (0x7b52ee, 0x9d7dff),
            Accent::Blue => (0x2b7bf0, 0x4f97ff),
            Accent::Teal => (0x0b959b, 0x1aa9b0),
        }
    }

    /// The body of the mascot. The same on light and dark: Traduko is on the
    /// desktop, whatever the windows look like.
    pub fn body(self) -> traduko_blob::Rgb {
        let hex = self.hex().0;
        [16, 8, 0].map(|shift| ((hex >> shift) & 0xff) as f32 / 255.0)
    }

    /// As a swatch, in a menu of either appearance.
    pub fn swatch(self) -> Hsla {
        rgb(self.hex().0).into()
    }
}

impl Palette {
    fn light(accent: Accent) -> Self {
        Self {
            card: rgba(0xffffffa8).into(),
            menu: rgb(0xffffff).into(),
            ink: rgb(0x121214).into(),
            // Dark enough for small text on the white cards (4.9 to 1).
            muted: rgb(0x707075).into(),
            line: rgba(0x00000014).into(),
            chip: rgba(0x0000000f).into(),
            chip_hover: rgba(0x0000001c).into(),
            accent,
            primary: rgb(accent.hex().0).into(),
            ok: rgb(0x1fb46a).into(),
        }
    }

    fn dark(accent: Accent) -> Self {
        Self {
            card: rgba(0x242427a0).into(),
            menu: rgb(0x28282a).into(),
            ink: rgb(0xf4f4f5).into(),
            muted: rgb(0xa0a0a6).into(),
            line: rgba(0xffffff1a).into(),
            chip: rgba(0xffffff1a).into(),
            chip_hover: rgba(0xffffff2b).into(),
            accent,
            primary: rgb(accent.hex().1).into(),
            ok: rgb(0x34c77b).into(),
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

/// The cross at the right of a header, which closes its window. On the
/// shell, where a chip would not show: the colour of a card.
pub fn close(p: &Palette) -> Stateful<Div> {
    let hover = p.chip_hover;
    div()
        .id("close")
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded_full()
        .bg(p.card)
        .text_color(p.ink)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        // A press here is not a grab of the header.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(Icon::new(IconName::X).size(px(14.)))
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
    // The colour is the user's, not the system's: it stays.
    let accent = cx.try_global::<Palette>().map(|palette| palette.accent).unwrap_or_default();
    apply_with(appearance, accent, cx);
}

/// Changes the colour that stands out, in every window that is open.
pub fn set_accent(accent: Accent, cx: &mut App) {
    apply_with(cx.window_appearance(), accent, cx);
    cx.refresh_windows();
}

fn apply_with(appearance: WindowAppearance, accent: Accent, cx: &mut App) {
    let forced = match std::env::var("TRADUKO_APPEARANCE").as_deref() {
        Ok("light") => Some(ThemeMode::Light),
        Ok("dark") => Some(ThemeMode::Dark),
        _ => None,
    };
    // The glass is the system's: it has to be told as well.
    if let Some(mode) = forced {
        crate::native::set_appearance(mode.is_dark());
    }
    let mode = forced.unwrap_or_else(|| ThemeMode::from(appearance));
    let palette = if mode.is_dark() { Palette::dark(accent) } else { Palette::light(accent) };
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
        theme.popover = palette.menu;
        theme.popover_foreground = palette.ink;
    });
}
