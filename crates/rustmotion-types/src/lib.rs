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
    /// Partie de la page à filmer.
    #[serde(default)]
    pub framing: Framing,
}

/// Partie de la page à filmer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Framing {
    /// L'élément principal (grand canvas, svg, vidéo…) s'il ne remplit pas déjà la page ; sinon la page entière.
    #[default]
    Auto,
    /// Toute la fenêtre.
    Page,
    /// L'élément désigné par ce sélecteur CSS.
    Selector(String),
}

impl Framing {
    /// `auto`, `page`, ou un sélecteur CSS.
    pub fn parse(s: &str) -> Framing {
        match s.trim() {
            "" | "auto" => Framing::Auto,
            "page" => Framing::Page,
            sel => Framing::Selector(sel.to_string()),
        }
    }
}

/// Élément sur lequel la vidéo est cadrée.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameInfo {
    /// Description courte : `canvas#cv`, `svg.scene`…
    pub label: String,
    /// Taille de l'élément une fois ajusté, en pixels CSS.
    pub width: f64,
    pub height: f64,
}

impl FrameInfo {
    /// Hauteur de sortie pour une largeur donnée, en gardant les proportions de l'élément (arrondie au pair).
    pub fn height_for(&self, width: u32) -> u32 {
        let h = width as f64 * self.height / self.width.max(1.0);
        ((h / 2.0).round() * 2.0).max(2.0) as u32
    }
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
            framing: Framing::Auto,
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
    /// `window.DURATION` (ou `__DURATION`) déclarée par le script de la page.
    Script,
    /// Lecteur intégré de la page : horloge « 0:00 / 1:08 » et curseur de position.
    Player,
    /// Durée d'une vidéo ou d'un son de la page.
    Media,
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
            DurationSource::Player => lang.pick("lecteur de la page", "page's player"),
            DurationSource::Media => lang.pick("vidéo / son de la page", "page's video / audio"),
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
    /// Élément sur lequel la vidéo est cadrée (cadrage automatique ou sélecteur), s'il y en a un.
    #[serde(default)]
    pub frame: Option<FrameInfo>,
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
    /// Bande son compagnon de même nom (`page.html` → `page.mp3`…), chemin complet.
    #[serde(default)]
    pub audio: Option<String>,
}

/// Extensions reconnues pour la bande son compagnon, par ordre de préférence.
pub const AUDIO_EXTS: [&str; 7] = ["mp3", "wav", "m4a", "aac", "ogg", "flac", "opus"];

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

/// Consignes envoyées à l'IA pour générer un motion design (onglet « Motion design »).
pub mod motion {
    /// Cadence inscrite dans les animations générées.
    pub const FPS: u32 = 30;

    /// Consignes techniques données au modèle : le document doit être rendu image par image
    /// par le moteur (horloge virtuelle), d'où une durée finie et un temps exclusivement piloté par l'horloge.
    pub fn system_prompt(w: u32, h: u32, duration: f64) -> String {
        let d = format!("{duration}");
        format!(
            r#"You are a senior motion designer and creative front-end developer. You turn a brief into ONE self-contained HTML document that plays a polished, professional motion design animation. It will be rendered frame by frame into a video by a headless browser with a virtual clock.

Hard requirements:
- Frame: exactly {w}×{h} px. html and body: margin 0, width {w}px, height {h}px, overflow hidden. Lay everything out for that exact size; nothing may overflow or be cut off.
- In <head>, include: <meta name="rustmotion:duration" content="{d}">, <meta name="rustmotion:width" content="{w}">, <meta name="rustmotion:height" content="{h}">, <meta name="rustmotion:fps" content="{FPS}">, and a short descriptive <title>.
- Total duration: exactly {d} seconds. Everything starts automatically at load (t = 0), with no user interaction, and the last frame holds at the end.
- Finite animation only: never `infinite` iteration counts, never GSAP `repeat: -1`, no endless requestAnimationFrame logic that depends on wall-clock time. Ambient/looping motion must use a finite repeat count covering the duration.
- Drive time only with CSS animations/transitions, the Web Animations API, GSAP timelines, or requestAnimationFrame / performance.now(). No Date-based randomness; if you need randomness, use a small seeded PRNG so every render is identical.
- Allowed external resources: GSAP from https://cdn.jsdelivr.net/npm/gsap@3.12.5/dist/gsap.min.js (and its plugins from the same CDN version), and Google Fonts. Everything else (shapes, icons, illustrations, backgrounds) must be inline SVG, CSS or canvas. No external images, video or audio.
- No visible controls, scrollbars, timecodes, watermarks or debug output.
- Write any on-screen text in the language of the brief, with correct spelling and accents.

Design quality: clear visual hierarchy, a coherent palette and typography, purposeful easing (no linear motion except for constant drifts), staggered and well-paced reveals, scene transitions when the brief has several messages, and enough time for each text to be read.

Answer with the complete HTML document only, starting with <!DOCTYPE html> and ending with </html>. No explanation, no Markdown code fences."#
        )
    }

