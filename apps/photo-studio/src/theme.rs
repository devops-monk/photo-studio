//! Apple-inspired light and dark themes: soft greys, system-blue accent, generous radii.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Visuals,
};
use studio_core::text::FontFace;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

/// Colours the custom widgets read; mirrors the active egui visuals.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub window: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub raised: Color32,
    pub canvas: Color32,
    pub border: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_text: Color32,
    pub danger: Color32,
    pub success: Color32,
    pub hover: Color32,
}

pub const RADIUS: u8 = 8;
pub const RADIUS_LG: u8 = 12;

impl Tokens {
    pub fn dark() -> Self {
        Tokens {
            dark: true,
            window: Color32::from_rgb(30, 30, 32),
            panel: Color32::from_rgb(38, 38, 41),
            card: Color32::from_rgb(46, 46, 50),
            raised: Color32::from_rgb(58, 58, 62),
            canvas: Color32::from_rgb(24, 24, 26),
            border: Color32::from_rgb(64, 64, 70),
            separator: Color32::from_rgb(52, 52, 56),
            text: Color32::from_rgb(242, 242, 247),
            text_dim: Color32::from_rgb(174, 174, 178),
            text_faint: Color32::from_rgb(110, 110, 115),
            accent: Color32::from_rgb(10, 132, 255),
            accent_soft: Color32::from_rgba_unmultiplied(10, 132, 255, 46),
            accent_text: Color32::WHITE,
            danger: Color32::from_rgb(255, 69, 58),
            success: Color32::from_rgb(48, 209, 88),
            hover: Color32::from_rgba_unmultiplied(255, 255, 255, 14),
        }
    }

    pub fn light() -> Self {
        Tokens {
            dark: false,
            window: Color32::from_rgb(236, 236, 238),
            panel: Color32::from_rgb(246, 246, 248),
            card: Color32::from_rgb(255, 255, 255),
            raised: Color32::from_rgb(232, 232, 236),
            canvas: Color32::from_rgb(214, 214, 218),
            border: Color32::from_rgb(208, 208, 213),
            separator: Color32::from_rgb(222, 222, 226),
            text: Color32::from_rgb(29, 29, 31),
            text_dim: Color32::from_rgb(99, 99, 102),
            text_faint: Color32::from_rgb(160, 160, 165),
            accent: Color32::from_rgb(0, 122, 255),
            accent_soft: Color32::from_rgba_unmultiplied(0, 122, 255, 36),
            accent_text: Color32::WHITE,
            danger: Color32::from_rgb(255, 59, 48),
            success: Color32::from_rgb(52, 199, 89),
            hover: Color32::from_rgba_unmultiplied(0, 0, 0, 12),
        }
    }

    pub fn get(ctx: &egui::Context) -> Tokens {
        if ctx.global_style().visuals.dark_mode { Tokens::dark() } else { Tokens::light() }
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), std::sync::Arc::new(FontData::from_static(bytes)));
    };
    add(&mut fonts, "Inter", FontFace::Sans.bytes());
    add(&mut fonts, "InterMedium", FontFace::SansMedium.bytes());
    add(&mut fonts, "InterBold", FontFace::SansBold.bytes());
    add(&mut fonts, "Mono", FontFace::Mono.bytes());
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "Inter".to_owned());
    // Icons second, right after the text font.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    mono.insert(0, "Mono".to_owned());
    for (fam, face) in [("medium", "InterMedium"), ("bold", "InterBold")] {
        let list = vec![face.to_owned(), "phosphor".to_owned(), "Inter".to_owned()];
        fonts.families.insert(FontFamily::Name(fam.into()), list);
    }
    ctx.set_fonts(fonts);
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

fn visuals(t: &Tokens) -> Visuals {
    let mut v = if t.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.card;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.window_corner_radius = CornerRadius::same(RADIUS_LG);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(if t.dark { 120 } else { 50 }),
    };
    v.popup_shadow =
        Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(if t.dark { 100 } else { 40 }) };
    v.extreme_bg_color = if t.dark { Color32::from_rgb(28, 28, 30) } else { Color32::WHITE };
    v.faint_bg_color = t.card;
    v.hyperlink_color = t.accent;
    v.selection.bg_fill = t.accent;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.slider_trailing_fill = true;
    v.window_highlight_topmost = false;

    let r = CornerRadius::same(5);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.panel;
    w.noninteractive.weak_bg_fill = t.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = t.raised;
    w.inactive.weak_bg_fill = t.raised;
    w.inactive.bg_stroke = Stroke::new(1.0, t.border);
    w.inactive.fg_stroke = Stroke::new(1.0, t.text);
    w.inactive.corner_radius = r;

    w.hovered.bg_fill = if t.dark { Color32::from_rgb(70, 70, 76) } else { Color32::from_rgb(222, 222, 228) };
    w.hovered.weak_bg_fill = w.hovered.bg_fill;
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.5, t.text);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = t.accent;
    w.active.weak_bg_fill = t.accent;
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open.bg_fill = t.raised;
    w.open.weak_bg_fill = t.raised;
    w.open.corner_radius = r;
    v
}

/// Install fonts-independent styling for both light and dark, then pick the mode.
pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    for (theme, tokens) in [(egui::Theme::Dark, Tokens::dark()), (egui::Theme::Light, Tokens::light())] {
        ctx.set_visuals_of(theme, visuals(&tokens));
    }
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Heading, medium(17.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size = egui::vec2(28.0, 24.0);
        s.spacing.window_margin = Margin::same(16);
        s.spacing.menu_margin = Margin::same(6);
        s.spacing.slider_width = 150.0;
        s.spacing.slider_rail_height = 4.0;
        s.spacing.combo_width = 120.0;
        s.spacing.scroll.bar_width = 6.0;
        s.spacing.scroll.floating = true;
        s.interaction.tooltip_delay = 0.35;
        s.visuals.indent_has_left_vline = false;
    });
    ctx.set_theme(match mode {
        ThemeMode::System => egui::ThemePreference::System,
        ThemeMode::Dark => egui::ThemePreference::Dark,
        ThemeMode::Light => egui::ThemePreference::Light,
    });
}
