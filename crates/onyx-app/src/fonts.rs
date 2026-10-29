//! The faces the window carries, and which face each place uses.
//!
//! The chrome is the system's face and is never set here. A note's words
//! are set in a face of the person's choice (`TEXT_FACES`), code in a
//! monospace face of theirs (`MONO_FACES`), and the app's name (the
//! sidebar's wordmark, About's title card) in Optima where the system
//! has it, else Belleza, a free face with the same flared strokes,
//! carried for Windows and Linux. Each carried face is a static file a
//! weight under the SIL Open Font License, with that license beside the
//! files in `assets/fonts/`: handed a variable font, gpui holds one face
//! of it and draws every bold as regular.

use std::borrow::Cow;

use gpui::{App, SharedString};

use crate::i18n::phrase;

/// A face a person may choose: its key in the settings, the name drawn
/// on its tile (through `t`), and the family name the text system knows.
pub struct Face {
    pub key: &'static str,
    pub name: &'static str,
    pub family: &'static str,
}

/// The faces a note's words may be set in. The first is a new install's.
pub const TEXT_FACES: &[Face] = &[
    Face { key: "inter", name: phrase("Inter"), family: "Inter" },
    Face { key: "source-serif", name: phrase("Source Serif"), family: "Source Serif 4" },
    Face { key: "system", name: phrase("System"), family: ".SystemUIFont" },
];

/// The faces code is set in: a block of code, a code span, a note as it
/// is written. The second is one choice whichever face a system has in
/// that place, so a settings file carried between systems keeps its meaning.
pub const MONO_FACES: &[Face] = &[
    Face { key: "jetbrains-mono", name: phrase("JetBrains Mono"), family: "JetBrains Mono" },
    Face { key: "system-mono", name: phrase("System Mono"), family: if cfg!(target_os = "macos") { "Menlo" } else if cfg!(target_os = "windows") { "Consolas" } else { "DejaVu Sans Mono" } },
];

const FACES: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Belleza-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Italic.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-Regular.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-Italic.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-SemiBold.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-Bold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-SemiBold.ttf"),
];

/// Load the carried faces. Called once at start.
pub fn install(cx: &mut App) {
    let faces = FACES.iter().map(|b| Cow::Borrowed(*b)).collect();
    if let Err(e) = cx.text_system().add_fonts(faces) {
        eprintln!("onyx: the carried faces did not load: {e}");
    }
}

/// The family a note's words are set in, by the settings' key; an
/// unknown key is the first face.
pub fn text_family(key: &str) -> SharedString {
    TEXT_FACES.iter().find(|f| f.key == key).unwrap_or(&TEXT_FACES[0]).family.into()
}

/// The family code is set in, by the settings' key.
pub fn mono_family(key: &str) -> SharedString {
    MONO_FACES.iter().find(|f| f.key == key).unwrap_or(&MONO_FACES[0]).family.into()
}

/// The app's name's face. Optima has only regular and bold, so the
/// wordmark draws its regular twice half a pixel apart for a weight
/// between (`widgets::wordmark`). Asked of the text system once: listing
/// every installed face takes about a tenth of a second on a Mac with
/// many fonts, and asked at every frame it made every interaction wait
/// that long.
pub fn wordmark_family(cx: &App) -> SharedString {
    static FAMILY: std::sync::OnceLock<SharedString> = std::sync::OnceLock::new();
    FAMILY
        .get_or_init(|| {
            let names = cx.text_system().all_font_names();
            if names.iter().any(|n| n == "Optima") { "Optima".into() } else { "Belleza".into() }
        })
        .clone()
}
