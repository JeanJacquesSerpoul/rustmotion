//! Textes de l'interface en français et en anglais.

use rustmotion_types::Lang;

const STORAGE_KEY: &str = "rustmotion.lang";

pub struct Texts {
    pub tagline: &'static str,
    pub detecting_tools: &'static str,
    pub outside_tauri: &'static str,
    pub ffmpeg_missing: &'static str,
    pub browser_missing: &'static str,
    pub lang_title: &'static str,
    // Source
    pub source: &'static str,
    pub drop_title: &'static str,
    pub drop_sub: &'static str,
    pub path_placeholder: &'static str,
    pub btn_file: &'static str,
    pub btn_folder: &'static str,
    pub analysing: &'static str,
    pub probe_hint: &'static str,
    pub reanalyse: &'static str,
    pub duration_unknown: &'static str,
    pub infinite_hint: &'static str,
    // Format
    pub format: &'static str,
    pub presets: &'static str,
    pub width: &'static str,
    pub height: &'static str,
    pub duration_s: &'static str,
    pub start_s: &'static str,
    pub fps_label: &'static str,
    pub sharpness: &'static str,
    // Export
    pub export: &'static str,
    pub quality: &'static str,
    pub transparent: &'static str,
    pub transparent_hint: &'static str,
    pub audio_label: &'static str,
    pub audio_none: &'static str,
    pub btn_choose: &'static str,
    pub output_label: &'static str,
    pub btn_save_as: &'static str,
    pub advanced_show: &'static str,
    pub advanced_hide: &'static str,
    pub wait_label: &'static str,
    pub browser_label: &'static str,
    pub ffmpeg_label: &'static str,
    // Aperçu / rendu
    pub preview: &'static str,
    pub preview_alt: &'static str,
    pub frames: &'static str,
    pub rate: &'static str,
    pub duration: &'static str,
    pub format_short: &'static str,
    pub frame: &'static str,
    pub speed: &'static str,
    pub speed_unit: &'static str,
    pub elapsed: &'static str,
    pub remaining: &'static str,
    pub cancel: &'static str,
    pub video_ready: &'static str,
    pub play: &'static str,
    pub reveal: &'static str,
    pub rerender: &'static str,
    pub render_failed: &'static str,
    pub render_cancelled: &'static str,
    pub export_btn: &'static str,
    pub need_tools: &'static str,
    pub need_input: &'static str,
    pub ready_hint: &'static str,
    pub drop_overlay: &'static str,
}

pub static FR: Texts = Texts {
    tagline: "Animation HTML → vidéo, image par image",
    detecting_tools: "Détection des outils…",
    outside_tauri: "Hors Tauri",
    ffmpeg_missing: "ffmpeg introuvable",
    browser_missing: "Chromium introuvable",
    lang_title: "Langue",
    source: "Source",
    drop_title: "Déposez un fichier .html ici",
    drop_sub: "ou un dossier contenant index.html, ou collez une URL",
    path_placeholder: "Chemin ou https://…",
    btn_file: "Fichier…",
    btn_folder: "Dossier…",
    analysing: "Analyse de la page…",
    probe_hint: "La page est analysée automatiquement pour détecter sa durée.",
    reanalyse: "Ré-analyser",
    duration_unknown: "Durée non détectée",
    infinite_hint: "Animation infinie : fixez la durée",
    format: "Format",
    presets: "Préréglages",
    width: "Largeur",
    height: "Hauteur",
    duration_s: "Durée (s)",
    start_s: "Départ (s)",
    fps_label: "Images / seconde",
    sharpness: "Netteté (devicePixelRatio)",
    export: "Export",
    quality: "Qualité",
    transparent: "Fond transparent",
    transparent_hint: " (la page doit avoir un fond transparent — ProRes 4444 / VP9)",
    audio_label: "Piste audio (optionnelle)",
    audio_none: "Aucune",
    btn_choose: "Choisir…",
    output_label: "Fichier de sortie",
    btn_save_as: "Enregistrer sous…",
    advanced_show: "▸ Options avancées",
    advanced_hide: "▾ Options avancées",
    wait_label: "Attente après chargement (ms, temps réel)",
    browser_label: "Navigateur Chromium (vide = auto)",
    ffmpeg_label: "ffmpeg (vide = auto)",
    preview: "Aperçu",
    preview_alt: "Aperçu de l'animation",
    frames: "Images",
    rate: "Cadence",
    duration: "Durée",
    format_short: "Format",
    frame: "Image",
    speed: "Vitesse",
    speed_unit: "img/s",
    elapsed: "Écoulé",
    remaining: "Restant",
    cancel: "Annuler",
    video_ready: "Vidéo prête ✔",
    play: "Lire la vidéo",
    reveal: "Afficher dans le dossier",
    rerender: "Relancer le rendu",
    render_failed: "Échec du rendu",
    render_cancelled: "Rendu annulé",
    export_btn: "Exporter la vidéo",
    need_tools: "Chromium (Chrome/Edge) et ffmpeg sont nécessaires.",
    need_input: "Choisissez d'abord une animation HTML.",
    ready_hint: "Chaque image est rendue à temps fixe : aucune saccade, quelle que soit la machine.",
    drop_overlay: "Déposez pour charger",
};

