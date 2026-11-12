//! Types partagés entre le moteur natif, l'application Tauri et l'interface Leptos (wasm).

use serde::{Deserialize, Serialize};

/// Codec / conteneur de sortie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    /// MP4 H.264 (libx264) — compatible partout.
    #[default]
    H264,
    /// MP4 H.265 / HEVC (libx265) — plus léger à qualité égale.
    H265,
    /// MOV ProRes 4444 — qualité master, supporte la transparence.
    ProRes4444,
    /// WebM VP9 — web, supporte la transparence.
    Vp9,
}

impl Codec {
    pub const ALL: [Codec; 4] = [Codec::H264, Codec::H265, Codec::ProRes4444, Codec::Vp9];

    pub fn extension(self) -> &'static str {
        match self {
            Codec::H264 | Codec::H265 => "mp4",
            Codec::ProRes4444 => "mov",
            Codec::Vp9 => "webm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Codec::H264 => "MP4 · H.264",
            Codec::H265 => "MP4 · H.265 (HEVC)",
            Codec::ProRes4444 => "MOV · ProRes 4444",
            Codec::Vp9 => "WebM · VP9",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Codec::H264 => "h264",
            Codec::H265 => "h265",
            Codec::ProRes4444 => "prores4444",
            Codec::Vp9 => "vp9",
        }
    }

    pub fn from_id(s: &str) -> Option<Codec> {
        Codec::ALL.into_iter().find(|c| c.id().eq_ignore_ascii_case(s))
    }

    /// Le codec peut-il conserver un canal alpha ?
    pub fn supports_alpha(self) -> bool {
        matches!(self, Codec::ProRes4444 | Codec::Vp9)
    }

    /// Le codec utilise-t-il un réglage CRF ?
    pub fn uses_crf(self) -> bool {
        !matches!(self, Codec::ProRes4444)
    }
}

/// Paramètres complets d'un rendu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderSettings {
    /// Chemin d'un fichier .html, d'un dossier contenant index.html, ou URL http(s).
    pub input: String,
    /// Fichier vidéo de sortie.
    pub output: String,
    /// Largeur du viewport CSS (px).
    pub width: u32,
    /// Hauteur du viewport CSS (px).
    pub height: u32,
    /// Facteur de rendu (devicePixelRatio). 2.0 => vidéo 2x plus grande, rendu net.
    pub scale: f64,
    pub fps: u32,
    /// Durée capturée, en secondes.
    pub duration: f64,
    /// Décalage de départ dans l'animation, en secondes.
    pub start: f64,
    pub codec: Codec,
    /// CRF (0 = sans perte, 51 = pire). Ignoré pour ProRes.
    pub crf: u8,
    /// Fond transparent (ProRes 4444 / VP9 uniquement).
    pub transparent: bool,
    /// Piste audio optionnelle à multiplexer.
    pub audio: Option<String>,
    /// Attente supplémentaire (ms, temps réel) après le chargement de la page.
    pub wait_ms: u32,
    /// Chemin explicite du navigateur (sinon détection automatique).
    pub browser_path: Option<String>,
    /// Chemin explicite de ffmpeg (sinon détection automatique).
    pub ffmpeg_path: Option<String>,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            input: String::new(),
            output: String::new(),
            width: 1920,
            height: 1080,
            scale: 1.0,
            fps: 30,
            duration: 5.0,
            start: 0.0,
            codec: Codec::H264,
            crf: 18,
            transparent: false,
            audio: None,
            wait_ms: 250,
            browser_path: None,
            ffmpeg_path: None,
        }
    }
}

impl RenderSettings {
    pub fn total_frames(&self) -> u32 {
        ((self.duration * self.fps as f64).round() as u32).max(1)
    }

    pub fn output_size(&self) -> (u32, u32) {
        let w = (self.width as f64 * self.scale).round() as u32;
        let h = (self.height as f64 * self.scale).round() as u32;
        (w, h)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Launching,
    Loading,
    Capturing,
    Finalizing,
    Done,
    Cancelled,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Launching => "Lancement du navigateur…",
            Stage::Loading => "Chargement de la page…",
            Stage::Capturing => "Capture des images",
            Stage::Finalizing => "Finalisation de l'encodage…",
            Stage::Done => "Terminé",
            Stage::Cancelled => "Annulé",
        }
    }
}

/// Événement de progression émis pendant un rendu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub stage: Stage,
    pub frame: u32,
    pub total: u32,
    pub elapsed_ms: u64,
    pub eta_ms: Option<u64>,
    /// Vitesse de capture (images / seconde réelle).
    pub render_fps: f64,
    /// Miniature JPEG en data URL (envoyée de temps en temps).
    pub preview: Option<String>,
}

/// Résultat de l'analyse d'une page.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProbeInfo {
    pub title: String,
    /// Durée détectée (s) : meta, GSAP ou animations CSS/WAAPI finies.
    pub duration: Option<f64>,
    /// D'où vient la durée détectée.
    pub duration_source: Option<String>,
    /// Au moins une animation infinie a été trouvée.
    pub infinite: bool,
    /// Nombre d'animations CSS / Web Animations trouvées.
    pub animations: u32,
    pub gsap: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    /// Miniature JPEG (data URL).
    pub thumbnail: Option<String>,
}

/// Outils externes détectés.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolStatus {
    pub browser: Option<String>,
    pub ffmpeg: Option<String>,
    pub ffmpeg_version: Option<String>,
}
