//! Onglet « Motion design » : un brief (saisi, ou lu dans un PDF / .txt) est envoyé à l'IA
//! via le proxy OpenRouter ; l'animation HTML produite est prévisualisée, ajustable,
//! puis transmise à l'onglet de conversion vidéo.

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use rustmotion_types::motion::{
    brief_message, music_code_message, music_code_system, music_model, refine_message, soundtrack_prompt, system_prompt,
    visible_text,
};
use rustmotion_types::Lang;
use serde::{Deserialize, Serialize};

use crate::app::PRESETS;
use crate::i18n::{self, motion_texts};
use crate::icons::{icon, Icon};
use crate::settings::{load, store, AiSettings};
use crate::tauri::{self, invoke};

const KEY_MUSIC_STYLE: &str = "rustmotion.music.style";
const KEY_WITH_MUSIC: &str = "rustmotion.music.auto";
const KEY_SOUND: &str = "rustmotion.preview.sound";
const KEY_MUSIC_ENGINE: &str = "rustmotion.music.engine";

/// Durée d'un nouveau projet (s).
const DEFAULT_DURATION: f64 = 20.0;
/// Durées proposées dans la liste (s) ; toute autre valeur reste possible.
const DURATIONS: [f64; 5] = [10.0, 20.0, 30.0, 60.0, 90.0];

#[derive(Clone, PartialEq)]
enum Gen {
    Idle,
    Running,
    Cancelled,
    Error(String),
}

#[derive(Serialize, Clone)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize, Clone)]
struct AiResult {
    html: String,
    truncated: bool,
    model: Option<String>,
    #[serde(default)]
    cost: Option<f64>,
}

#[derive(Deserialize, Clone)]
struct AiProgress {
    chars: usize,
    reasoning: usize,
    tail: String,
    elapsed_ms: u64,
}

/// Qui compose la bande son.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MusicEngine {
    /// Le modèle de l'animation écrit le code Web Audio de la bande son (son synthétisé, calé sur l'animation).
    Same,
    /// Modèle musical Lyria 3 (musique réaliste, prix fixe).
    Lyria,
}

impl MusicEngine {
    fn key(self) -> &'static str {
        match self {
            MusicEngine::Same => "same",
            MusicEngine::Lyria => "lyria",
        }
    }
}

/// Bande son à composer : moteur, modèle, coût (fixe pour Lyria, estimé sinon) et description.
#[derive(Clone)]
struct MusicPlan {
    engine: MusicEngine,
    model: String,
    cost: Option<f64>,
    duration: f64,
    /// Description de l'animation (brief, titre, textes à l'écran).
    context: String,
    style: String,
}

impl MusicPlan {
    /// Ce que reçoit le modèle (montré dans la confirmation).
    fn preview(&self) -> String {
        match self.engine {
            MusicEngine::Lyria => soundtrack_prompt(&self.context, self.duration, &self.style),
            MusicEngine::Same => music_code_message(&self.context, "…").replace("…\n\nWrite", "[HTML]\n\nWrite"),
        }
    }
}

/// Réponse attendue du compositeur-codeur (une fonction compose() de quelques centaines de lignes).
const MUSIC_CODE_TOKENS: usize = 5000;

/// Requête payante préparée, en attente de confirmation.
#[derive(Clone)]
enum Pending {
    /// Génération (ou ajustement) de l'animation, éventuellement suivie de sa bande son.
    Animation {
        messages: Vec<ChatMessage>,
        request: String,
        refine: bool,
        model: String,
        input_tokens: usize,
        output_tokens: usize,
        /// Coût estimé en USD (inconnu si le proxy ne fournit pas le tarif du modèle).
        cost: Option<f64>,
        music: Option<MusicPlan>,
        /// Bande son de la version ajustée, reprise par la nouvelle version.
        carry_audio: Option<String>,
    },
    /// Bande son seule, pour l'animation affichée.
    Music { html_path: String, plan: MusicPlan },
}

#[derive(Clone, PartialEq)]
enum MusicGen {
    Idle,
    /// Composition en cours (secondes écoulées).
    Running(u32),
    Cancelled,
    /// Bande son retirée : nom sous lequel elle a été mise de côté.
    Removed(String),
    Error(String),
}

/// Réponse attendue pour une première génération (une animation de quelques scènes ≈ 10 000 caractères).
const EXPECTED_OUTPUT_TOKENS: usize = 5000;

/// Estimation grossière du nombre de jetons d'un texte (≈ 3,5 caractères par jeton pour du code / texte mixte).
fn estimate_tokens(chars: usize) -> usize {
    (chars as f64 / 3.5).ceil() as usize
}

fn fmt_tokens(lang: Lang, n: usize) -> String {
    let approx = if n >= 10_000 { format!("{} k", (n + 500) / 1000) } else { n.to_string() };
    lang.pick(format!("≈ {approx} jetons"), format!("≈ {approx} tokens"))
}

/// Montant réellement facturé (sans « ≈ ») : 3 décimales au plus, 2 au moins.
fn fmt_cost(lang: Lang, v: f64) -> String {
    let s = if v > 0.0 && v < 0.001 {
        "< 0.001".to_string()
    } else {
        let s = format!("{v:.3}");
        if s.ends_with('0') { s[..s.len() - 1].to_string() } else { s }
    };
    match lang {
        Lang::Fr => format!("{} $", s.replace('.', ",")),
        Lang::En => format!("${s}"),
    }
}

fn fmt_usd(lang: Lang, v: f64) -> String {
    let s = if v < 0.01 { "< 0.01".to_string() } else { format!("{v:.2}") };
    match lang {
        Lang::Fr => format!("≈ {} $", s.replace('.', ",")),
        Lang::En => format!("≈ ${s}"),
    }
}

#[derive(Deserialize)]
struct PromptFile {
    name: String,
    text: String,
}

/// Une version générée de l'animation.
#[derive(Clone)]
struct Version {
    html: String,
    path: Option<String>,
    truncated: bool,
    model: Option<String>,
    width: u32,
    height: u32,
    duration: f64,
    /// Format sûr (généré ici ou lu dans les balises `rustmotion:`) ; sinon repris de l'analyse.
    sized: bool,
    /// Message utilisateur qui a produit cette version (brief ou ajustement).
    request: String,
    /// Bande son compagnon (« anim.html » → « anim.mp3 »).
    audio: Option<String>,
    /// Coût réel de la génération de cette version (USD), s'il est connu.
    cost: Option<f64>,
    /// Coût réel de sa bande son, si elle a été composée ici.
    audio_cost: Option<f64>,
}

fn file_name(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_string()
}

// --- Lecteur de l'aperçu ------------------------------------------------------------------
// L'aperçu tourne sous la même horloge virtuelle que l'export (`shim.js` du moteur) : ce qu'on
// voit est ce qui sera rendu, et on peut mettre en pause ou se placer à n'importe quel instant.

/// Horloge virtuelle du moteur (une seule source pour l'aperçu et le rendu).
const SHIM: &str = include_str!("../crates/rustmotion-engine/src/shim.js");

/// Pilote du lecteur, injecté après l'horloge : fait avancer le temps virtuel au rythme réel
/// pendant la lecture, et dialogue avec l'application par `postMessage`.
/// L'horloge ne recule pas : un retour en arrière est signalé (`rewind`) et l'aperçu est rechargé.
const PLAYER: &str = r#"(() => {
  const W = window, R = W.__rmReal, cfg = W.__rmPlayer;
  let t = 0, playing = false, base = 0, origin = 0, lastPost = 0, ready = false, q = Promise.resolve();
  const post = (m) => { try { parent.postMessage(Object.assign({ rm: 'player', token: cfg.token }, m), '*'); } catch (e) {} };
  const seekTo = (sec) => (q = q.then(async () => {
    sec = Math.min(Math.max(sec, t), cfg.dur);
    try { await W.__rm.seek(sec * 1000); } catch (e) {}
    t = sec;
  }));
  const status = (force) => {
    const n = R.now();
    if (force || n - lastPost > 80) { lastPost = n; post({ ev: 'time', t, playing }); }
  };
  const frame = async () => {
    if (!playing) return;
    const target = base + (R.now() - origin) / 1000;
    if (target >= cfg.dur) { await seekTo(cfg.dur); playing = false; status(true); return; }
    await seekTo(target);
    status(false);
    R.raf(frame);
  };
  const play = () => {
    if (playing) return;
    if (t >= cfg.dur - 0.001) { post({ ev: 'restart' }); return; }
    playing = true; base = t; origin = R.now(); status(true); R.raf(frame);
  };
  const pause = () => { playing = false; status(true); };
  W.addEventListener('message', async (e) => {
    const d = e.data;
    if (!d || d.rm !== 'cmd' || !ready) return;
    if (d.cmd === 'play') play();
    else if (d.cmd === 'pause') pause();
    else if (d.cmd === 'seek') {
      if (d.t < t - 0.0005) { post({ ev: 'rewind', t: d.t, playing }); return; }
      const was = playing; playing = false;
      await seekTo(d.t);
      status(true);
      if (was) play();
    }
  });
  const start = async () => {
    try { await W.__rm.init(false); } catch (e) {}
    ready = true;
    await seekTo(cfg.t);
    if (cfg.play) play(); else status(true);
  };
  if (document.readyState === 'complete') start(); else W.addEventListener('load', start);
})();"#;

