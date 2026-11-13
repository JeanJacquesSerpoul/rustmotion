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
    pub query_label: &'static str,
    pub seek_chip: &'static str,
    pub framing_label: &'static str,
    pub framing_auto: &'static str,
    pub framing_page: &'static str,
    pub height_from_frame: &'static str,
    pub script_linked: &'static str,
    pub script_injected: &'static str,
    pub script_missing: &'static str,
    pub need_probe: &'static str,
    pub export_refused: &'static str,
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
    query_label: "Paramètres d'URL (optionnel)",
    seek_chip: "Pilotée par window.seek()",
    framing_label: "Cadrage",
    framing_auto: "Élément principal (auto)",
    framing_page: "Page entière",
    height_from_frame: "Hauteur imposée par les proportions de l'élément cadré",
    script_linked: "chargé par la page",
    script_injected: "ajouté automatiquement",
    script_missing: "introuvable : la page HTML doit contenir toute l'animation",
    need_probe: "La page doit être analysée avant l'export.",
    export_refused: "Export refusé.",
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
    query_label: "URL parameters (optional)",
    seek_chip: "Driven by window.seek()",
    framing_label: "Framing",
    framing_auto: "Main element (auto)",
    framing_page: "Whole page",
    height_from_frame: "Height set by the framed element's proportions",
    script_linked: "loaded by the page",
    script_injected: "added automatically",
    script_missing: "not found: the HTML page must contain the whole animation",
    need_probe: "The page must be analysed before exporting.",
    export_refused: "Export refused.",
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

/// Textes de l'onglet « Motion design » et des onglets.
pub struct MotionTexts {
    pub tab_motion: &'static str,
    pub tab_convert: &'static str,
    pub brief: &'static str,
    pub brief_placeholder: &'static str,
    pub btn_import: &'static str,
    pub reading: &'static str,
    pub import_hint: &'static str,
    pub clear: &'static str,
    pub format: &'static str,
    pub width: &'static str,
    pub height: &'static str,
    pub duration_s: &'static str,
    pub ai: &'static str,
    pub ai_model: &'static str,
    pub models_loading: &'static str,
    pub models_unavailable: &'static str,
    pub outside_tauri: &'static str,
    pub proxy_show: &'static str,
    pub proxy_hide: &'static str,
    pub ai_url: &'static str,
    pub ai_key: &'static str,
    pub ai_key_placeholder: &'static str,
    pub proxy_hint: &'static str,
    pub waiting: &'static str,
    pub thinking: &'static str,
    pub preview: &'static str,
    pub preview_empty: &'static str,
    pub replay: &'static str,
    pub cancel: &'static str,
    pub failed: &'static str,
    pub cancelled: &'static str,
    pub generate: &'static str,
    pub need_brief: &'static str,
    pub need_model: &'static str,
    pub ready_hint: &'static str,
    pub truncated: &'static str,
    pub saved: &'static str,
    pub save_failed: &'static str,
    pub convert: &'static str,
    pub open_browser: &'static str,
    pub reveal: &'static str,
    pub refine: &'static str,
    pub refine_placeholder: &'static str,
    pub refine_btn: &'static str,
    pub regenerate: &'static str,
    pub regenerate_title: &'static str,
}

pub static MOTION_FR: MotionTexts = MotionTexts {
    tab_motion: "Motion design",
    tab_convert: "Conversion vidéo",
    brief: "Brief",
    brief_placeholder: "Décrivez l'animation : message, ambiance, couleurs, rythme, scènes…\nOu importez un PDF / un fichier texte.",
    btn_import: "Importer PDF / TXT…",
    reading: "Lecture du fichier…",
    import_hint: "Vous pouvez aussi déposer un PDF ou un .txt sur la fenêtre.",
    clear: "Effacer",
    format: "Format",
    width: "Largeur",
    height: "Hauteur",
    duration_s: "Durée (s)",
    ai: "Modèle IA",
    ai_model: "Modèle (identifiant OpenRouter)",
    models_loading: "Interrogation du proxy…",
    models_unavailable: "Liste des modèles indisponible : saisissez l'identifiant (ex. anthropic/claude-sonnet-5.5).",
    outside_tauri: "Disponible uniquement dans l'application.",
    proxy_show: "▸ Proxy IA",
    proxy_hide: "▾ Proxy IA",
    ai_url: "URL du proxy OpenRouter",
    ai_key: "Clé du proxy (PROXY_API_KEYS)",
    ai_key_placeholder: "Vide si le proxy n'en exige pas",
    proxy_hint: "La clé OpenRouter reste sur le proxy. Ces réglages sont mémorisés sur ce poste.",
    waiting: "Envoi du brief…",
    thinking: "Le modèle réfléchit…",
    preview: "Aperçu du motion design",
    preview_empty: "L'animation générée s'affichera ici",
    replay: "↻ Rejouer",
    cancel: "Annuler",
    failed: "Échec de la génération",
    cancelled: "Génération annulée",
    generate: "Générer le motion design",
    need_brief: "Rédigez ou importez d'abord un brief.",
    need_model: "Indiquez un modèle IA.",
    ready_hint: "Le brief est envoyé à l'IA, qui écrit une animation HTML convertible en vidéo.",
    truncated: "Réponse coupée par la limite du modèle : l'animation est peut-être incomplète.",
    saved: "Animation enregistrée ✔",
    save_failed: "Enregistrement impossible",
    convert: "Convertir en vidéo →",
    open_browser: "Ouvrir dans le navigateur",
    reveal: "Afficher dans le dossier",
    refine: "Ajustements (optionnel)",
    refine_placeholder: "Ex. : titre plus grand, fond bleu nuit, transition plus lente à la fin…",
    refine_btn: "Appliquer les ajustements",
    regenerate: "Régénérer",
    regenerate_title: "Nouvelle génération à partir du brief seul",
};

