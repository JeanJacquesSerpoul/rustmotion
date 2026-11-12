//! RustMotion — convertit une animation HTML/CSS/JS en vidéo.
//!
//! Principe : une page est chargée dans Chromium headless (piloté en CDP) avec une
//! horloge virtuelle injectée (`shim.js`). Pour chaque image, le temps virtuel est
//! avancé de 1/fps, la page est capturée en PNG et envoyée à ffmpeg via stdin.

pub mod browser;
pub mod cdp;
pub mod encoder;
pub mod i18n;
pub mod render;
pub mod server;
mod util;

pub use browser::find_browser;
pub use encoder::{ffmpeg_args, ffmpeg_version, find_ffmpeg};
pub use i18n::{lang, set_lang, system_lang};
pub use render::{probe, render, resolve_input, Outcome};
pub use rustmotion_types as types;

use rustmotion_types::ToolStatus;

/// Détecte le navigateur et ffmpeg disponibles.
pub fn tool_status(browser: Option<&str>, ffmpeg: Option<&str>) -> ToolStatus {
    let browser = match browser.filter(|s| !s.trim().is_empty()) {
        Some(p) => std::path::Path::new(p).is_file().then(|| p.to_string()),
        None => find_browser().map(|p| p.display().to_string()),
    };
    let ff = find_ffmpeg(ffmpeg);
    ToolStatus {
        browser,
        ffmpeg_version: ff.as_deref().and_then(ffmpeg_version),
        ffmpeg: ff.map(|p| p.display().to_string()),
    }
}