    pub fn brief_message(brief: &str) -> String {
        format!("Brief:\n\n{}\n\nCreate the motion design described in this brief.", brief.trim())
    }

    pub fn refine_message(instructions: &str) -> String {
        format!(
            "Apply these changes to the animation, keep everything else, and return the complete updated HTML document:\n\n{}",
            instructions.trim()
        )
    }

    /// Modèle musical pour une bande son courte (clip de 30 s).
    pub const MUSIC_CLIP_MODEL: &str = "google/lyria-3-clip-preview";
    /// Modèle musical pour un morceau complet (animations de plus de 30 s).
    pub const MUSIC_FULL_MODEL: &str = "google/lyria-3-pro-preview";
    /// Durée d'un clip produit par le modèle court.
    pub const MUSIC_CLIP_SECONDS: f64 = 30.0;

    /// Modèle musical adapté à la durée de l'animation, et son prix fixe (USD par génération).
    pub fn music_model(duration: f64) -> (&'static str, f64) {
        if duration <= MUSIC_CLIP_SECONDS {
            (MUSIC_CLIP_MODEL, 0.04)
        } else {
            (MUSIC_FULL_MODEL, 0.08)
        }
    }

    /// Texte visible d'un document HTML (textes à l'écran), sans scripts, styles ni en-tête,
    /// limité à `max` caractères : sert à décrire une animation existante au modèle musical.
    pub fn visible_text(html: &str, max: usize) -> String {
        let lower = html.to_ascii_lowercase();
        let mut out = String::new();
        let mut i = 0;
        while i < html.len() {
            if lower[i..].starts_with('<') {
                // Saute aussi le contenu des balises sans texte visible.
                let from = ["script", "style", "head", "title"]
                    .into_iter()
                    .find(|t| lower[i + 1..].starts_with(t))
                    .and_then(|t| lower[i..].find(&format!("</{t}")).map(|e| i + e))
                    .unwrap_or(i);
                i = lower[from..].find('>').map_or(html.len(), |e| from + e + 1);
                out.push(' ');
                continue;
            }
            let next = lower[i..].find('<').map_or(html.len(), |e| i + e);
            out.push_str(&html[i..next]);
            i = next;
        }
        let text = out
            .replace("&nbsp;", " ")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&#39;", "'")
            .replace("&quot;", "\"");
        let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if joined.chars().count() > max {
            joined.chars().take(max).collect::<String>() + "…"
        } else {
            joined
        }
    }

    /// Demande envoyée au modèle musical. `context` décrit l'animation (brief, ou titre et
    /// textes d'une animation existante) ; `style` est le style musical souhaité (facultatif).
    pub fn soundtrack_prompt(context: &str, duration: f64, style: &str) -> String {
        let d = (duration * 10.0).round() / 10.0;
        let style = match style.trim() {
            "" => "Choose a style that matches the mood and pace of the animation.".to_string(),
            s => format!("Musical style: {s}."),
        };
        format!(
            "Instrumental soundtrack for a {d}-second motion design video. No vocals, no lyrics.\n{style}\n\
             It starts immediately, follows the rhythm of the animation with clear accents on scene changes, \
             and ends with a clean final hit at {d} seconds (anything after that will be cut).\n\n\
             About the animation:\n{}",
            context.trim()
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn music_model_by_duration() {
            assert_eq!(music_model(8.0), (MUSIC_CLIP_MODEL, 0.04));
            assert_eq!(music_model(30.0).0, MUSIC_CLIP_MODEL);
            assert_eq!(music_model(68.0), (MUSIC_FULL_MODEL, 0.08));
        }

        #[test]
        fn visible_text_skips_code() {
            let h = "<html><head><title>T</title><style>.a{x:1}</style></head><body><h1>Un petit&nbsp;saut</h1>\
                     <script>let a = '<b>';</script><p>pour toucher\n le ciel</p></body></html>";
            assert_eq!(visible_text(h, 100), "Un petit saut pour toucher le ciel");
            assert_eq!(visible_text(h, 8), "Un petit…");
        }

        #[test]
        fn prompt_mentions_duration_and_style() {
            let p = soundtrack_prompt("Brief: un saut", 8.0, "ukulélé joyeux");
            assert!(p.contains("8-second") && p.contains("ukulélé joyeux") && p.contains("Brief: un saut"));
            assert!(soundtrack_prompt("x", 12.25, "").contains("12.3-second"));
        }
    }
}
