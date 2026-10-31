//! The window's look: which appearance to draw (the system's, or light or
//! dark pinned), which accent, and code's inks.
//!
//! The palette stays in `themes/onyx.json`, one config per appearance.
//! An accent is a substitution over the keys that carry it there (the
//! primary button, the focus ring, links, the selection), painted into
//! both configs before they are handed to the toolkit, so `Theme::change`
//! keeps the chosen accent through every appearance switch with nothing
//! to patch afterwards.

use gpui::{App, Window};
use gpui_component::theme::{Theme, ThemeConfig, ThemeMode, ThemeSet};

use crate::i18n::phrase;

/// One accent: its key in the settings, its name (drawn through `t`),
/// and its base, hover and active shades in each appearance, plus the
/// link's shade in each.
pub struct Accent {
    pub key: &'static str,
    pub name: &'static str,
    pub light: [&'static str; 3],
    pub dark: [&'static str; 3],
    pub link: [&'static str; 2],
}

/// Seven hues of one lightness and chroma in OKLCH: 0.52 and 0.13 in the
/// light appearance, 0.74 and 0.105 in the dark, the chroma eased for the
/// hues that are loud at it and amber's light shade a little lighter,
/// where an orange at 0.52 is brown. They go round the wheel in even
/// steps and leave out two places: the red-orange around hue 40, and the
/// blues between teal and periwinkle.
pub const ACCENTS: &[Accent] = &[
    Accent { key: "periwinkle", name: phrase("Periwinkle"), light: ["#5061B3", "#4A5BAC", "#4050A0"], dark: ["#94A7ED", "#A0B3FB", "#879AE0"], link: ["#4A5BAC", "#94A7ED"] },
    Accent { key: "orchid", name: phrase("Orchid"), light: ["#845293", "#7E4C8D", "#724181"], dark: ["#C498D2", "#D1A4DF", "#B78BC4"], link: ["#7E4C8D", "#C498D2"] },
    Accent { key: "rose", name: phrase("Rose"), light: ["#9C4A66", "#954360", "#893856"], dark: ["#DA93A9", "#E79FB5", "#CC879C"], link: ["#954360", "#DA93A9"] },
    Accent { key: "amber", name: phrase("Amber"), light: ["#A86003", "#A05B00", "#905203"], dark: ["#DD9B5F", "#EAA76B", "#CF8E52"], link: ["#A05B00", "#DD9B5F"] },
    Accent { key: "moss", name: phrase("Moss"), light: ["#5A723A", "#556C34", "#4A6129"], dark: ["#9CB67C", "#A7C288", "#8FA86F"], link: ["#556C34", "#9CB67C"] },
    Accent { key: "jade", name: phrase("Jade"), light: ["#327860", "#2B725A", "#1E6750"], dark: ["#77BCA1", "#83C8AD", "#6AAE95"], link: ["#2B725A", "#77BCA1"] },
    Accent { key: "teal", name: phrase("Teal"), light: ["#277580", "#1F6F79", "#0E646E"], dark: ["#6FB9C4", "#7BC5D0", "#61ACB7"], link: ["#1F6F79", "#6FB9C4"] },
];

/// What is drawn on an accent: white on the light appearance's shade,
/// the ink on the dark one's, each 4.8 to 1 or more on every accent.
const ON_LIGHT: &str = "#FFFFFF";
const ON_DARK: &str = "#1F1E1D";

/// The accent by its key; an unknown key is the first.
pub fn accent(key: &str) -> &'static Accent {
    ACCENTS.iter().find(|a| a.key == key).unwrap_or(&ACCENTS[0])
}

/// The accent's base in the appearance showing, for a swatch.
pub fn accent_hex(key: &str, dark: bool) -> &'static str {
    let a = accent(key);
    if dark { a.dark[0] } else { a.light[0] }
}

/// The accent as the frame wears it: the dark appearance's shade in both,
/// since the light one is 2 to 1 on the frame's graphite.
pub fn on_frame(key: &str) -> (&'static str, &'static str) {
    (accent(key).dark[0], ON_DARK)
}

/// The inks code is written in, one set an appearance: everything that
/// colors code or a terminal takes them from here, by the names the theme
/// file gives them ("@l-blue", "@d-rose"). The hues are Rosé Pine's (pine,
/// iris, gold, rose, foam, love) with a sage green beside them, each
/// taken darker or lighter until it reads at 4.5 to 1 or more on the
/// window's page, the gray for comments a step quieter.
pub struct Inks {
    pub blue: u32,
    pub violet: u32,
    pub gold: u32,
    pub rose: u32,
    pub teal: u32,
    pub green: u32,
    pub red: u32,
    pub gray: u32,
}

pub const INKS_LIGHT: Inks = Inks { blue: 0x286983, violet: 0x7A6398, gold: 0x8A6A1E, rose: 0xA3566F, teal: 0x357783, green: 0x4C7B58, red: 0xB0505F, gray: 0x85818F };
pub const INKS_DARK: Inks = Inks { blue: 0x7FB4D0, violet: 0xC4A7E7, gold: 0xE9C083, rose: 0xE8B0AE, teal: 0x9CCFD8, green: 0xA3CCA4, red: 0xEB8FA6, gray: 0x8F8BA0 };

/// The theme file with each ink's name replaced by its color.
fn inked(json: &str) -> String {
    let mut out = json.to_string();
    for (p, inks) in [("l", &INKS_LIGHT), ("d", &INKS_DARK)] {
        for (name, c) in [("blue", inks.blue), ("violet", inks.violet), ("gold", inks.gold), ("rose", inks.rose), ("teal", inks.teal), ("green", inks.green), ("red", inks.red), ("gray", inks.gray)] {
            out = out.replace(&format!("\"@{p}-{name}\""), &format!("\"#{c:06X}\""));
        }
    }
    out
}

fn paint(cfg: &mut ThemeConfig, a: &Accent) {
    let dark = cfg.mode.is_dark();
    let [base, hover, active] = if dark { a.dark } else { a.light };
    let link = if dark { a.link[1] } else { a.link[0] };
    let on = if dark { ON_DARK } else { ON_LIGHT };
    let c = &mut cfg.colors;
    c.primary = Some(base.into());
    c.primary_foreground = Some(on.into());
    c.sidebar_primary_foreground = Some(on.into());
    c.primary_hover = Some(hover.into());
    c.primary_active = Some(active.into());
    c.ring = Some(base.into());
    c.link = Some(link.into());
    c.link_hover = Some(active.into());
    c.link_active = Some(active.into());
    c.sidebar_primary = Some(base.into());
    c.list_active_border = Some(base.into());
    c.drag_border = Some(base.into());
    c.progress_bar = Some(base.into());
    c.slider_bar = Some(base.into());
    c.drop_target = Some(format!("{base}33").into());
    c.selection = Some(format!("{base}{}", if dark { "66" } else { "59" }).into());
}

/// Hand the toolkit both palettes with the accent painted in. Called at
/// start and again whenever the accent changes; `apply` then draws it.
pub fn install(accent_key: &str, cx: &mut App) {
    let set: ThemeSet = match serde_json::from_str(&inked(include_str!("../themes/onyx.json"))) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("onyx: the theme file is invalid, using the default look: {e}");
            return;
        }
    };
    let a = accent(accent_key);
    let theme = Theme::global_mut(cx);
    for mut cfg in set.themes {
        paint(&mut cfg, a);
        let cfg = std::rc::Rc::new(cfg);
        if cfg.mode.is_dark() {
            theme.dark_theme = cfg;
        } else {
            theme.light_theme = cfg;
        }
    }
}

/// Draw the appearance asked for: the system's for `system`, else the
/// one named. Called at start, at every change of the system's
/// appearance, and from the settings. Never call the toolkit's
/// `sync_system_appearance` elsewhere: a pinned appearance would flip
/// with the system.
pub fn apply(appearance: &str, window: Option<&mut Window>, cx: &mut App) {
    match appearance {
        "light" => Theme::change(ThemeMode::Light, window, cx),
        "dark" => Theme::change(ThemeMode::Dark, window, cx),
        _ => Theme::sync_system_appearance(window, cx),
    }
    // The toolkit draws again only the window it is handed; every window
    // wears the palette.
    cx.refresh_windows();
}