pub static MOTION_EN: MotionTexts = MotionTexts {
    tab_motion: "Motion design",
    tab_convert: "Video conversion",
    brief: "Brief",
    brief_placeholder: "Describe the animation: message, mood, colours, pace, scenes…\nOr import a PDF / text file.",
    btn_import: "Import PDF / TXT…",
    reading: "Reading file…",
    import_hint: "You can also drop a PDF or a .txt onto the window.",
    clear: "Clear",
    format: "Format",
    width: "Width",
    height: "Height",
    duration_s: "Duration (s)",
    ai: "AI model",
    ai_model: "Model (OpenRouter id)",
    models_loading: "Querying the proxy…",
    models_unavailable: "Model list unavailable: type the id (e.g. anthropic/claude-sonnet-5.5).",
    outside_tauri: "Only available in the app.",
    proxy_show: "▸ AI proxy",
    proxy_hide: "▾ AI proxy",
    ai_url: "OpenRouter proxy URL",
    ai_key: "Proxy key (PROXY_API_KEYS)",
    ai_key_placeholder: "Empty if the proxy doesn't require one",
    proxy_hint: "The OpenRouter key stays on the proxy. These settings are remembered on this computer.",
    waiting: "Sending the brief…",
    thinking: "The model is thinking…",
    preview: "Motion design preview",
    preview_empty: "The generated animation will appear here",
    replay: "↻ Replay",
    cancel: "Cancel",
    failed: "Generation failed",
    cancelled: "Generation cancelled",
    generate: "Generate the motion design",
    need_brief: "Write or import a brief first.",
    need_model: "Enter an AI model.",
    ready_hint: "The brief is sent to the AI, which writes an HTML animation ready to convert to video.",
    truncated: "Response cut off by the model's limit: the animation may be incomplete.",
    saved: "Animation saved ✔",
    save_failed: "Could not save",
    convert: "Convert to video →",
    open_browser: "Open in browser",
    reveal: "Show in folder",
    refine: "Adjustments (optional)",
    refine_placeholder: "E.g. bigger title, midnight blue background, slower final transition…",
    refine_btn: "Apply adjustments",
    regenerate: "Regenerate",
    regenerate_title: "New generation from the brief alone",
};

pub fn motion_texts(lang: Lang) -> &'static MotionTexts {
    match lang {
        Lang::Fr => &MOTION_FR,
        Lang::En => &MOTION_EN,
    }
}

/// Nombre avec espaces de milliers (« 12 345 »).
fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

pub fn chars(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Fr => format!("{} caractère{}", thousands(n), if n > 1 { "s" } else { "" }),
        Lang::En => format!("{} character{}", thousands(n), if n == 1 { "" } else { "s" }),
    }
}

pub fn models_found(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Fr => format!("{n} modèles proposés par le proxy (saisissez pour filtrer)."),
        Lang::En => format!("{n} models offered by the proxy (type to filter)."),
    }
}

pub fn writing(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Fr => format!("Écriture de l'animation… {}", chars(lang, n)),
        Lang::En => format!("Writing the animation… {}", chars(lang, n)),
    }
}

pub fn version_of(lang: Lang, i: usize, n: usize) -> String {
    match lang {
        Lang::Fr => format!("Version {i} / {n}"),
        Lang::En => format!("Version {i} / {n}"),
    }
}

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

pub fn framed_on(lang: Lang, label: &str) -> String {
    match lang {
        Lang::Fr => format!("Cadré sur {label}"),
        Lang::En => format!("Framed on {label}"),
    }
}

pub fn file_not_found(lang: Lang, file: &str) -> String {
    match lang {
        Lang::Fr => format!("Fichier introuvable : {file}"),
        Lang::En => format!("File not found: {file}"),
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