/// Fin de la balise ouvrante `<name …>` (pas `<header>` pour `head`), si elle existe.
fn tag_end(lower: &str, name: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = lower[from..].find(&format!("<{name}")) {
        let at = from + i + name.len() + 1;
        if lower[at..].starts_with(|c: char| c == '>' || c.is_ascii_whitespace()) {
            return lower[at..].find('>').map(|e| at + e + 1);
        }
        from = at;
    }
    None
}

/// Document de l'aperçu : l'animation, précédée de l'horloge virtuelle et du pilote du lecteur
/// (au tout début de `<head>`, pour passer avant les scripts de la page).
fn player_doc(html: &str, start: f64, play: bool, duration: f64, token: u32) -> String {
    let scripts = format!(
        "<script>window.__rmReal={{raf:window.requestAnimationFrame.bind(window),now:performance.now.bind(performance)}};\
         window.__rmPlayer={{t:{start},play:{play},dur:{dur},token:{token}}};</script>\
         <script>{SHIM}</script><script>{PLAYER}</script>",
        dur = duration.max(0.04)
    );
    let lower = html.to_ascii_lowercase();
    let at = tag_end(&lower, "head").or_else(|| tag_end(&lower, "html")).or_else(|| tag_end(&lower, "!doctype")).unwrap_or(0);
    format!("{}{scripts}{}", &html[..at], &html[at..])
}

/// Temps du lecteur : « 0:03.2 ».
fn fmt_clock(t: f64) -> String {
    let t = t.max(0.0);
    let m = (t / 60.0).floor() as u64;
    format!("{m}:{:04.1}", t - m as f64 * 60.0)
}

/// Début de la « demande » d'une animation ouverte depuis un fichier (elle n'a pas de brief).
const IMPORTED: &str = "Existing animation";

/// Nom lisible d'un modèle musical.
fn music_label(model: &str) -> &str {
    match model {
        rustmotion_types::motion::MUSIC_CLIP_MODEL => "Lyria 3 Clip",
        rustmotion_types::motion::MUSIC_FULL_MODEL => "Lyria 3 Pro",
        other => other,
    }
}

