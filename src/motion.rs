//! Onglet « Motion design » : un brief (saisi, ou lu dans un PDF / .txt) est envoyé à l'IA
//! via le proxy OpenRouter ; l'animation HTML produite est prévisualisée, ajustable,
//! puis transmise à l'onglet de conversion vidéo.

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use rustmotion_types::motion::{brief_message, music_model, refine_message, soundtrack_prompt, system_prompt, visible_text};
use rustmotion_types::Lang;
use serde::{Deserialize, Serialize};

use crate::app::PRESETS;
use crate::i18n::{self, motion_texts};
use crate::tauri::{self, invoke};

const KEY_URL: &str = "rustmotion.ai.url";
const KEY_MODEL: &str = "rustmotion.ai.model";
const KEY_KEY: &str = "rustmotion.ai.key";
const KEY_MUSIC_STYLE: &str = "rustmotion.music.style";
const KEY_WITH_MUSIC: &str = "rustmotion.music.auto";
const KEY_SOUND: &str = "rustmotion.preview.sound";
const DEFAULT_URL: &str = "http://127.0.0.1:9018/v1";
const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-5.5";

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
}

#[derive(Deserialize, Clone)]
struct AiProgress {
    chars: usize,
    reasoning: usize,
    tail: String,
    elapsed_ms: u64,
}

/// Modèle proposé par le proxy, avec son tarif (USD par jeton) quand il est connu.
#[derive(Deserialize, Clone)]
struct ModelInfo {
    id: String,
    prompt: Option<f64>,
    completion: Option<f64>,
}

/// Bande son à composer : modèle musical, prix fixe (USD) et demande envoyée.
#[derive(Clone)]
struct MusicPlan {
    model: &'static str,
    cost: f64,
    duration: f64,
    prompt: String,
}

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
}

fn file_name(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_string()
}

/// Début de la « demande » d'une animation ouverte depuis un fichier (elle n'a pas de brief).
const IMPORTED: &str = "Existing animation";

