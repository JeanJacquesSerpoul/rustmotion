//! Types partagés entre le moteur natif, l'application Tauri et l'interface Leptos (wasm).

use serde::{Deserialize, Serialize};

/// Langue de l'interface et des messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    #[default]
    Fr,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Fr, Lang::En];

    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }

    /// Interprète un code de langue (« fr », « fr-FR », « en_US »…). Tout ce qui n'est pas du français donne l'anglais.
    pub fn from_code(s: &str) -> Lang {
        if s.trim().to_ascii_lowercase().starts_with("fr") {
            Lang::Fr
        } else {
            Lang::En
        }
    }

    /// Choisit le texte dans la langue courante.
    pub fn pick<T>(self, fr: T, en: T) -> T {
        match self {
            Lang::Fr => fr,
            Lang::En => en,
        }
    }
}

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

    pub fn description(self, lang: Lang) -> &'static str {
        match self {
            Codec::H264 => lang.pick("Compatible partout", "Plays everywhere"),
            Codec::H265 => lang.pick("Plus léger, moderne", "Smaller, modern"),
            Codec::ProRes4444 => lang.pick("Master · alpha", "Master · alpha"),
            Codec::Vp9 => lang.pick("Web · alpha", "Web · alpha"),
        }
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
    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Stage::Launching => lang.pick("Lancement du navigateur…", "Launching browser…"),
            Stage::Loading => lang.pick("Chargement de la page…", "Loading page…"),
            Stage::Capturing => lang.pick("Capture des images", "Capturing frames"),
            Stage::Finalizing => lang.pick("Finalisation de l'encodage…", "Finishing encoding…"),
            Stage::Done => lang.pick("Terminé", "Done"),
            Stage::Cancelled => lang.pick("Annulé", "Cancelled"),
        }
    }
}

/// Origine de la durée détectée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurationSource {
    /// `<meta name="rustmotion:duration">` ou `window.RUSTMOTION.duration`.
    Page,
    /// `window.DURATION` d'une page pilotée par `window.seek(t)`.
    Script,
    /// Timeline globale GSAP.
    Gsap,
    /// Fin de la dernière animation CSS / Web Animations finie.
    Animations,
}

impl DurationSource {
    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            DurationSource::Page => lang.pick("déclarée par la page", "declared by the page"),
            DurationSource::Script => lang.pick("window.DURATION", "window.DURATION"),
            DurationSource::Gsap => lang.pick("timeline GSAP", "GSAP timeline"),
            DurationSource::Animations => lang.pick("animations CSS / Web Animations", "CSS / Web Animations"),
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
    pub duration_source: Option<DurationSource>,
    /// Au moins une animation infinie a été trouvée.
    pub infinite: bool,
    /// Nombre d'animations CSS / Web Animations trouvées.
    pub animations: u32,
    pub gsap: bool,
    /// La page dessine elle-même chaque image via `window.seek(t)`.
    pub seek_api: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    /// Miniature JPEG (data URL).
    pub thumbnail: Option<String>,
    /// Fichiers demandés par la page mais introuvables (chemins relatifs au dossier servi).
    #[serde(default)]
    pub missing_files: Vec<String>,
    /// Une activité d'animation a été détectée (animations CSS/WAAPI, requestAnimationFrame,
    /// timers, GSAP, window.seek, vidéo…).
    #[serde(default)]
    pub is_motion: bool,
}

impl ProbeInfo {
    /// Fichiers manquants qui empêchent la page de fonctionner (scripts et feuilles de style).
    pub fn blocking_missing(&self) -> Vec<String> {
        self.missing_files
            .iter()
            .filter(|f| {
                let f = f.to_ascii_lowercase();
                f.ends_with(".js") || f.ends_with(".mjs") || f.ends_with(".css")
            })
            .cloned()
            .collect()
    }

    /// Raison de refuser l'export, le cas échéant.
    pub fn rejection(&self) -> Option<Rejection> {
        let missing = self.blocking_missing();
        if !missing.is_empty() {
            Some(Rejection::MissingFiles(missing))
        } else if !self.is_motion {
            Some(Rejection::NotMotion)
        } else {
            None
        }
    }
}

/// Pourquoi une page est refusée.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rejection {
    /// La page référence des scripts ou des styles absents : elle est incomplète.
    MissingFiles(Vec<String>),
    /// Aucune animation détectée : ce n'est pas un motion design.
    NotMotion,
}

impl Rejection {
    pub fn message(&self, lang: Lang) -> String {
        match self {
            Rejection::MissingFiles(files) => match lang {
                Lang::Fr => format!("Page incomplète : fichier(s) introuvable(s) : {}.", files.join(", ")),
                Lang::En => format!("Incomplete page: missing file(s): {}.", files.join(", ")),
            },
            Rejection::NotMotion => lang
                .pick(
                    "Aucune animation détectée : ce fichier n'est pas un motion design complet.",
                    "No animation detected: this file is not a complete motion design.",
                )
                .to_string(),
        }
    }
}

/// Fichiers qui composent la source : la page HTML et son script compagnon de même nom.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SourceFiles {
    /// Nom du fichier HTML (None pour une URL).
    pub html: Option<String>,
    /// Nom du script attendu à côté (`page.html` → `page.js`), pour une page locale.
    pub expected_script: Option<String>,
    /// Le script compagnon existe.
    pub script_found: bool,
    /// La page le charge déjà via `<script src>` ; sinon RustMotion l'ajoute.
    pub script_linked: bool,
}

impl SourceFiles {
    /// Le script compagnon existe mais n'est pas chargé par la page : il sera ajouté.
    pub fn script_injected(&self) -> bool {
        self.script_found && !self.script_linked
    }

    /// Page locale sans script compagnon.
    pub fn script_missing(&self) -> bool {
        self.expected_script.is_some() && !self.script_found
    }
}

/// Outils externes détectés.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolStatus {
    pub browser: Option<String>,
    pub ffmpeg: Option<String>,
    pub ffmpeg_version: Option<String>,
}