/// Texte limité à `max` caractères.
fn clip_text(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() > max {
        s.chars().take(max).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

/// Format détecté par l'analyse de l'onglet Conversion pour le fichier courant.
#[derive(Clone, PartialEq)]
pub struct SourceFormat {
    pub path: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
}

fn fmt_secs(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 60 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

/// Construit l'onglet. `source` est l'animation de la barre commune : un HTML choisi ailleurs
/// s'affiche ici, et chaque version affichée ici y est envoyée par `on_file`.
/// `on_audio(html, audio)` signale une nouvelle bande son ; `on_convert` passe à l'onglet Conversion.
#[allow(clippy::too_many_arguments)]
pub fn motion_tab(
    lang: RwSignal<Lang>,
    active: Signal<bool>,
    dropped_doc: RwSignal<Option<String>>,
    dropped_audio: RwSignal<Option<String>>,
    ai: AiSettings,
    settings_open: RwSignal<bool>,
    browser_path: Signal<String>,
    source: Signal<String>,
    source_format: Signal<Option<SourceFormat>>,
    on_file: impl Fn(String) + Copy + Send + Sync + 'static,
    on_audio: impl Fn(String, String) + Copy + Send + Sync + 'static,
    on_convert: impl Fn() + Copy + Send + Sync + 'static,
    on_export: impl Fn() + Copy + Send + Sync + 'static,
    exporting: Signal<bool>,
    export_status: Signal<String>,
) -> impl IntoView {
    let in_tauri = tauri::available();
    let m = move || motion_texts(lang.get());

    // --- Brief et format -----------------------------------------------------
    let brief = RwSignal::new(String::new());
    let brief_file = RwSignal::new(None::<String>);
    let file_error = RwSignal::new(None::<String>);
    let loading_file = RwSignal::new(false);
    let width = RwSignal::new(1920u32);
    let height = RwSignal::new(1080u32);
    let duration = RwSignal::new(DEFAULT_DURATION);
    let durations_open = RwSignal::new(false);

    // --- Génération ----------------------------------------------------------
    let gen = RwSignal::new(Gen::Idle);
    let progress = RwSignal::new(None::<AiProgress>);
    let versions = RwSignal::new(Vec::<Version>::new());
    let current = RwSignal::new(0usize);
    let refine = RwSignal::new(String::new());
    let save_error = RwSignal::new(None::<String>);
    let replay = RwSignal::new(0u32);
    let pending = RwSignal::new(None::<Pending>);

    // --- Bande son -----------------------------------------------------------
    let music_style = RwSignal::new(load(KEY_MUSIC_STYLE, ""));
    let with_music = RwSignal::new(load(KEY_WITH_MUSIC, "0") == "1");
    // Par défaut, la bande son est écrite par le modèle de l'animation (choix mémorisé).
    let music_engine = RwSignal::new(if load(KEY_MUSIC_ENGINE, "same") == "lyria" { MusicEngine::Lyria } else { MusicEngine::Same });
    let music = RwSignal::new(MusicGen::Idle);
    // Total réellement facturé pendant la session (USD).
    let spent = RwSignal::new(0.0f64);
    let sound_on = RwSignal::new(load(KEY_SOUND, "1") == "1");
    // Bande son de la version affichée, en data URL pour l'aperçu.
    let audio_src = RwSignal::new(None::<String>);

    let version = move || versions.with(|v| v.get(current.get()).cloned());
    let running = move || gen.get() == Gen::Running;
    let music_running = move || matches!(music.get(), MusicGen::Running(_));
    // Le proxy ne traite qu'une génération à la fois (animation ou bande son).
    let busy = move || running() || music_running();

    let load_file = move |path: String| {
        #[derive(Serialize)]
        struct A {
            path: String,
        }
        loading_file.set(true);
        file_error.set(None);
        spawn_local(async move {
            match invoke::<_, PromptFile>("read_prompt_file", &A { path }).await {
                Ok(f) => {
                    brief.set(f.text);
                    brief_file.set(Some(f.name));
                }
                Err(e) => file_error.set(Some(e)),
            }
            loading_file.set(false);
        });
    };

    Effect::new(move |_| {
        if let Some(p) = dropped_doc.get() {
            dropped_doc.set(None);
            load_file(p);
        }
    });

    // Animation choisie dans la barre commune : elle devient l'animation affichée ici.
    Effect::new(move |_| {
        let path = source.get().trim().trim_matches('"').to_string();
        if path.is_empty() || gen.get_untracked() == Gen::Running || matches!(music.get_untracked(), MusicGen::Running(_)) {
            return;
        }
        // Déjà dans l'historique (version générée ici) : on l'affiche simplement.
        if let Some(i) = versions.with_untracked(|vs| vs.iter().position(|v| v.path.as_deref() == Some(path.as_str()))) {
            if current.get_untracked() != i {
                current.set(i);
            }
            return;
        }
        #[derive(Serialize)]
        struct A {
            path: String,
        }
        #[derive(Deserialize)]
        struct Opened {
            html: String,
            title: Option<String>,
            width: Option<u32>,
            height: Option<u32>,
            duration: Option<f64>,
            audio: Option<String>,
        }
        spawn_local(async move {
            let Ok(Some(o)) = invoke::<_, Option<Opened>>("open_motion", &A { path: path.clone() }).await else { return };
            // La source a changé entre-temps, ou une génération a démarré.
            if source.get_untracked().trim().trim_matches('"') != path || gen.get_untracked() == Gen::Running {
                return;
            }
            let sized = o.width.is_some() && o.height.is_some() && o.duration.is_some();
            let (w, h, d) = (
                o.width.unwrap_or_else(|| width.get_untracked()),
                o.height.unwrap_or_else(|| height.get_untracked()),
                o.duration.unwrap_or_else(|| duration.get_untracked()),
            );
            // Le format suit le fichier, pour que les ajustements ne le modifient pas par erreur.
            width.set(w);
            height.set(h);
            duration.set(d);
            // Nouvel historique : les ajustements partent de ce fichier, pas du brief précédent.
            let name = o.title.unwrap_or_else(|| path.rsplit(['/', '\\']).next().unwrap_or_default().to_string());
            versions.set(vec![Version {
                html: o.html,
                path: Some(path),
                truncated: false,
                model: None,
                width: w,
                height: h,
                duration: d,
                sized,
                request: format!("{IMPORTED} \"{name}\", opened from a file."),
                audio: o.audio,
                cost: None,
                audio_cost: None,
            }]);
            current.set(0);
            refine.set(String::new());
            gen.set(Gen::Idle);
        });
    });

    // Fichier sans balises `rustmotion:` : son format vient de l'analyse de l'onglet Conversion.
    Effect::new(move |_| {
        let Some(f) = source_format.get() else { return };
        let i = current.get_untracked();
        let target = versions.with_untracked(|vs| vs.get(i).is_some_and(|v| !v.sized && v.path.as_deref() == Some(f.path.as_str())));
        if !target {
            return;
        }
        let mut dims = (0, 0, 0.0);
        versions.update(|vs| {
            let v = &mut vs[i];
            v.width = f.width.unwrap_or(v.width);
            v.height = f.height.unwrap_or(v.height);
            v.duration = f.duration.unwrap_or(v.duration);
            v.sized = true;
            dims = (v.width, v.height, v.duration);
        });
        width.set(dims.0);
        height.set(dims.1);
        duration.set(dims.2);
    });

    // La version affichée ici devient l'animation de la barre commune.
    Effect::new(move |prev: Option<Option<String>>| {
        let path = versions.with(|vs| vs.get(current.get()).and_then(|v| v.path.clone()));
        if let Some(p) = path.clone().filter(|_| prev.as_ref() != Some(&path)) {
            on_file(p);
        }
        path
    });

    // Effacer le brief reste réversible quelques secondes (bouton « Annuler »).
    let cleared = RwSignal::new(None::<(String, Option<String>)>);
    let clear_gen = StoredValue::new(0u32);
    let clear_brief = move || {
        cleared.set(Some((brief.get_untracked(), brief_file.get_untracked())));
        brief.set(String::new());
        brief_file.set(None);
        clear_gen.update_value(|g| *g += 1);
        let g = clear_gen.get_value();
        set_timeout(
            move || {
                if clear_gen.get_value() == g {
                    cleared.set(None);
                }
            },
            std::time::Duration::from_secs(8),
        );
    };
    let undo_clear = move || {
        if let Some((text, file)) = cleared.get_untracked() {
            brief.set(text);
            brief_file.set(file);
        }
        cleared.set(None);
    };
    // Une nouvelle saisie rend l'annulation sans objet.
    Effect::new(move |_| {
        if !brief.with(String::is_empty) {
            cleared.set(None);
        }
    });

    let pick_brief = move || {
        #[derive(Serialize)]
        struct A {
            kind: &'static str,
        }
        spawn_local(async move {
            if let Ok(Some(p)) = invoke::<_, Option<String>>("pick_file", &A { kind: "prompt" }).await {
                load_file(p);
            }
        });
    };

    // Coût réel d'un appel : ajouté au cumul de la session.
    let add_spent = move |cost: Option<f64>| {
        if let Some(c) = cost {
            spent.update(|s| *s += c);
        }
    };

    // Rattache une bande son (ou aucune) à la version dont le fichier est `html`,
    // et la signale à la conversion. `cost` : prix réel si elle vient d'être composée.
    let set_version_audio = move |html: String, audio: Option<String>, cost: Option<f64>| {
        versions.update(|vs| {
            for v in vs.iter_mut().filter(|v| v.path.as_deref() == Some(html.as_str())) {
                v.audio = audio.clone();
                v.audio_cost = cost;
            }
        });
        on_audio(html, audio.unwrap_or_default());
    };

    // Retire la bande son de l'animation affichée (le fichier est renommé, pas supprimé).
    let detach = move || {
        let Some(html) = version().and_then(|v| v.path) else { return };
        #[derive(Serialize)]
        struct A {
            html_path: String,
        }
        spawn_local(async move {
            match invoke::<_, Option<String>>("detach_audio", &A { html_path: html.clone() }).await {
                Ok(moved) => {
                    set_version_audio(html, None, None);
                    music.set(match moved {
                        Some(p) => MusicGen::Removed(file_name(&p)),
                        None => MusicGen::Idle,
                    });
                }
                Err(e) => music.set(MusicGen::Error(e)),
            }
        });
    };

    // Utilise un fichier audio existant comme bande son de l'animation `html`.
    let attach = move |html: String, audio: String| {
        #[derive(Serialize)]
        struct A {
            html_path: String,
            audio_path: String,
        }
        spawn_local(async move {
            match invoke::<_, String>("attach_audio", &A { html_path: html.clone(), audio_path: audio }).await {
                Ok(p) => {
                    set_version_audio(html, Some(p), None);
                    music.set(MusicGen::Idle);
                }
                Err(e) => music.set(MusicGen::Error(e)),
            }
        });
    };

    // Compose la bande son de l'animation `html` : modèle musical (Lyria), ou modèle de
    // l'animation qui en écrit le code (rendu hors écran par le moteur).
    let run_music = move |html: String, plan: MusicPlan| {
        #[derive(Serialize)]
        struct Chat {
            base_url: String,
            api_key: Option<String>,
            model: String,
            messages: Vec<ChatMessage>,
        }
        #[derive(Serialize)]
        struct CodeReq {
            chat: Chat,
            html_path: String,
            duration: f64,
            browser_path: Option<String>,
        }
        #[derive(Serialize)]
        struct LyriaReq {
            base_url: String,
            api_key: Option<String>,
            model: String,
            prompt: String,
            html_path: String,
        }
        #[derive(Serialize)]
        struct A<R> {
            req: R,
        }
        let (base_url, api_key) = ai.auth();
        let call: (&'static str, Result<serde_json::Value, String>) = match plan.engine {
            MusicEngine::Lyria => {
                let prompt = soundtrack_prompt(&plan.context, plan.duration, &plan.style);
                let req = LyriaReq { base_url, api_key, model: plan.model, prompt, html_path: html.clone() };
                ("ai_soundtrack", serde_json::to_value(A { req }).map_err(|e| e.to_string()))
            }
            MusicEngine::Same => {
                // Le HTML de l'animation sert à caler la musique sur ses moments clés.
                let source = versions.with_untracked(|vs| {
                    vs.iter().find(|v| v.path.as_deref() == Some(html.as_str())).map(|v| v.html.clone())
                });
                let messages = vec![
                    ChatMessage { role: "system", content: music_code_system(plan.duration, &plan.style) },
                    ChatMessage { role: "user", content: music_code_message(&plan.context, &source.unwrap_or_default()) },
                ];
                let req = CodeReq {
                    chat: Chat { base_url, api_key, model: plan.model, messages },
                    html_path: html.clone(),
                    duration: plan.duration,
                    browser_path: Some(browser_path.get_untracked()).filter(|p| !p.trim().is_empty()),
                };
                ("ai_soundtrack_code", serde_json::to_value(A { req }).map_err(|e| e.to_string()))
            }
        };
        let (cmd, args) = call;
        let Ok(args) = args else { return };
        music.set(MusicGen::Running(0));
        let tick = set_interval_with_handle(
            move || music.update(|m| if let MusicGen::Running(s) = m { *s += 1 }),
            std::time::Duration::from_secs(1),
        )
        .ok();
        spawn_local(async move {
            #[derive(Deserialize)]
            struct MusicResult {
                path: String,
                cost: Option<f64>,
            }
            let res = invoke::<_, Option<MusicResult>>(cmd, &args).await;
            if let Some(t) = tick {
                t.clear();
            }
            match res {
                Ok(Some(r)) => {
                    add_spent(r.cost);
                    set_version_audio(html, Some(r.path), r.cost);
                    music.set(MusicGen::Idle);
                }
                Ok(None) => music.set(MusicGen::Cancelled),
                Err(e) => music.set(MusicGen::Error(e)),
            }
        });
    };

    // Coût estimé d'un appel au modèle de texte, d'après ses tarifs (inconnu sans la liste du proxy).
    let estimate_cost = move |model: &str, input_tokens: usize, output_tokens: usize| {
        ai.models.with_untracked(|r| {
            let info = r.as_ref()?.as_ref().ok()?.iter().find(|i| i.id == model)?;
            Some(input_tokens as f64 * info.prompt? + output_tokens as f64 * info.completion?)
        })
    };

    // Prépare la bande son selon le moteur choisi. `html_chars` : taille du HTML de l'animation
    // (lu par le modèle de l'animation pour se caler dessus).
    let plan_music = move |context: String, d: f64, html_chars: usize| {
        let style = music_style.get_untracked();
        match music_engine.get_untracked() {
            MusicEngine::Lyria => {
                let (model, cost) = music_model(d);
                MusicPlan { engine: MusicEngine::Lyria, model: model.to_string(), cost: Some(cost), duration: d, context, style }
            }
            MusicEngine::Same => {
                let model = ai.model.get_untracked().trim().to_string();
                let sent = music_code_system(d, &style).chars().count() + context.chars().count() + html_chars.min(40_000);
                let cost = estimate_cost(&model, estimate_tokens(sent), MUSIC_CODE_TOKENS);
                MusicPlan { engine: MusicEngine::Same, model, cost, duration: d, context, style }
            }
        }
    };

    // Envoie la conversation au proxy, enregistre le résultat et l'ajoute aux versions.
    // Ensuite : compose sa bande son (`music`), ou reprend celle de la version ajustée (`carry_audio`).
    let run = move |messages: Vec<ChatMessage>, request: String, music_plan: Option<MusicPlan>, carry_audio: Option<String>| {
        #[derive(Serialize)]
        struct Req {
            base_url: String,
            api_key: Option<String>,
            model: String,
            messages: Vec<ChatMessage>,
        }
        #[derive(Serialize)]
        struct A {
            req: Req,
        }
        let (base_url, api_key) = ai.auth();
        let a = A { req: Req { base_url, api_key, model: ai.model.get_untracked(), messages } };
        let (w, h, d) = (width.get_untracked(), height.get_untracked(), duration.get_untracked());
        gen.set(Gen::Running);
        progress.set(None);
        save_error.set(None);
        spawn_local(async move {
            match invoke::<_, Option<AiResult>>("ai_generate", &a).await {
                Ok(Some(r)) => {
                    add_spent(r.cost);
                    #[derive(Serialize)]
                    struct S {
                        html: String,
                    }
                    let path = match invoke::<_, String>("save_motion", &S { html: r.html.clone() }).await {
                        Ok(p) => Some(p),
                        Err(e) => {
                            save_error.set(Some(e));
                            None
                        }
                    };
                    versions.update(|v| {
                        v.push(Version {
                            html: r.html,
                            path: path.clone(),
                            truncated: r.truncated,
                            model: r.model,
                            cost: r.cost,
                            audio_cost: None,
                            width: w,
                            height: h,
                            duration: d,
                            sized: true,
                            request,
                            audio: None,
                        })
                    });
                    current.set(versions.with_untracked(|v| v.len() - 1));
                    refine.set(String::new());
                    gen.set(Gen::Idle);
                    if let Some(p) = path {
                        match (music_plan, carry_audio) {
                            (Some(plan), _) => run_music(p, plan),
                            (None, Some(a)) => attach(p, a),
                            _ => {}
                        }
                    }
                }
                Ok(None) => gen.set(Gen::Cancelled),
                Err(e) => gen.set(Gen::Error(e)),
            }
        });
    };

    // Toute génération est payante : on prépare la requête, on estime son coût,
    // et elle ne part qu'après confirmation explicite.
    let ask_confirm = move |messages: Vec<ChatMessage>,
                            request: String,
                            refine: bool,
                            output_tokens: usize,
                            music: Option<MusicPlan>,
                            carry_audio: Option<String>| {
        let model = ai.model.get_untracked().trim().to_string();
        let input_tokens = estimate_tokens(messages.iter().map(|m| m.content.chars().count()).sum());
        let price = ai.models.with_untracked(|r| {
            r.as_ref().and_then(|r| r.as_ref().ok()).and_then(|list| list.iter().find(|i| i.id == model)).map(|i| (i.prompt, i.completion))
        });
        let cost = match price {
            Some((Some(p), Some(c))) => Some(input_tokens as f64 * p + output_tokens as f64 * c),
            _ => None,
        };
        pending.set(Some(Pending::Animation {
            messages,
            request,
            refine,
            model,
            input_tokens,
            output_tokens,
            cost,
            music,
            carry_audio,
        }));
    };

    let confirm = move || {
        let Some(p) = pending.get_untracked() else { return };
        pending.set(None);
        match p {
            Pending::Animation { messages, request, music, carry_audio, .. } => run(messages, request, music, carry_audio),
            Pending::Music { html_path, plan } => run_music(html_path, plan),
        }
    };

    // Une demande en attente ne doit pas partir avec des réglages qui ont changé depuis.
    Effect::new(move |prev: Option<()>| {
        brief.track();
        refine.track();
        width.track();
        height.track();
        duration.track();
        ai.model.track();
        music_style.track();
        with_music.track();
        music_engine.track();
        current.track();
        if prev.is_some() {
            pending.set(None);
        }
    });

    let generate = move || {
        let b = brief.get_untracked();
        let (w, h, d) = (width.get_untracked(), height.get_untracked(), duration.get_untracked());
        let msg = brief_message(&b);
        // L'animation n'existe pas encore : on estime la taille de son HTML.
        let music = with_music
            .get_untracked()
            .then(|| plan_music(format!("Brief: {}", clip_text(&b, 1500)), d, EXPECTED_OUTPUT_TOKENS * 7 / 2));
        ask_confirm(
            vec![
                ChatMessage { role: "system", content: system_prompt(w, h, d) },
                ChatMessage { role: "user", content: msg },
            ],
            b,
            false,
            EXPECTED_OUTPUT_TOKENS,
            music,
            None,
        );
    };

    // Bande son seule, pour l'animation affichée : décrite par son brief et ses textes à l'écran.
    let compose_music = move || {
        let Some(v) = version() else { return };
        let Some(path) = v.path.clone() else { return };
        let first = versions.with_untracked(|vs| vs.first().map(|f| f.request.clone())).unwrap_or_default();
        let mut context = String::new();
        if let Some(rest) = first.strip_prefix(IMPORTED) {
            // Animation ouverte depuis un fichier : pas de brief, seulement son titre.
            let title = rest.split('"').nth(1).unwrap_or_default();
            if !title.is_empty() {
                context.push_str(&format!("Title: {title}\n"));
            }
        } else if !first.trim().is_empty() {
            context.push_str(&format!("Brief: {}\n", clip_text(&first, 1500)));
        }
        let text = visible_text(&v.html, 600);
        if !text.is_empty() {
            context.push_str(&format!("On-screen text: {text}"));
        }
        pending.set(Some(Pending::Music { html_path: path, plan: plan_music(context, v.duration, v.html.chars().count()) }));
    };

    let import_audio = move |audio: String| {
        if let Some(p) = version().and_then(|v| v.path) {
            attach(p, audio);
        }
    };
    let pick_audio = move || {
        #[derive(Serialize)]
        struct A {
            kind: &'static str,
        }
        spawn_local(async move {
            if let Ok(Some(p)) = invoke::<_, Option<String>>("pick_file", &A { kind: "audio" }).await {
                import_audio(p);
            }
        });
    };
    Effect::new(move |_| {
        if let Some(a) = dropped_audio.get() {
            dropped_audio.set(None);
            import_audio(a);
        }
    });

    let apply_refine = move || {
        let Some(v) = version() else { return };
        let instr = refine.get_untracked();
        let (w, h, d) = (width.get_untracked(), height.get_untracked(), duration.get_untracked());
        // Le brief d'origine reste le premier message ; la version affichée sert de base aux ajustements.
        let first = versions.with_untracked(|vs| vs.first().map(|f| f.request.clone())).unwrap_or_default();
        let mut extra = String::new();
        if (w, h) != (v.width, v.height) || (d - v.duration).abs() > 1e-6 {
            extra = format!(
                "\n\nAlso adapt the animation to the new format: {w}×{h} px, {d} seconds (update the rustmotion meta tags)."
            );
        }
        // Le modèle réécrit le document complet : la réponse fait à peu près la taille de la version actuelle.
        let output_tokens = estimate_tokens(v.html.chars().count()).max(EXPECTED_OUTPUT_TOKENS / 2);
        ask_confirm(
            vec![
                ChatMessage { role: "system", content: system_prompt(w, h, d) },
                ChatMessage { role: "user", content: brief_message(&first) },
                ChatMessage { role: "assistant", content: v.html },
                ChatMessage { role: "user", content: format!("{}{extra}", refine_message(&instr)) },
            ],
            instr,
            true,
            output_tokens,
            None,
            // La version ajustée garde la bande son de celle dont elle part.
            v.audio.clone(),
        );
    };

    let cancel = move || {
        #[derive(Serialize)]
        struct A {}
        spawn_local(async move {
            let _ = invoke::<_, ()>("ai_cancel", &A {}).await;
        });
    };

    if in_tauri {
        tauri::listen::<AiProgress>("ai-progress", move |p| {
            if gen.get_untracked() == Gen::Running {
                progress.set(Some(p));
            }
        });
    }

    // --- Aperçu : l'iframe garde la taille réelle de la vidéo, réduite pour tenir dans le cadre.
    let stage_ref = NodeRef::<html::Div>::new();
    let stage_size = RwSignal::new((0.0f64, 0.0f64));
    let measure = move || {
        if let Some(el) = stage_ref.get_untracked() {
            stage_size.set((el.client_width() as f64, el.client_height() as f64));
        }
    };
    let _ = window_event_listener(leptos::ev::resize, move |_| measure());
    Effect::new(move |_| {
        // Mesure après affichage de l'onglet ou changement de version.
        active.track();
        current.track();
        versions.track();
        set_timeout(measure, std::time::Duration::from_millis(30));
    });

    // --- Aperçu sonore : la bande son repart de zéro à chaque chargement de l'animation.
    let audio_ref = NodeRef::<html::Audio>::new();
    let version_audio = Memo::new(move |_| versions.with(|vs| vs.get(current.get()).and_then(|v| v.audio.clone())));
    Effect::new(move |_| {
        let Some(path) = version_audio.get() else {
            audio_src.set(None);
            return;
        };
        #[derive(Serialize)]
        struct A {
            path: String,
        }
        spawn_local(async move {
            if let Ok(url) = invoke::<_, String>("read_audio", &A { path: path.clone() }).await {
                // Le son rejoint l'animation au prochain point de synchronisation du lecteur.
                if version_audio.get_untracked().as_deref() == Some(path.as_str()) {
                    audio_src.set(Some(url));
                }
            }
        });
    });
    let stop_audio = move || {
        if let Some(el) = audio_ref.get_untracked() {
            let _ = el.pause();
        }
    };

    // --- Lecteur : l'animation tourne sous l'horloge virtuelle du moteur (comme à l'export).
    // `replay` sert de jeton : chaque valeur correspond à un chargement de l'aperçu.
    let frame_ref = NodeRef::<html::Iframe>::new();
    let player_t = RwSignal::new(0.0f64);
    let player_playing = RwSignal::new(false);
    let scrubbing = RwSignal::new(false);
    // Position et lecture au prochain chargement (après un retour en arrière, par exemple).
    let player_start = StoredValue::new((0.0f64, true));
    // Version affichée au dernier chargement : une autre version repart de zéro.
    let player_key = StoredValue::new(None::<(usize, usize, Option<String>)>);

    // Le son suit l'image : lancé, recalé ou arrêté à chaque nouvelle du lecteur.
    let sync_audio = move |t: f64, playing: bool| {
        let Some(el) = audio_ref.get_untracked() else { return };
        let has = audio_src.with_untracked(Option::is_some);
        if !(playing && has && sound_on.get_untracked() && active.get_untracked()) {
            let _ = el.pause();
            if has {
                el.set_current_time(t);
            }
            return;
        }
        if el.paused() {
            el.set_current_time(t);
            let _ = el.play();
        } else if (el.current_time() - t).abs() > 0.25 {
            el.set_current_time(t);
        }
    };
    let send = move |cmd: &'static str, t: f64| {
        #[derive(Serialize)]
        struct Cmd {
            rm: &'static str,
            cmd: &'static str,
            t: f64,
        }
        let Some(win) = frame_ref.get_untracked().and_then(|f| f.content_window()) else { return };
        if let Ok(msg) = serde_wasm_bindgen::to_value(&Cmd { rm: "cmd", cmd, t }) {
            let _ = win.post_message(&msg, "*");
        }
    };
    // Recharge l'aperçu à la position `t` (l'horloge virtuelle ne sait pas reculer).
    let restart = move |t: f64, play: bool| {
        player_start.set_value((t, play));
        player_t.set(t);
        replay.update(|r| *r += 1);
    };
    let rewind_gen = StoredValue::new(0u32);
    let _ = window_event_listener(leptos::ev::message, move |ev| {
        #[derive(Deserialize)]
        struct Msg {
            rm: String,
            token: u32,
            ev: String,
            #[serde(default)]
            t: f64,
            #[serde(default)]
            playing: bool,
        }
        let Ok(msg) = serde_wasm_bindgen::from_value::<Msg>(ev.data()) else { return };
        if msg.rm != "player" || msg.token != replay.get_untracked() {
            return;
        }
        match msg.ev.as_str() {
            "time" => {
                if !scrubbing.get_untracked() {
                    player_t.set(msg.t);
                }
                player_playing.set(msg.playing);
                sync_audio(msg.t, msg.playing);
            }
            "restart" => restart(0.0, true),
            "rewind" => {
                // Pendant un glissement de la timeline, seul le dernier retour en arrière recharge.
                rewind_gen.update_value(|g| *g += 1);
                let g = rewind_gen.get_value();
                let (t, playing) = (msg.t, msg.playing);
                set_timeout(
                    move || {
                        if rewind_gen.get_value() == g {
                            restart(t, playing);
                        }
                    },
                    std::time::Duration::from_millis(120),
                );
            }
            _ => {}
        }
    });
    let toggle_play = move || {
        if player_playing.get_untracked() {
            send("pause", 0.0);
        } else {
            send("play", 0.0);
        }
    };
    let seek = move |t: f64| send("seek", t);

    // Pas de son pendant une génération, ni quand l'onglet est masqué (la lecture s'arrête aussi).
    Effect::new(move |_| {
        if running() || !active.get() {
            stop_audio();
            if player_playing.get_untracked() {
                send("pause", 0.0);
            }
        }
    });
    let toggle_sound = move || {
        let on = !sound_on.get_untracked();
        sound_on.set(on);
        store(KEY_SOUND, if on { "1" } else { "0" });
        sync_audio(player_t.get_untracked(), on && player_playing.get_untracked());
    };

    let set_duration = move |s: String| {
        if let Ok(v) = s.trim().replace(',', ".").parse::<f64>() {
            if v > 0.0 {
                duration.set(v);
            }
        }
    };
    let can_call = move || in_tauri && !busy() && !ai.model.get().trim().is_empty();
    let can_generate = move || can_call() && !brief.get().trim().is_empty();
    let can_refine = move || can_call() && version().is_some() && !refine.get().trim().is_empty();
    // Bande son : il faut une animation enregistrée sur le disque.
    let has_file = move || version().is_some_and(|v| v.path.is_some());
    let can_music = move || in_tauri && !busy() && has_file();

    // --- Raccourcis : Ctrl+Entrée (générer / ajuster), Échap (annuler une confirmation),
    // Espace (rejouer, hors champ de saisie). Ils ouvrent la confirmation, sans jamais la valider.
    let _ = window_event_listener(leptos::ev::keydown, move |ev| {
        if !active.get_untracked() || ev.repeat() {
            return;
        }
        let focus = document().active_element();
        let focus_id = focus.as_ref().map(|e| e.id()).unwrap_or_default();
        let typing = focus.as_ref().is_some_and(|e| {
            matches!(e.tag_name().to_ascii_lowercase().as_str(), "input" | "textarea" | "select" | "button")
        });
        let key = ev.key();
        if key == "Escape" && pending.with_untracked(Option::is_some) {
            ev.prevent_default();
            pending.set(None);
        } else if key == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            ev.prevent_default();
            if pending.with_untracked(Option::is_some) {
                return;
            }
            // Dans le champ d'ajustements, ou s'il est rempli : ajuster ; sinon : générer depuis le brief.
            let want_refine = focus_id == "refine" || (focus_id != "brief" && !refine.get_untracked().trim().is_empty());
            if want_refine && can_refine() {
                apply_refine();
            } else if !want_refine && can_generate() {
                generate();
            }
        } else if key == " " && !typing && !ev.ctrl_key() && version().is_some() && !running() {
            ev.prevent_default();
            toggle_play();
        }
    });

    view! {
        <main class="grid motion" class:hidden=move || !active.get()>
            <section class="col settings">
                // ---------------- Brief
                <div class="card">
                    <div class="card-h"><span class="step">"1"</span><h2>{move || m().brief}</h2></div>
                    <textarea id="brief" class="brief" rows="10" placeholder=move || m().brief_placeholder
                        prop:value=move || brief.get()
                        on:input=move |ev| { brief.set(event_target_value(&ev)); brief_file.set(None); } />
                    <div class="row">
                        <button class="btn" on:click=move |_| pick_brief() disabled=move || loading_file.get() || !in_tauri>
                            {icon(Icon::Upload)}{move || m().btn_import}
                        </button>
                        {move || {
                            let l = lang.get();
                            if loading_file.get() {
                                return view! { <span class="chip"><span class="spin"></span>{motion_texts(l).reading}</span> }.into_any();
                            }
                            if let Some(e) = file_error.get() {
                                return view! { <span class="chip err">{e}</span> }.into_any();
                            }
                            let n = brief.with(|b| b.chars().count());
                            match brief_file.get() {
                                Some(name) => view! {
                                    <span class="chip ok">{name}</span>
                                    <span class="hint">{i18n::chars(l, n)}</span>
                                }.into_any(),
                                None if n > 0 => view! { <span class="hint">{i18n::chars(l, n)}</span> }.into_any(),
                                // Brief tout juste effacé : on peut le récupérer quelques secondes.
                                None if cleared.with(Option::is_some) => view! {
                                    <span class="hint">{motion_texts(l).brief_cleared}</span>
                                    <button class="link undo" on:click=move |_| undo_clear()>{motion_texts(l).undo}</button>
                                }.into_any(),
                                None => view! { <span class="hint">{motion_texts(l).import_hint}</span> }.into_any(),
                            }
                        }}
                        <Show when=move || !brief.get().is_empty()>
                            <button class="btn ghost icon small push" title=move || m().clear aria-label=move || m().clear
                                on:click=move |_| clear_brief()>{icon(Icon::Trash)}</button>
                        </Show>
                    </div>
                </div>

                // ---------------- Format
                <div class="card">
                    <div class="card-h"><span class="step">"2"</span><h2>{move || m().format}</h2></div>
                    <div class="seg">
                        {PRESETS.into_iter().map(|(name, w, h)| view! {
                            <button class="pill" class:active=move || width.get() == w && height.get() == h
                                on:click=move |_| { width.set(w); height.set(h); }>
                                <b>{name}</b><small>{format!("{w}×{h}")}</small>
                            </button>
                        }).collect_view()}
                    </div>
                    <div class="fields three">
                        <div class="field">
                            <label class="lbl">{move || m().width}</label>
                            <input type="number" min="16" max="8192" prop:value=move || width.get()
                                on:change=move |ev| if let Ok(v) = event_target_value(&ev).parse() { width.set(v) } />
                        </div>
                        <div class="field">
                            <label class="lbl">{move || m().height}</label>
                            <input type="number" min="16" max="8192" prop:value=move || height.get()
                                on:change=move |ev| if let Ok(v) = event_target_value(&ev).parse() { height.set(v) } />
                        </div>
                        // Durée : saisie libre, avec une liste de durées courantes proposée (▾).
                        <div class="field">
                            <label class="lbl" for="duration">{move || m().duration_s}</label>
                            <div class="combo" on:keydown=move |ev| if ev.key() == "Escape" && durations_open.get_untracked() {
                                ev.stop_propagation();
                                durations_open.set(false);
                            }>
                                <input id="duration" type="number" min="1" max="600" step="0.5" prop:value=move || duration.get()
                                    on:change=move |ev| set_duration(event_target_value(&ev)) />
                                <button class="combo-btn" aria-haspopup="listbox" aria-expanded=move || durations_open.get().to_string()
                                    title=move || m().duration_choices aria-label=move || m().duration_choices
                                    on:click=move |_| durations_open.update(|o| *o = !*o)>{icon(Icon::ChevronDown)}</button>
                                <Show when=move || durations_open.get()>
                                    <div class="menu-scrim" on:click=move |_| durations_open.set(false)></div>
                                    <div class="combo-menu" role="listbox" aria-label=move || m().duration_choices>
                                        {DURATIONS.into_iter().map(|d| view! {
                                            <button role="option" class:active=move || (duration.get() - d).abs() < 1e-6
                                                aria-selected=move || ((duration.get() - d).abs() < 1e-6).to_string()
                                                on:click=move |_| { duration.set(d); durations_open.set(false); }>
                                                {format!("{d} s")}
                                            </button>
                                        }).collect_view()}
                                    </div>
                                </Show>
                            </div>
                        </div>
                    </div>
                    // Format modifié par rapport à l'animation affichée : appliqué au prochain ajustement.
                    {move || version().filter(|v| {
                        (width.get(), height.get()) != (v.width, v.height) || (duration.get() - v.duration).abs() > 1e-6
                    }).map(|v| view! {
                        <div class="format-diff">
                            <span class="chip warn">{i18n::format_changed(lang.get(), v.width, v.height, v.duration)}</span>
                            <button class="link" on:click=move |_| { width.set(v.width); height.set(v.height); duration.set(v.duration); }>
                                {move || m().restore}
                            </button>
                        </div>
                    })}
                </div>

                // ---------------- Bande son
                <div class="card">
                    <div class="card-h">
                        <span class="step">"3"</span><h2>{move || m().music}</h2>
                        <span class="card-meta">{move || match music_engine.get() {
                            MusicEngine::Lyria => music_label(music_model(version().map(|v| v.duration).unwrap_or(duration.get())).0).to_string(),
                            MusicEngine::Same => ai.model.get(),
                        }}</span>
                    </div>
                    // État : bande son de l'animation affichée, composition en cours, erreur.
                    <div class="music-state">
                        {move || {
                            let l = lang.get();
                            let tx = motion_texts(l);
                            match music.get() {
                                MusicGen::Running(s) => view! {
                                    <span class="chip"><span class="spin"></span>{tx.music_running}<em>{fmt_secs(s as u64 * 1000)}</em></span>
                                    <button class="btn ghost icon small push" title=tx.cancel aria-label=tx.cancel
                                        on:click=move |_| cancel()>{icon(Icon::Close)}</button>
                                }.into_any(),
                                state => {
                                    let current_audio = version().and_then(|v| v.audio);
                                    view! {
                                        {match current_audio {
                                            Some(a) => {
                                                let cost = version().and_then(|v| v.audio_cost)
                                                    .map(|c| format!(" · {}", fmt_cost(l, c))).unwrap_or_default();
                                                view! {
                                                    <span class="chip ok music-file" title=a.clone()>"♪ "{file_name(&a)}{cost}</span>
                                                    <button class="btn ghost icon small push" title=format!("{} — {}", tx.music_remove, tx.music_remove_title)
                                                        aria-label=tx.music_remove disabled=move || busy()
                                                        on:click=move |_| detach()>{icon(Icon::Trash)}</button>
                                                }.into_any()
                                            }
                                            None if version().is_some() => view! { <span class="hint">{tx.music_none}</span> }.into_any(),
                                            None => view! { <span class="hint">{tx.music_need_anim}</span> }.into_any(),
                                        }}
                                        {match state {
                                            MusicGen::Error(e) => Some(view! { <div class="result err"><strong>{tx.music_failed}</strong><pre>{e}</pre></div> }.into_any()),
                                            MusicGen::Cancelled => Some(view! { <span class="hint">{tx.music_cancelled}</span> }.into_any()),
                                            MusicGen::Removed(name) => Some(view! { <span class="hint">{i18n::music_removed(l, &name)}</span> }.into_any()),
                                            _ => None,
                                        }}
                                    }.into_any()
                                }
                            }
                        }}
                    </div>
                    // Qui compose : le modèle de l'animation (code audio synchronisé) ou Lyria 3 (musique réaliste).
                    <div class="seg tight engine" role="radiogroup" aria-label=move || m().music_engine>
                        {[MusicEngine::Same, MusicEngine::Lyria].into_iter().map(|e| view! {
                            <button class="pill" role="radio" class:active=move || music_engine.get() == e
                                aria-checked=move || (music_engine.get() == e).to_string()
                                disabled=move || music_running()
                                on:click=move |_| { music_engine.set(e); store(KEY_MUSIC_ENGINE, e.key()); }>
                                <b>{move || if e == MusicEngine::Same { m().engine_same } else { m().engine_lyria }}</b>
                                <small>{move || if e == MusicEngine::Same { m().engine_same_hint } else { m().engine_lyria_hint }}</small>
                            </button>
                        }).collect_view()}
                    </div>
                    // Confirmation d'une bande son seule (payante).
                    {move || {
                        let l = lang.get();
                        let tx = motion_texts(l);
                        let Some(Pending::Music { plan, .. }) = pending.get() else { return None };
                        Some(view! {
                            <div class="confirm" role="alertdialog" aria-labelledby="music-confirm-title">
                                <strong id="music-confirm-title">{tx.confirm_music_title}</strong>
                                <dl class="confirm-list">
                                    <div><dt>{tx.confirm_model}</dt><dd>{music_label(&plan.model).to_string()}</dd></div>
                                    <div><dt>{tx.duration_s}</dt><dd>{format!("{}", plan.duration)}</dd></div>
                                    <div class="cost span2"><dt>{tx.confirm_cost}</dt>
                                        <dd>{plan.cost.map(|c| fmt_usd(l, c)).unwrap_or_else(|| tx.confirm_cost_unknown.to_string())}</dd></div>
                                </dl>
                                <details class="prompt">
                                    <summary>{tx.confirm_prompt}</summary>
                                    <pre>{plan.preview()}</pre>
                                </details>
                                <p class="hint small">{if plan.engine == MusicEngine::Same { tx.confirm_music_code_note } else { tx.confirm_music_note }}</p>
                                <div class="row">
                                    <button class="btn grow" on:click=move |_| pending.set(None)>{tx.confirm_no}</button>
                                    <button class="btn primary grow" on:click=move |_| confirm()>{tx.confirm_music_yes}</button>
                                </div>
                            </div>
                        })
                    }}
                    <div class="row">
                        <button class="btn grow" on:click=move |_| pick_audio() disabled=move || !can_music()
                            title=move || if has_file() { m().music_import_title } else { m().music_need_anim }>
                            {icon(Icon::Upload)}{move || m().music_import}
                        </button>
                        <button class="btn grow" on:click=move |_| compose_music()
                            disabled=move || !can_music() || matches!(pending.get(), Some(Pending::Music { .. }))
                            title=move || if has_file() { "" } else { m().music_need_anim }>
                            {icon(Icon::Sparkles)}
                            {move || if version().is_some_and(|v| v.audio.is_some()) { m().music_recompose } else { m().music_compose }}
                        </button>
                    </div>
                    <div class="field">
                        <label class="lbl" for="music-style">{move || m().music_style}</label>
                        <input id="music-style" type="text" spellcheck="false" placeholder=move || m().music_style_placeholder
                            prop:value=move || music_style.get()
                            on:change=move |ev| {
                                let v = event_target_value(&ev).trim().to_string();
                                store(KEY_MUSIC_STYLE, &v);
                                music_style.set(v);
                            } />
                    </div>
                    <label class="toggle">
                        <input type="checkbox" prop:checked=move || with_music.get()
                            on:change=move |ev| {
                                let on = event_target_checked(&ev);
                                store(KEY_WITH_MUSIC, if on { "1" } else { "0" });
                                with_music.set(on);
                            } />
                        <span class="sw"></span>
                        <span>{move || m().music_with}<small>{move || if music_engine.get() == MusicEngine::Same { m().music_with_hint_same } else { m().music_with_hint }}</small></span>
                    </label>
                </div>

            </section>

            // ---------------- Aperçu + génération
            <section class="col side">
                <div class="card preview-card">
                    <div class="viewport mstage" node_ref=stage_ref
                        style=move || {
                            let (w, h) = version().map(|v| (v.width, v.height)).unwrap_or((width.get(), height.get()));
                            format!("aspect-ratio: {} / {}", w.max(1), h.max(1))
                        }>
                        {move || {
                            let l = lang.get();
                            if running() {
                                let p = progress.get();
                                let status = match &p {
                                    Some(p) if p.chars > 0 => i18n::writing(l, p.chars),
                                    Some(p) if p.reasoning > 0 => motion_texts(l).thinking.to_string(),
                                    _ => motion_texts(l).waiting.to_string(),
                                };
                                return view! {
                                    <div class="stream">
                                        <div class="stream-h"><span class="spin"></span><span>{status}</span>
                                            <em>{p.as_ref().map(|p| fmt_secs(p.elapsed_ms)).unwrap_or_default()}</em></div>
                                        <pre>{p.map(|p| p.tail).unwrap_or_default()}</pre>
                                    </div>
                                }.into_any();
                            }
                            // Chaque chargement porte son jeton (`replay`) : Leptos ne réécrit `srcdoc` que si
                            // le document change, et les messages d'un ancien chargement sont ignorés.
                            let token = replay.get();
                            match version() {
                                Some(v) => {
                                    let (w, h) = (v.width as f64, v.height as f64);
                                    // Une autre version repart du début ; sinon, reprise à la position demandée.
                                    let key = (current.get_untracked(), versions.with_untracked(Vec::len), v.path.clone());
                                    let (t0, play) = if player_key.with_value(|k| k.as_ref() != Some(&key)) {
                                        player_key.set_value(Some(key));
                                        player_start.set_value((0.0, true));
                                        (0.0, true)
                                    } else {
                                        player_start.get_value()
                                    };
                                    let doc = player_doc(&v.html, t0, play, v.duration, token);
                                    view! {
                                        <iframe class="mframe" sandbox="allow-scripts" srcdoc=doc node_ref=frame_ref
                                            title=motion_texts(l).preview
                                            style=move || {
                                                // Ajusté et centré : le cadre peut être plafonné en hauteur (formats verticaux).
                                                let (cw, ch) = stage_size.get();
                                                let s = if cw > 0.0 && ch > 0.0 { (cw / w).min(ch / h) } else { 0.0 };
                                                format!(
                                                    "width:{w}px;height:{h}px;transform:translate({}px,{}px) scale({s})",
                                                    (cw - w * s) / 2.0, (ch - h * s) / 2.0
                                                )
                                            }></iframe>
                                    }.into_any()
                                }
                                None => view! { <div class="vp-empty">{motion_texts(l).preview_empty}</div> }.into_any(),
                            }
                        }}
                        <span class="vp-tag">{move || {
                            let (w, h, d) = version().map(|v| (v.width, v.height, v.duration))
                                .unwrap_or((width.get(), height.get(), duration.get()));
                            format!("{w}×{h} · {d} s")
                        }}</span>
                    </div>
                    <audio node_ref=audio_ref preload="auto" src=move || audio_src.get().unwrap_or_default()
                        on:timeupdate=move |_| {
                            // La bande son s'arrête avec l'animation (le clip peut être plus long).
                            if let (Some(el), Some(v)) = (audio_ref.get_untracked(), version()) {
                                if el.current_time() >= v.duration {
                                    let _ = el.pause();
                                }
                            }
                        }></audio>
                    <Show when=move || version().is_some() && !running()>
                        // Lecteur : lecture / pause, position, temps, retour au début, son.
                        <div class="player">
                            <button class="btn pp" aria-pressed=move || player_playing.get().to_string()
                                title=move || if player_playing.get() { m().pause } else { m().play }
                                on:click=move |_| toggle_play()>
                                {move || if player_playing.get() { icon(Icon::Pause).into_any() } else { icon(Icon::Play).into_any() }}
                            </button>
                            <input type="range" class="timeline" min="0" step="0.01" aria-label=move || m().timeline
                                max=move || version().map(|v| v.duration).unwrap_or(1.0)
                                prop:value=move || player_t.get()
                                style=move || {
                                    let d = version().map(|v| v.duration).unwrap_or(1.0).max(0.001);
                                    format!("--p:{:.3}%", (player_t.get() / d * 100.0).clamp(0.0, 100.0))
                                }
                                on:input=move |ev| {
                                    if let Ok(t) = event_target_value(&ev).parse::<f64>() {
                                        scrubbing.set(true);
                                        player_t.set(t);
                                        seek(t);
                                    }
                                }
                                on:change=move |_| scrubbing.set(false) />
                            <span class="ptime">{move || format!("{} / {}",
                                fmt_clock(player_t.get()), fmt_clock(version().map(|v| v.duration).unwrap_or(0.0)))}</span>
                            <button class="btn ghost icon" title=move || m().replay_title aria-label=move || m().replay_title
                                on:click=move |_| restart(0.0, true)>{icon(Icon::Restart)}</button>
                            <Show when=move || audio_src.with(Option::is_some)>
                                <button class="btn ghost icon sound" class:off=move || !sound_on.get()
                                    title=move || if sound_on.get() { m().sound_mute } else { m().sound_unmute }
                                    aria-label=move || if sound_on.get() { m().sound_mute } else { m().sound_unmute }
                                    on:click=move |_| toggle_sound()>
                                    {move || if sound_on.get() { icon(Icon::Volume).into_any() } else { icon(Icon::VolumeOff).into_any() }}
                                </button>
                            </Show>
                        </div>
                        <div class="row">
                            // Bande de versions : la demande de chacune en infobulle, ♪ si elle a une bande son.
                            {move || {
                                let l = lang.get();
                                let list = versions.get();
                                (list.len() > 1).then(|| view! {
                                    <div class="versions" role="tablist" aria-label=motion_texts(l).versions>
                                        {list.into_iter().enumerate().map(|(i, v)| {
                                            let mut tip = if v.request.starts_with(IMPORTED) {
                                                motion_texts(l).opened_file.to_string()
                                            } else {
                                                clip_text(&v.request, 220)
                                            };
                                            if let Some(c) = v.cost {
                                                tip.push_str(&format!("\n{}", fmt_cost(l, c)));
                                            }
                                            view! {
                                                <button class="vchip" role="tab" class:active=move || current.get() == i
                                                    aria-selected=move || (current.get() == i).to_string()
                                                    title=tip on:click=move |_| current.set(i)>
                                                    {format!("v{}", i + 1)}
                                                    {v.audio.is_some().then(|| view! { <i>"♪"</i> })}
                                                </button>
                                            }
                                        }).collect_view()}
                                    </div>
                                })
                            }}
                            <span class="grow"></span>
                            {move || version().map(|v| {
                                let cost = v.cost.map(|c| fmt_cost(lang.get(), c));
                                let label = [v.model, cost].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                                (!label.is_empty()).then(|| view! { <span class="hint small" title=move || m().cost_real>{label}</span> })
                            })}
                        </div>
                    </Show>
                </div>

                <div class="card render-card">
                    // Modèle utilisé (modifiable dans les réglages) et dépense réelle de la session.
                    <div class="model-line">
                        <span class="model-name" title=move || ai.model.get()>
                            {move || m().model_label}" "<b>{move || ai.model.get()}</b>
                        </span>
                        <Show when=move || { spent.get() > 0.0 }>
                            <span class="spent">{move || i18n::session_spent(lang.get(), &fmt_cost(lang.get(), spent.get()))}</span>
                        </Show>
                        <button class="btn ghost icon small push" title=move || m().change_model aria-label=move || m().change_model
                            on:click=move |_| settings_open.set(true)>{icon(Icon::Settings)}</button>
                    </div>
                    {move || {
                        let l = lang.get();
                        let tx = motion_texts(l);
                        if running() {
                            return view! {
                                <button class="btn danger wide" on:click=move |_| cancel()>{icon(Icon::Close)}{tx.cancel}</button>
                            }.into_any();
                        }
                        if let Some(Pending::Animation { refine, model, input_tokens, output_tokens, cost, music, .. }) = pending.get() {
                            // Total : animation (estimée) + bande son (prix fixe).
                            let total = match (cost, music.as_ref().map(|mp| mp.cost)) {
                                (Some(c), None) => fmt_usd(l, c),
                                (Some(c), Some(Some(mc))) => fmt_usd(l, c + mc),
                                (None, None) => tx.confirm_cost_unknown.to_string(),
                                (Some(c), Some(None)) => format!("{} + {}", fmt_usd(l, c), tx.confirm_cost_unknown),
                                (None, Some(Some(mc))) => format!("{} + {}", tx.confirm_cost_unknown, fmt_usd(l, mc)),
                                (None, Some(None)) => tx.confirm_cost_unknown.to_string(),
                            };
                            return view! {
                                <div class="confirm" role="alertdialog" aria-labelledby="confirm-title">
                                    <strong id="confirm-title">{if refine { tx.confirm_refine_title } else { tx.confirm_title }}</strong>
                                    <dl class="confirm-list">
                                        <div><dt>{tx.confirm_model}</dt><dd>{model}</dd></div>
                                        <div><dt>{tx.confirm_sent}</dt><dd>{fmt_tokens(l, input_tokens)}</dd></div>
                                        <div><dt>{tx.confirm_reply}</dt><dd>{fmt_tokens(l, output_tokens)}</dd></div>
                                        <div class="cost"><dt>{tx.confirm_cost}</dt><dd>{total}</dd></div>
                                        {music.map(|mp| view! {
                                            <div class="span2"><dt>{tx.music}</dt>
                                                <dd>{format!("{} · {}", music_label(&mp.model),
                                                    mp.cost.map(|c| fmt_usd(l, c)).unwrap_or_else(|| tx.confirm_cost_unknown.to_string()))}</dd></div>
                                        })}
                                    </dl>
                                    <p class="hint small">{tx.confirm_note}</p>
                                    <div class="row">
                                        <button class="btn grow" on:click=move |_| pending.set(None)>{tx.confirm_no}</button>
                                        <button class="btn primary grow" on:click=move |_| confirm()>{tx.confirm_yes}</button>
                                    </div>
                                </div>
                            }.into_any();
                        }
                        let status = match gen.get() {
                            Gen::Error(e) => Some(view! { <div class="result err"><strong>{tx.failed}</strong><pre>{e}</pre></div> }.into_any()),
                            Gen::Cancelled => Some(view! { <div class="result"><strong>{tx.cancelled}</strong></div> }.into_any()),
                            _ => None,
                        };
                        match version() {
                            None => view! {
                                {status}
                                <button class="btn primary big wide" title=tx.shortcut_run on:click=move |_| generate() disabled=move || !can_generate()>
                                    {icon(Icon::Sparkles)}{tx.generate}
                                </button>
                                <p class="hint center">{move || {
                                    let tx = motion_texts(lang.get());
                                    if !in_tauri { tx.outside_tauri }
                                    else if brief.get().trim().is_empty() { tx.need_brief }
                                    else if ai.model.get().trim().is_empty() { tx.need_model }
                                    else { tx.ready_hint }
                                }}</p>
                            }.into_any(),
                            Some(v) => {
                                let saved = v.path.is_some();
                                view! {
                                    {status}
                                    {v.truncated.then(|| view! { <div class="result warn"><strong>{tx.truncated}</strong></div> })}
                                    {(!saved).then(|| view! { <div class="result err"><strong>{tx.save_failed}</strong>
                                        <pre>{save_error.get().unwrap_or_default()}</pre></div> })}
                                    // Export direct avec les réglages courants ; l'onglet Conversion pour les ajuster.
                                    <button class="btn primary big wide" disabled=move || !saved || exporting.get()
                                        on:click=move |_| on_export()>
                                        {icon(Icon::Video)}{move || if exporting.get() { m().exporting_btn } else { m().export_now }}
                                    </button>
                                    <p class="hint center export-line">
                                        <span>{move || export_status.get()}</span>
                                        <button class="link" on:click=move |_| on_convert()>{tx.export_settings}</button>
                                    </p>
                                    <div class="refine">
                                        <label class="lbl" for="refine">{tx.refine}</label>
                                        <textarea id="refine" rows="3" placeholder=tx.refine_placeholder
                                            prop:value=move || refine.get()
                                            on:input=move |ev| refine.set(event_target_value(&ev)) />
                                        <div class="row">
                                            <button class="btn grow" title=tx.shortcut_run on:click=move |_| apply_refine() disabled=move || !can_refine()>
                                                {icon(Icon::Wand)}{tx.refine_btn}
                                            </button>
                                            <button class="btn ghost" on:click=move |_| generate() disabled=move || !can_generate()
                                                title=tx.regenerate_title>
                                                {icon(Icon::Refresh)}{tx.regenerate}
                                            </button>
                                        </div>
                                    </div>
                                }.into_any()
                            }
                        }
                    }}
                </div>
            </section>
        </main>
    }
}