/// Nom lisible d'un modèle musical.
fn music_label(model: &str) -> &'static str {
    if model == rustmotion_types::motion::MUSIC_CLIP_MODEL {
        "Lyria 3 Clip"
    } else {
        "Lyria 3 Pro"
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

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn load(key: &str, default: &str) -> String {
    storage().and_then(|s| s.get_item(key).ok().flatten()).unwrap_or_else(|| default.to_string())
}

fn store(key: &str, value: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, value);
    }
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
    source: Signal<String>,
    source_format: Signal<Option<SourceFormat>>,
    on_file: impl Fn(String) + Copy + Send + Sync + 'static,
    on_audio: impl Fn(String, String) + Copy + Send + Sync + 'static,
    on_convert: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let in_tauri = tauri::available();
    let m = move || motion_texts(lang.get());

    // --- Réglages IA (mémorisés) ---------------------------------------------
    let ai_url = RwSignal::new(load(KEY_URL, DEFAULT_URL));
    let ai_model = RwSignal::new(load(KEY_MODEL, DEFAULT_MODEL));
    let ai_key = RwSignal::new(load(KEY_KEY, ""));
    let models = RwSignal::new(None::<Result<Vec<ModelInfo>, String>>);
    let show_ai = RwSignal::new(false);

    // --- Brief et format -----------------------------------------------------
    let brief = RwSignal::new(String::new());
    let brief_file = RwSignal::new(None::<String>);
    let file_error = RwSignal::new(None::<String>);
    let loading_file = RwSignal::new(false);
    let width = RwSignal::new(1920u32);
    let height = RwSignal::new(1080u32);
    let duration = RwSignal::new(8.0f64);

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
    let music = RwSignal::new(MusicGen::Idle);
    let sound_on = RwSignal::new(load(KEY_SOUND, "1") == "1");
    // Bande son de la version affichée, en data URL pour l'aperçu.
    let audio_src = RwSignal::new(None::<String>);

    let version = move || versions.with(|v| v.get(current.get()).cloned());
    let running = move || gen.get() == Gen::Running;
    let music_running = move || matches!(music.get(), MusicGen::Running(_));
    // Le proxy ne traite qu'une génération à la fois (animation ou bande son).
    let busy = move || running() || music_running();

    let refresh_models = move || {
        if !in_tauri {
            return;
        }
        #[derive(Serialize)]
        struct A {
            base_url: String,
            api_key: Option<String>,
        }
        let key = ai_key.get_untracked();
        let a = A { base_url: ai_url.get_untracked(), api_key: (!key.trim().is_empty()).then_some(key) };
        models.set(None);
        spawn_local(async move {
            models.set(Some(invoke::<_, Vec<ModelInfo>>("ai_models", &a).await));
        });
    };
    refresh_models();

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

    let ai_auth = move || {
        let key = ai_key.get_untracked();
        (ai_url.get_untracked(), (!key.trim().is_empty()).then_some(key))
    };

    // Rattache une bande son à la version dont le fichier est `html`, et la signale à la conversion.
    let set_version_audio = move |html: String, audio: String| {
        versions.update(|vs| {
            for v in vs.iter_mut().filter(|v| v.path.as_deref() == Some(html.as_str())) {
                v.audio = Some(audio.clone());
            }
        });
        on_audio(html, audio);
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
                    set_version_audio(html, p);
                    music.set(MusicGen::Idle);
                }
                Err(e) => music.set(MusicGen::Error(e)),
            }
        });
    };

    // Compose la bande son de l'animation `html` avec le modèle musical.
    let run_music = move |html: String, plan: MusicPlan| {
        #[derive(Serialize)]
        struct Req {
            base_url: String,
            api_key: Option<String>,
            model: &'static str,
            prompt: String,
            html_path: String,
        }
        #[derive(Serialize)]
        struct A {
            req: Req,
        }
        let (base_url, api_key) = ai_auth();
        let a = A { req: Req { base_url, api_key, model: plan.model, prompt: plan.prompt, html_path: html.clone() } };
        music.set(MusicGen::Running(0));
        let tick = set_interval_with_handle(
            move || music.update(|m| if let MusicGen::Running(s) = m { *s += 1 }),
            std::time::Duration::from_secs(1),
        )
        .ok();
        spawn_local(async move {
            let res = invoke::<_, Option<String>>("ai_soundtrack", &a).await;
            if let Some(t) = tick {
                t.clear();
            }
            match res {
                Ok(Some(p)) => {
                    set_version_audio(html, p);
                    music.set(MusicGen::Idle);
                }
                Ok(None) => music.set(MusicGen::Cancelled),
                Err(e) => music.set(MusicGen::Error(e)),
            }
        });
    };

    let plan_music = move |context: String, d: f64| {
        let (model, cost) = music_model(d);
        MusicPlan { model, cost, duration: d, prompt: soundtrack_prompt(&context, d, &music_style.get_untracked()) }
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
        let (base_url, api_key) = ai_auth();
        let a = A { req: Req { base_url, api_key, model: ai_model.get_untracked(), messages } };
        let (w, h, d) = (width.get_untracked(), height.get_untracked(), duration.get_untracked());
        gen.set(Gen::Running);
        progress.set(None);
        save_error.set(None);
        spawn_local(async move {
            match invoke::<_, Option<AiResult>>("ai_generate", &a).await {
                Ok(Some(r)) => {
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
        let model = ai_model.get_untracked().trim().to_string();
        let input_tokens = estimate_tokens(messages.iter().map(|m| m.content.chars().count()).sum());
        let price = models.with_untracked(|r| {
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
        ai_model.track();
        music_style.track();
        with_music.track();
        current.track();
        if prev.is_some() {
            pending.set(None);
        }
    });

    let generate = move || {
        let b = brief.get_untracked();
        let (w, h, d) = (width.get_untracked(), height.get_untracked(), duration.get_untracked());
        let msg = brief_message(&b);
        let music = with_music.get_untracked().then(|| plan_music(format!("Brief: {}", clip_text(&b, 1500)), d));
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
        pending.set(Some(Pending::Music { html_path: path, plan: plan_music(context, v.duration) }));
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
                if version_audio.get_untracked().as_deref() == Some(path.as_str()) {
                    audio_src.set(Some(url));
                    // Relance l'animation pour qu'image et son partent ensemble.
                    replay.update(|r| *r += 1);
                }
            }
        });
    });
    let stop_audio = move || {
        if let Some(el) = audio_ref.get_untracked() {
            let _ = el.pause();
        }
    };
    let play_audio = move || {
        let Some(el) = audio_ref.get_untracked() else { return };
        let _ = el.pause();
        el.set_current_time(0.0);
        if sound_on.get_untracked() && audio_src.with_untracked(Option::is_some) && active.get_untracked() {
            let _ = el.play();
        }
    };
    // Pas de son pendant une génération, ni quand l'onglet est masqué.
    Effect::new(move |_| {
        if running() || !active.get() {
            stop_audio();
        }
    });
    let toggle_sound = move || {
        let on = !sound_on.get_untracked();
        sound_on.set(on);
        store(KEY_SOUND, if on { "1" } else { "0" });
        if on {
            replay.update(|r| *r += 1);
        } else {
            stop_audio();
        }
    };

    let set_duration = move |s: String| {
        if let Ok(v) = s.trim().replace(',', ".").parse::<f64>() {
            if v > 0.0 {
                duration.set(v);
            }
        }
    };
    let can_call = move || in_tauri && !busy() && !ai_model.get().trim().is_empty();
    let can_generate = move || can_call() && !brief.get().trim().is_empty();
    let can_refine = move || can_call() && version().is_some() && !refine.get().trim().is_empty();
    // Bande son : il faut une animation enregistrée sur le disque.
    let has_file = move || version().is_some_and(|v| v.path.is_some());
    let can_music = move || in_tauri && !busy() && has_file();

    view! {
        <main class="grid motion" class:hidden=move || !active.get()>
            <section class="col settings">
                // ---------------- Brief
                <div class="card">
                    <div class="card-h"><span class="step">"1"</span><h2>{move || m().brief}</h2></div>
                    <textarea class="brief" rows="10" placeholder=move || m().brief_placeholder
                        prop:value=move || brief.get()
                        on:input=move |ev| { brief.set(event_target_value(&ev)); brief_file.set(None); } />
                    <div class="row">
                        <button class="btn" on:click=move |_| pick_brief() disabled=move || loading_file.get() || !in_tauri>
                            {move || m().btn_import}
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
                                None => view! { <span class="hint">{motion_texts(l).import_hint}</span> }.into_any(),
                            }
                        }}
                        <Show when=move || !brief.get().is_empty()>
                            <button class="link push" on:click=move |_| { brief.set(String::new()); brief_file.set(None); }>
                                {move || m().clear}
                            </button>
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
                        <div class="field">
                            <label class="lbl">{move || m().duration_s}</label>
                            <input type="number" min="1" max="600" step="0.5" prop:value=move || duration.get()
                                on:change=move |ev| set_duration(event_target_value(&ev)) />
                        </div>
                    </div>
                </div>

                // ---------------- Bande son
                <div class="card">
                    <div class="card-h">
                        <span class="step">"3"</span><h2>{move || m().music}</h2>
                        <span class="card-meta">{move || music_label(music_model(version().map(|v| v.duration).unwrap_or(duration.get())).0)}</span>
                    </div>
                    // État : bande son de l'animation affichée, composition en cours, erreur.
                    <div class="music-state">
                        {move || {
                            let l = lang.get();
                            let tx = motion_texts(l);
                            match music.get() {
                                MusicGen::Running(s) => view! {
                                    <span class="chip"><span class="spin"></span>{tx.music_running}<em>{fmt_secs(s as u64 * 1000)}</em></span>
                                    <button class="link push" on:click=move |_| cancel()>{tx.cancel}</button>
                                }.into_any(),
                                state => {
                                    let current_audio = version().and_then(|v| v.audio);
                                    view! {
                                        {match current_audio {
                                            Some(a) => view! { <span class="chip ok music-file" title=a.clone()>"♪ "{file_name(&a)}</span> }.into_any(),
                                            None if version().is_some() => view! { <span class="hint">{tx.music_none}</span> }.into_any(),
                                            None => view! { <span class="hint">{tx.music_need_anim}</span> }.into_any(),
                                        }}
                                        {match state {
                                            MusicGen::Error(e) => Some(view! { <div class="result err"><strong>{tx.music_failed}</strong><pre>{e}</pre></div> }.into_any()),
                                            MusicGen::Cancelled => Some(view! { <span class="hint">{tx.music_cancelled}</span> }.into_any()),
                                            _ => None,
                                        }}
                                    }.into_any()
                                }
                            }
                        }}
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
                                    <div><dt>{tx.confirm_model}</dt><dd>{music_label(plan.model)}</dd></div>
                                    <div><dt>{tx.duration_s}</dt><dd>{format!("{}", plan.duration)}</dd></div>
                                    <div class="cost span2"><dt>{tx.confirm_cost}</dt><dd>{fmt_usd(l, plan.cost)}</dd></div>
                                </dl>
                                <details class="prompt">
                                    <summary>{tx.confirm_prompt}</summary>
                                    <pre>{plan.prompt}</pre>
                                </details>
                                <p class="hint small">{tx.confirm_music_note}</p>
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
                            {move || m().music_import}
                        </button>
                        <button class="btn grow" on:click=move |_| compose_music()
                            disabled=move || !can_music() || matches!(pending.get(), Some(Pending::Music { .. }))
                            title=move || if has_file() { "" } else { m().music_need_anim }>
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
                        <span>{move || m().music_with}<small>{move || m().music_with_hint}</small></span>
                    </label>
                </div>

                // ---------------- Modèle IA
                <div class="card">
                    <div class="card-h">
                        <span class="step">"4"</span><h2>{move || m().ai}</h2>
                        <span class="card-meta">{move || ai_model.get()}</span>
                    </div>
                    <div class="field">
                        <label class="lbl" for="ai-model">{move || m().ai_model}</label>
                        <input id="ai-model" type="text" list="ai-models" spellcheck="false" prop:value=move || ai_model.get()
                            on:change=move |ev| {
                                let v = event_target_value(&ev).trim().to_string();
                                store(KEY_MODEL, &v);
                                ai_model.set(v);
                            } />
                        <datalist id="ai-models">
                            {move || models.get().and_then(|r| r.ok()).unwrap_or_default().into_iter()
                                .map(|i| view! { <option value=i.id></option> }).collect_view()}
                        </datalist>
                        <p class="hint small">{move || {
                            let l = lang.get();
                            match models.get() {
                                None if in_tauri => motion_texts(l).models_loading.to_string(),
                                None => motion_texts(l).outside_tauri.to_string(),
                                Some(Ok(list)) if !list.is_empty() => i18n::models_found(l, list.len()),
                                Some(_) => motion_texts(l).models_unavailable.to_string(),
                            }
                        }}</p>
                    </div>
                    <button class="link" on:click=move |_| show_ai.update(|v| *v = !*v)>
                        {move || if show_ai.get() { m().proxy_hide } else { m().proxy_show }}
                    </button>
                    <Show when=move || show_ai.get()>
                        <div class="advanced">
                            <div class="field">
                                <label class="lbl" for="ai-url">{move || m().ai_url}</label>
                                <input id="ai-url" type="text" spellcheck="false" prop:value=move || ai_url.get()
                                    on:change=move |ev| {
                                        let v = event_target_value(&ev).trim().to_string();
                                        let v = if v.is_empty() { DEFAULT_URL.to_string() } else { v };
                                        store(KEY_URL, &v);
                                        ai_url.set(v);
                                        refresh_models();
                                    } />
                            </div>
                            <div class="field">
                                <label class="lbl" for="ai-key">{move || m().ai_key}</label>
                                <input id="ai-key" type="password" autocomplete="off" placeholder=move || m().ai_key_placeholder
                                    prop:value=move || ai_key.get()
                                    on:change=move |ev| {
                                        let v = event_target_value(&ev).trim().to_string();
                                        store(KEY_KEY, &v);
                                        ai_key.set(v);
                                        refresh_models();
                                    } />
                            </div>
                            <p class="hint small">{move || m().proxy_hint}</p>
                        </div>
                    </Show>
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
                            // Leptos réutilise l'iframe et ne réécrit `srcdoc` que si la valeur change :
                            // le numéro de lecture, en commentaire final, force le rechargement (rejeu).
                            let run = replay.get();
                            match version() {
                                Some(v) => {
                                    let (w, h) = (v.width as f64, v.height as f64);
                                    let doc = format!("{}\n<!-- {run} -->", v.html);
                                    view! {
                                        <iframe class="mframe" sandbox="allow-scripts" srcdoc=doc
                                            title=motion_texts(l).preview on:load=move |_| play_audio()
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
                        <div class="row">
                            <button class="btn" on:click=move |_| replay.update(|r| *r += 1)>{move || m().replay}</button>
                            <Show when=move || audio_src.with(Option::is_some)>
                                <button class="btn ghost sound" class:off=move || !sound_on.get()
                                    title=move || if sound_on.get() { m().sound_mute } else { m().sound_unmute }
                                    aria-pressed=move || sound_on.get().to_string()
                                    on:click=move |_| toggle_sound()>
                                    "♪"
                                </button>
                            </Show>
                            {move || {
                                let n = versions.with(Vec::len);
                                (n > 1).then(|| view! {
                                    <div class="versions">
                                        <button class="btn ghost" disabled=move || current.get() == 0
                                            on:click=move |_| current.update(|c| *c = c.saturating_sub(1))>"‹"</button>
                                        <span>{move || i18n::version_of(lang.get(), current.get() + 1, versions.with(Vec::len))}</span>
                                        <button class="btn ghost" disabled=move || { current.get() + 1 >= versions.with(Vec::len) }
                                            on:click=move |_| current.update(|c| *c += 1)>"›"</button>
                                    </div>
                                })
                            }}
                            <span class="grow"></span>
                            {move || version().and_then(|v| v.model).map(|md| view! { <span class="hint small">{md}</span> })}
                        </div>
                    </Show>
                </div>

                <div class="card render-card">
                    {move || {
                        let l = lang.get();
                        let tx = motion_texts(l);
                        if running() {
                            return view! {
                                <button class="btn danger wide" on:click=move |_| cancel()>{tx.cancel}</button>
                            }.into_any();
                        }
                        if let Some(Pending::Animation { refine, model, input_tokens, output_tokens, cost, music, .. }) = pending.get() {
                            // Total : animation (estimée) + bande son (prix fixe).
                            let total = match (&cost, &music) {
                                (Some(c), Some(mp)) => fmt_usd(l, c + mp.cost),
                                (Some(c), None) => fmt_usd(l, *c),
                                (None, Some(mp)) => format!("{} + {}", tx.confirm_cost_unknown, fmt_usd(l, mp.cost)),
                                (None, None) => tx.confirm_cost_unknown.to_string(),
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
                                                <dd>{format!("{} · {}", music_label(mp.model), fmt_usd(l, mp.cost))}</dd></div>
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
                                <button class="btn primary big wide" on:click=move |_| generate() disabled=move || !can_generate()>
                                    {tx.generate}
                                </button>
                                <p class="hint center">{move || {
                                    let tx = motion_texts(lang.get());
                                    if !in_tauri { tx.outside_tauri }
                                    else if brief.get().trim().is_empty() { tx.need_brief }
                                    else if ai_model.get().trim().is_empty() { tx.need_model }
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
                                    <button class="btn primary big wide" disabled=!saved on:click=move |_| on_convert()>
                                        {tx.convert}
                                    </button>
                                    <div class="refine">
                                        <label class="lbl" for="refine">{tx.refine}</label>
                                        <textarea id="refine" rows="3" placeholder=tx.refine_placeholder
                                            prop:value=move || refine.get()
                                            on:input=move |ev| refine.set(event_target_value(&ev)) />
                                        <div class="row">
                                            <button class="btn grow" on:click=move |_| apply_refine() disabled=move || !can_refine()>
                                                {tx.refine_btn}
                                            </button>
                                            <button class="btn ghost" on:click=move |_| generate() disabled=move || !can_generate()
                                                title=tx.regenerate_title>
                                                {tx.regenerate}
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
