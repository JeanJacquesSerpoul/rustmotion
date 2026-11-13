//! Onglet « Motion design » : un brief (saisi, ou lu dans un PDF / .txt) est envoyé à l'IA
//! via le proxy OpenRouter ; l'animation HTML produite est prévisualisée, ajustable,
//! puis transmise à l'onglet de conversion vidéo.

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use rustmotion_types::motion::{brief_message, refine_message, system_prompt};
use rustmotion_types::Lang;
use serde::{Deserialize, Serialize};

use crate::app::PRESETS;
use crate::i18n::{self, motion_texts};
use crate::tauri::{self, invoke};

const KEY_URL: &str = "rustmotion.ai.url";
const KEY_MODEL: &str = "rustmotion.ai.model";
const KEY_KEY: &str = "rustmotion.ai.key";
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
    /// Message utilisateur qui a produit cette version (brief ou ajustement).
    request: String,
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

/// Construit l'onglet. `on_convert` reçoit le chemin du fichier HTML à convertir en vidéo.
pub fn motion_tab(
    lang: RwSignal<Lang>,
    active: Signal<bool>,
    dropped_doc: RwSignal<Option<String>>,
    on_convert: impl Fn(String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let in_tauri = tauri::available();
    let m = move || motion_texts(lang.get());

    // --- Réglages IA (mémorisés) ---------------------------------------------
    let ai_url = RwSignal::new(load(KEY_URL, DEFAULT_URL));
    let ai_model = RwSignal::new(load(KEY_MODEL, DEFAULT_MODEL));
    let ai_key = RwSignal::new(load(KEY_KEY, ""));
    let models = RwSignal::new(None::<Result<Vec<String>, String>>);
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

    let version = move || versions.with(|v| v.get(current.get()).cloned());
    let running = move || gen.get() == Gen::Running;

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
            models.set(Some(invoke::<_, Vec<String>>("ai_models", &a).await));
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

    // Envoie la conversation au proxy, enregistre le résultat et l'ajoute aux versions.
    let run = move |messages: Vec<ChatMessage>, request: String| {
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
        let key = ai_key.get_untracked();
        let a = A {
            req: Req {
                base_url: ai_url.get_untracked(),
                api_key: (!key.trim().is_empty()).then_some(key),
                model: ai_model.get_untracked(),
                messages,
            },
        };
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
                            path,
                            truncated: r.truncated,
                            model: r.model,
                            width: w,
                            height: h,
                            duration: d,
                            request,
                        })
                    });
                    current.set(versions.with_untracked(|v| v.len() - 1));
                    refine.set(String::new());
                    gen.set(Gen::Idle);
                }
                Ok(None) => gen.set(Gen::Cancelled),
                Err(e) => gen.set(Gen::Error(e)),
            }
        });
    };

    let generate = move || {
        let b = brief.get_untracked();
        let (w, h, d) = (width.get_untracked(), height.get_untracked(), duration.get_untracked());
        let msg = brief_message(&b);
        run(
            vec![
                ChatMessage { role: "system", content: system_prompt(w, h, d) },
                ChatMessage { role: "user", content: msg },
            ],
            b,
        );
    };

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
        run(
            vec![
                ChatMessage { role: "system", content: system_prompt(w, h, d) },
                ChatMessage { role: "user", content: brief_message(&first) },
                ChatMessage { role: "assistant", content: v.html },
                ChatMessage { role: "user", content: format!("{}{extra}", refine_message(&instr)) },
            ],
            instr,
        );
    };

    let cancel = move || {
        #[derive(Serialize)]
        struct A {}
        spawn_local(async move {
            let _ = invoke::<_, ()>("ai_cancel", &A {}).await;
        });
    };

    let open_path = move |cmd: &'static str, path: String| {
        #[derive(Serialize)]
        struct A {
            path: String,
        }
        spawn_local(async move {
            let _ = invoke::<_, ()>(cmd, &A { path }).await;
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

    let set_duration = move |s: String| {
        if let Ok(v) = s.trim().replace(',', ".").parse::<f64>() {
            if v > 0.0 {
                duration.set(v);
            }
        }
    };
    let can_call = move || in_tauri && !running() && !ai_model.get().trim().is_empty();
    let can_generate = move || can_call() && !brief.get().trim().is_empty();
    let can_refine = move || can_call() && version().is_some() && !refine.get().trim().is_empty();

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

                // ---------------- Modèle IA
                <div class="card">
                    <div class="card-h">
                        <span class="step">"3"</span><h2>{move || m().ai}</h2>
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
                                .map(|id| view! { <option value=id></option> }).collect_view()}
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
                            replay.track();
                            match version() {
                                Some(v) => {
                                    let (w, h) = (v.width as f64, v.height as f64);
                                    view! {
                                        <iframe class="mframe" sandbox="allow-scripts" srcdoc=v.html
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
                    <Show when=move || version().is_some() && !running()>
                        <div class="row">
                            <button class="btn" on:click=move |_| replay.update(|r| *r += 1)>{move || m().replay}</button>
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
                                let path = v.path.clone();
                                let (p1, p2, p3) = (path.clone(), path.clone(), path.clone());
                                view! {
                                    {status}
                                    {v.truncated.then(|| view! { <div class="result warn"><strong>{tx.truncated}</strong></div> })}
                                    {match &path {
                                        Some(p) => view! { <div class="result ok"><strong>{tx.saved}</strong><span>{p.clone()}</span></div> }.into_any(),
                                        None => view! { <div class="result err"><strong>{tx.save_failed}</strong>
                                            <pre>{save_error.get().unwrap_or_default()}</pre></div> }.into_any(),
                                    }}
                                    <button class="btn primary big wide" disabled=path.is_none()
                                        on:click=move |_| if let Some(p) = p1.clone() { on_convert(p) }>
                                        {tx.convert}
                                    </button>
                                    <div class="row">
                                        <button class="btn grow" disabled=path.is_none()
                                            on:click=move |_| if let Some(p) = p2.clone() { open_path("open_file", p) }>{tx.open_browser}</button>
                                        <button class="btn grow" disabled=path.is_none()
                                            on:click=move |_| if let Some(p) = p3.clone() { open_path("reveal", p) }>{tx.reveal}</button>
                                    </div>
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
