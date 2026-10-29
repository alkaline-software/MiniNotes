//! The settings: defaults, then `~/.onyx/config.json`, then the
//! environment. Loading never fails; a file that does not parse is the
//! defaults. Every field has a default, so a file written by an older
//! version, or edited by hand with a field missing, still loads.

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    /// `system` follows the OS; `light` and `dark` pin one look.
    pub appearance: String,
    /// The accent, by its key in the app's `look::ACCENTS`.
    pub accent: String,
    /// `system`, or a language's code. On a Mac the app's own
    /// `AppleLanguages` is the setting and this is not read.
    pub language: String,
    /// Ask for the newest release once a day and say so in Settings.
    /// Nothing is installed without the person's press.
    pub check_updates: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { appearance: "system".into(), accent: "teal".into(), language: "system".into(), check_updates: true }
    }
}

/// How a note is set in the editor and as it reads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EditorConfig {
    /// The face a note's words are set in, by its key in the app's
    /// `fonts::TEXT_FACES`: `inter`, `source-serif`, or `system`.
    pub text_face: String,
    /// The face code is set in, by its key in `fonts::MONO_FACES`.
    pub mono_face: String,
    /// `small`, `medium` or `large`: the words' size in every place at once.
    pub text_size: String,
    /// Lines no wider than a comfortable line of reading, centered;
    /// off, a note runs the width of the window.
    pub readable_width: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self { text_face: "inter".into(), mono_face: "jetbrains-mono".into(), text_size: "medium".into(), readable_width: true }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub app: AppConfig,
    pub editor: EditorConfig,
}

impl Config {
    /// The settings as the app runs with them: the file, then the
    /// environment's overrides (`ONYX_APPEARANCE` for a probe).
    pub fn load() -> Config {
        let mut cfg = Self::on_disk();
        if let Ok(v) = std::env::var("ONYX_APPEARANCE") {
            if !v.is_empty() {
                cfg.app.appearance = v;
            }
        }
        cfg
    }

    fn on_disk() -> Config {
        paths::read_json(&paths::config_file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }

    /// Change the file on disk. It starts from the file, not from `load`,
    /// so an override in the environment is never written back.
    pub fn edit(change: impl FnOnce(&mut Config)) -> std::io::Result<()> {
        let mut cfg = Self::on_disk();
        change(&mut cfg);
        paths::ensure_dirs()?;
        paths::write_json(&paths::config_file(), &serde_json::to_value(&cfg)?)
    }
}