pub static EN: Texts = Texts {
    tagline: "HTML animation → video, frame by frame",
    detecting_tools: "Detecting tools…",
    outside_tauri: "Not running in Tauri",
    ffmpeg_missing: "ffmpeg not found",
    browser_missing: "Chromium not found",
    lang_title: "Language",
    source: "Source",
    drop_title: "Drop an .html file here",
    drop_sub: "or a folder containing index.html, or paste a URL",
    path_placeholder: "Path or https://…",
    btn_file: "File…",
    btn_folder: "Folder…",
    analysing: "Analysing page…",
    probe_hint: "The page is analysed automatically to detect its duration.",
    reanalyse: "Re-analyse",
    duration_unknown: "Duration not detected",
    infinite_hint: "Infinite animation: set the duration",
    format: "Format",
    presets: "Presets",
    width: "Width",
    height: "Height",
    duration_s: "Duration (s)",
    start_s: "Start (s)",
    fps_label: "Frames per second",
    sharpness: "Sharpness (devicePixelRatio)",
    export: "Export",
    quality: "Quality",
    transparent: "Transparent background",
    transparent_hint: " (the page must have a transparent background — ProRes 4444 / VP9)",
    audio_label: "Audio track (optional)",
    audio_none: "None",
    btn_choose: "Choose…",
    output_label: "Output file",
    btn_save_as: "Save as…",
    advanced_show: "▸ Advanced options",
    advanced_hide: "▾ Advanced options",
    wait_label: "Wait after page load (ms, real time)",
    browser_label: "Chromium browser (empty = auto)",
    ffmpeg_label: "ffmpeg (empty = auto)",
    preview: "Preview",
    preview_alt: "Animation preview",
    frames: "Frames",
    rate: "Frame rate",
    duration: "Duration",
    format_short: "Format",
    frame: "Frame",
    speed: "Speed",
    speed_unit: "fps",
    elapsed: "Elapsed",
    remaining: "Remaining",
    cancel: "Cancel",
    video_ready: "Video ready ✔",
    play: "Play video",
    reveal: "Show in folder",
    rerender: "Render again",
    render_failed: "Render failed",
    render_cancelled: "Render cancelled",
    export_btn: "Export video",
    need_tools: "Chromium (Chrome/Edge) and ffmpeg are required.",
    need_input: "Choose an HTML animation first.",
    ready_hint: "Every frame is rendered at an exact time: no stutter, whatever the machine.",
    drop_overlay: "Drop to load",
};

pub fn texts(lang: Lang) -> &'static Texts {
    match lang {
        Lang::Fr => &FR,
        Lang::En => &EN,
    }
}

pub fn crf_label(lang: Lang, crf: u8) -> &'static str {
    match crf {
        0 => lang.pick("sans perte", "lossless"),
        1..=14 => lang.pick("quasi sans perte", "visually lossless"),
        15..=20 => lang.pick("très haute", "very high"),
        21..=26 => lang.pick("haute", "high"),
        27..=32 => lang.pick("moyenne", "medium"),
        _ => lang.pick("basse", "low"),
    }
}

pub fn animations(lang: Lang, n: u32) -> String {
    // Le français met le singulier à 0 et 1, l'anglais seulement à 1.
    match lang {
        Lang::Fr => format!("{n} animation{}", if n > 1 { "s" } else { "" }),
        Lang::En => format!("{n} animation{}", if n == 1 { "" } else { "s" }),
    }
}

pub fn frames_in(lang: Lang, n: u32, time: &str) -> String {
    match lang {
        Lang::Fr => format!("{n} images en {time}"),
        Lang::En => format!("{n} frames in {time}"),
    }
}

/// Langue mémorisée, sinon celle du système (via la WebView).
pub fn initial_lang() -> Lang {
    let Some(w) = web_sys::window() else { return Lang::Fr };
    if let Some(code) = w.local_storage().ok().flatten().and_then(|s| s.get_item(STORAGE_KEY).ok().flatten()) {
        return Lang::from_code(&code);
    }
    w.navigator().language().map(|l| Lang::from_code(&l)).unwrap_or(Lang::Fr)
}

/// Mémorise la langue et met à jour l'attribut `lang` du document.
pub fn remember_lang(lang: Lang) {
    let Some(w) = web_sys::window() else { return };
    if let Some(s) = w.local_storage().ok().flatten() {
        let _ = s.set_item(STORAGE_KEY, lang.code());
    }
    if let Some(el) = w.document().and_then(|d| d.document_element()) {
        let _ = el.set_attribute("lang", lang.code());
    }
}
