use leptos::prelude::*;
use leptos::task::spawn_local;
use rustmotion_types::{Codec, Framing, Lang, ProbeInfo, Progress, RenderSettings, SourceFiles, Stage, ToolStatus};
use serde::{Deserialize, Serialize};

use crate::i18n::{self, texts};
use crate::motion::SourceFormat;
use crate::tauri::{self, invoke};

#[derive(Clone, PartialEq)]
enum Phase {
    Idle,
    Rendering,
    Done(String),
    Cancelled,
    Error(String),
}

#[derive(Serialize)]
struct NoArgs {}

#[derive(Deserialize)]
struct DragDrop {
    paths: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Motion,
    Convert,
}

pub(crate) const PRESETS: [(&str, u32, u32); 6] = [
    ("1080p", 1920, 1080),
    ("720p", 1280, 720),
    ("4K", 3840, 2160),
    ("9:16", 1080, 1920),
    ("1:1", 1080, 1080),
    ("4:5", 1080, 1350),
];
const FPS: [u32; 5] = [24, 25, 30, 50, 60];
const SCALES: [f64; 3] = [1.0, 1.5, 2.0];

fn fmt_ms(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 60 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

fn file_name(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_string()
}

fn swap_ext(p: &str, ext: &str) -> String {
    let name_start = p.rfind(['/', '\\']).map(|i| i + 1).unwrap_or(0);
    match p[name_start..].rfind('.') {
        Some(dot) => format!("{}.{ext}", &p[..name_start + dot]),
        None => format!("{p}.{ext}"),
    }
}

fn opt(s: String) -> Option<String> {
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Ajoute des paramètres d'URL à la source (`a=1&b=2`, ou `#ancre`).
fn with_query(input: &str, query: &str) -> String {
    let (input, query) = (input.trim(), query.trim().trim_start_matches('?'));
    if input.is_empty() || query.is_empty() {
        return input.to_string();
    }
    if query.starts_with('#') {
        format!("{input}{query}")
    } else if input.contains('?') && (input.starts_with("http://") || input.starts_with("https://")) {
        format!("{input}&{query}")
    } else {
        format!("{input}?{query}")
    }
}

fn parse_f64(s: String) -> Result<f64, std::num::ParseFloatError> {
    s.trim().replace(',', ".").parse()
}

fn browser_name(path: Option<&str>, lang: Lang) -> &'static str {
    let Some(p) = path else { return texts(lang).browser_missing };
    let p = p.to_ascii_lowercase();
    if p.contains("msedge") || p.contains("edge") {
        "Edge"
    } else if p.contains("brave") {
        "Brave"
    } else if p.contains("chromium") {
        "Chromium"
    } else {
        "Chrome"
    }
}

#[component]
pub fn App() -> impl IntoView {
    let in_tauri = tauri::available();

    // --- Langue -------------------------------------------------------------
    let lang = RwSignal::new(i18n::initial_lang());
    let t = move || texts(lang.get());
    Effect::new(move |_| {
        let l = lang.get();
        i18n::remember_lang(l);
        if in_tauri {
            #[derive(Serialize)]
            struct A {
                lang: Lang,
            }
            spawn_local(async move {
                let _ = invoke::<_, ()>("set_lang", &A { lang: l }).await;
            });
        }
    });

    // --- État ---------------------------------------------------------------
    let tools = RwSignal::new(None::<ToolStatus>);
    let input = RwSignal::new(String::new());
    let files = RwSignal::new(None::<SourceFiles>);
    let output = RwSignal::new(String::new());
    let output_touched = RwSignal::new(false);
    let width = RwSignal::new(1920u32);
    let height = RwSignal::new(1080u32);
    let scale = RwSignal::new(1.0f64);
    let fps = RwSignal::new(30u32);
    let duration = RwSignal::new(5.0f64);
    let start = RwSignal::new(0.0f64);
    let codec = RwSignal::new(Codec::H264);
    let crf = RwSignal::new(18u8);
    let transparent = RwSignal::new(false);
    let audio = RwSignal::new(String::new());
    let wait_ms = RwSignal::new(250u32);
    let browser_path = RwSignal::new(String::new());
    let ffmpeg_path = RwSignal::new(String::new());
    let show_advanced = RwSignal::new(false);
    let framing = RwSignal::new(Framing::Auto);
    // Paramètres d'URL transmis à la page (ex. « render=1 » pour un mode de rendu sans commandes).
    let query = RwSignal::new(String::new());
    let full_input = move || with_query(&input.get_untracked(), &query.get_untracked());

    let probe_info = RwSignal::new(None::<ProbeInfo>);
    let probing = RwSignal::new(false);
    let probe_again = RwSignal::new(false);
    let probe_error = RwSignal::new(None::<String>);
    let phase = RwSignal::new(Phase::Idle);
    let progress = RwSignal::new(None::<Progress>);
    let preview = RwSignal::new(None::<String>);
    let dragging = RwSignal::new(false);
    let tab = RwSignal::new(Tab::Motion);
    // Brief (PDF / texte) déposé sur la fenêtre, transmis à l'onglet Motion design.
    let dropped_doc = RwSignal::new(None::<String>);
    // Fichier audio déposé sur l'onglet Motion design : il devient la bande son de l'animation.
    let dropped_audio = RwSignal::new(None::<String>);
    // La piste audio de l'export vient de la bande son compagnon (et suit donc l'animation).
    let audio_auto = RwSignal::new(true);
    // Résultat de « Sauvegarder » dans la barre commune, rattaché au fichier copié.
    let copy_status = RwSignal::new(None::<(String, Result<String, String>)>);

    // --- Actions ------------------------------------------------------------
    let refresh_tools = move || {
        #[derive(Serialize)]
        struct A {
            browser: Option<String>,
            ffmpeg: Option<String>,
        }
        let a = A { browser: opt(browser_path.get_untracked()), ffmpeg: opt(ffmpeg_path.get_untracked()) };
        spawn_local(async move {
            if let Ok(t) = invoke::<_, ToolStatus>("tool_status", &a).await {
                tools.set(Some(t));
            }
        });
    };

    let suggest = move || {
        if output_touched.get_untracked() {
            return;
        }
        #[derive(Serialize)]
        struct A {
            input: String,
            codec: Codec,
        }
        let a = A { input: input.get_untracked(), codec: codec.get_untracked() };
        spawn_local(async move {
            if let Ok(p) = invoke::<_, String>("suggest_output", &a).await {
                output.set(p);
            }
        });
    };

    let run_probe = move || {
        if input.get_untracked().trim().is_empty() {
            return;
        }
        // Une analyse est déjà en cours : on la refera avec les réglages à jour dès qu'elle finit.
        if probing.get_untracked() {
            probe_again.set(true);
            return;
        }
        #[derive(Serialize)]
        struct A {
            input: String,
            width: u32,
            height: u32,
            browser: Option<String>,
            framing: Framing,
        }
        let a = A {
            input: full_input(),
            width: width.get_untracked(),
            height: height.get_untracked(),
            browser: opt(browser_path.get_untracked()),
            framing: framing.get_untracked(),
        };
        probing.set(true);
        probe_error.set(None);
        spawn_local(async move {
            match invoke::<_, ProbeInfo>("probe", &a).await {
                Ok(info) => {
                    if let Some(d) = info.duration {
                        duration.set(d);
                    }
                    if let (Some(w), Some(h)) = (info.width, info.height) {
                        width.set(w);
                        height.set(h);
                    }
                    if let Some(f) = info.fps {
                        fps.set(f);
                    }
                    if info.thumbnail.is_some() && phase.get_untracked() != Phase::Rendering {
                        preview.set(info.thumbnail.clone());
                    }
                    probe_info.set(Some(info));
                }
                Err(e) => {
                    probe_info.set(None);
                    probe_error.set(Some(e));
                }
            }
            probing.set(false);
        });
    };

    Effect::new(move |_| {
        if !probing.get() && probe_again.get_untracked() {
            probe_again.set(false);
            run_probe();
        }
    });

    // Changer de format refait l'analyse (après une courte pause, le temps de taper les dimensions) :
    // la miniature montre alors le cadrage réel de la vidéo.
    let dims_gen = StoredValue::new(0u32);
    Effect::new(move |prev: Option<(u32, u32)>| {
        let dims = (width.get(), height.get());
        if prev.is_some_and(|p| p != dims) && probe_info.get_untracked().is_some() {
            dims_gen.update_value(|g| *g += 1);
            let gen = dims_gen.get_value();
            set_timeout(
                move || {
                    if dims_gen.get_value() == gen && phase.get_untracked() != Phase::Rendering {
                        run_probe();
                    }
                },
                std::time::Duration::from_millis(600),
            );
        }
        dims
    });

    // Fichiers de la source (script et bande son compagnon), relus sans navigateur.
    let refresh_files = move || {
        let path = input.get_untracked();
        if path.trim().is_empty() {
            return;
        }
        #[derive(Serialize)]
        struct A {
            input: String,
        }
        spawn_local(async move {
            if let Ok(f) = invoke::<_, SourceFiles>("inspect_source", &A { input: path.clone() }).await {
                // Ignore une réponse arrivée après un changement de source.
                if input.get_untracked() == path {
                    files.set(Some(f));
                }
            }
        });
    };

    // Bande son compagnon : elle devient la piste audio de l'export, sauf choix manuel.
    Effect::new(move |_| {
        let companion = files.with(|f| f.as_ref().and_then(|f| f.audio.clone()));
        if audio_auto.get_untracked() {
            audio.set(companion.unwrap_or_default());
        }
    });

    let set_input = move |path: String| {
        input.set(path.clone());
        files.set(None);
        // Une piste choisie à la main reste ; sinon elle suivra la nouvelle animation.
        if audio_auto.get_untracked() || audio.get_untracked().trim().is_empty() {
            audio_auto.set(true);
            audio.set(String::new());
        }
        refresh_files();
        probe_info.set(None);
        preview.set(None);
        if matches!(phase.get_untracked(), Phase::Done(_) | Phase::Error(_) | Phase::Cancelled) {
            phase.set(Phase::Idle);
        }
        suggest();
        run_probe();
    };

    let pick = move |kind: &'static str, target: RwSignal<String>, then_probe: bool| {
        #[derive(Serialize)]
        struct A {
            kind: &'static str,
        }
        spawn_local(async move {
            if let Ok(Some(p)) = invoke::<_, Option<String>>("pick_file", &A { kind }).await {
                if then_probe {
                    set_input(p);
                } else {
                    target.set(p);
                    if kind == "exe" {
                        refresh_tools();
                    }
                    if kind == "audio" {
                        audio_auto.set(false);
                    }
                }
            }
        });
    };

    let pick_folder = move || {
        spawn_local(async move {
            if let Ok(Some(p)) = invoke::<_, Option<String>>("pick_folder", &NoArgs {}).await {
                set_input(p);
            }
        });
    };

    let pick_output = move || {
        #[derive(Serialize)]
        struct A {
            codec: Codec,
            current: Option<String>,
        }
        let a = A { codec: codec.get_untracked(), current: opt(output.get_untracked()) };
        spawn_local(async move {
            if let Ok(Some(p)) = invoke::<_, Option<String>>("pick_output", &a).await {
                output.set(p);
                output_touched.set(true);
            }
        });
    };

    let set_codec = move |c: Codec| {
        codec.set(c);
        if !c.supports_alpha() {
            transparent.set(false);
        }
        let out = output.get_untracked();
        if !out.is_empty() {
            output.set(swap_ext(&out, c.extension()));
        }
    };

    let start_render = move || {
        let settings = RenderSettings {
            input: full_input(),
            output: output.get_untracked(),
            width: width.get_untracked(),
            height: height.get_untracked(),
            scale: scale.get_untracked(),
            fps: fps.get_untracked(),
            duration: duration.get_untracked(),
            start: start.get_untracked(),
            codec: codec.get_untracked(),
            crf: crf.get_untracked(),
            transparent: transparent.get_untracked() && codec.get_untracked().supports_alpha(),
            audio: opt(audio.get_untracked()),
            wait_ms: wait_ms.get_untracked(),
            browser_path: opt(browser_path.get_untracked()),
            ffmpeg_path: opt(ffmpeg_path.get_untracked()),
            framing: framing.get_untracked(),
        };
        #[derive(Serialize)]
        struct A {
            settings: RenderSettings,
        }
        phase.set(Phase::Rendering);
        progress.set(None);
        spawn_local(async move {
            let res = invoke::<_, Option<String>>("render", &A { settings }).await;
            phase.set(match res {
                Ok(Some(p)) => Phase::Done(p),
                Ok(None) => Phase::Cancelled,
                Err(e) => Phase::Error(e),
            });
            // Le fichier existe désormais : on évitera de l'écraser au prochain rendu.
            output_touched.set(false);
        });
    };

    let cancel = move || {
        spawn_local(async move {
            let _ = invoke::<_, ()>("cancel_render", &NoArgs {}).await;
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

    let save_copy = move || {
        #[derive(Serialize)]
        struct A {
            path: String,
        }
        let path = input.get_untracked();
        spawn_local(async move {
            match invoke::<_, Option<String>>("save_copy", &A { path: path.clone() }).await {
                Ok(Some(p)) => copy_status.set(Some((path, Ok(p)))),
                Ok(None) => {}
                Err(e) => copy_status.set(Some((path, Err(e)))),
            }
        });
    };

    // --- Événements Tauri ---------------------------------------------------
    if in_tauri {
        refresh_tools();
        tauri::listen::<Progress>("render-progress", move |p| {
            if let Some(img) = p.preview.clone() {
                preview.set(Some(img));
            }
            progress.set(Some(p));
        });
        tauri::listen::<DragDrop>("tauri://drag-drop", move |d| {
            dragging.set(false);
            if let Some(p) = d.paths.into_iter().next() {
                let lower = p.to_ascii_lowercase();
                let is_doc = [".pdf", ".txt", ".md", ".markdown"].iter().any(|e| lower.ends_with(e));
                if is_doc {
                    tab.set(Tab::Motion);
                    dropped_doc.set(Some(p));
                    return;
                }
                if phase.get_untracked() == Phase::Rendering {
                    return;
                }
                let is_audio = [".mp3", ".wav", ".aac", ".m4a", ".ogg", ".flac", ".opus"]
                    .iter()
                    .any(|e| lower.ends_with(e));
                if is_audio {
                    // Sur l'onglet Motion design, avec une animation : c'est sa bande son.
                    if tab.get_untracked() == Tab::Motion && !input.get_untracked().trim().is_empty() {
                        dropped_audio.set(Some(p));
                    } else {
                        tab.set(Tab::Convert);
                        audio_auto.set(false);
                        audio.set(p);
                    }
                } else {
                    // L'animation va dans la barre commune : l'onglet affiché ne change pas.
                    set_input(p);
                }
            }
        });
        tauri::listen::<serde_json::Value>("tauri://drag-enter", move |_| dragging.set(true));
        tauri::listen::<serde_json::Value>("tauri://drag-leave", move |_| dragging.set(false));
    }

    // --- Dérivés ------------------------------------------------------------
    let rendering = move || phase.get() == Phase::Rendering;
    let total_frames = move || ((duration.get() * fps.get() as f64).round() as u32).max(1);
    // Vidéo cadrée sur un élément : sa hauteur découle des proportions de l'élément.
    let framed = move || probe_info.get().and_then(|i| i.frame).filter(|_| framing.get() != Framing::Page);
    let eff_height = move || framed().map(|f| f.height_for(width.get())).unwrap_or_else(|| height.get());
    let out_size = move || {
        let s = scale.get();
        ((width.get() as f64 * s).round() as u32, (eff_height() as f64 * s).round() as u32)
    };
    let tools_ok = move || tools.get().map(|t| t.browser.is_some() && t.ffmpeg.is_some()).unwrap_or(false);
    let can_render = move || {
        // L'export exige une analyse réussie et sans motif de rejet.
        let accepted = probe_info.get().is_some_and(|i| i.rejection().is_none()) && !probing.get();
        !rendering() && tools_ok() && accepted && !input.get().trim().is_empty() && !output.get().trim().is_empty() && duration.get() > 0.0
    };

    // --- Barre commune : l'animation partagée par les deux onglets ---------------
    let source_path = move || input.get().trim().trim_matches('"').to_string();
    let is_local = move || {
        let p = source_path().to_ascii_lowercase();
        !p.is_empty() && !["http://", "https://", "file:", "data:"].iter().any(|s| p.starts_with(s))
    };
    let is_html_file = move || {
        let p = source_path().to_ascii_lowercase();
        is_local() && (p.ends_with(".html") || p.ends_with(".htm"))
    };
    let source_format = Signal::derive(move || {
        probe_info.get().map(|i| SourceFormat { path: source_path(), width: i.width, height: i.height, duration: i.duration })
    });

    // --- Vue ----------------------------------------------------------------
    view! {
        <div class="app" class:dragging=move || dragging.get()>
            <header class="top">
                <div class="brand">
                    <div class="logo"><span></span></div>
                    <div>
                        <h1>"RustMotion"</h1>
                        <p>{move || t().tagline}</p>
                    </div>
                </div>
                <nav class="tabs" role="tablist">
                    {[(Tab::Motion, "1"), (Tab::Convert, "2")].into_iter().map(|(tb, n)| view! {
                        <button role="tab" class:active=move || tab.get() == tb aria-selected=move || (tab.get() == tb).to_string()
                            on:click=move |_| tab.set(tb)>
                            <span class="tab-n">{n}</span>
                            {move || {
                                let m = i18n::motion_texts(lang.get());
                                if tb == Tab::Motion { m.tab_motion } else { m.tab_convert }
                            }}
                        </button>
                    }).collect_view()}
                </nav>
                <div class="tools">
                    <div class="lang" role="group" title=move || t().lang_title>
                        {Lang::ALL.into_iter().map(|l| view! {
                            <button class:active=move || lang.get() == l on:click=move |_| lang.set(l)>
                                {l.code().to_uppercase()}
                            </button>
                        }).collect_view()}
                    </div>
                    {move || {
                        let tx = t();
                        match tools.get() {
                            None => view! { <span class="badge muted">{if in_tauri { tx.detecting_tools } else { tx.outside_tauri }}</span> }.into_any(),
                            Some(tl) => view! {
                                <span class="badge" class:ok=tl.browser.is_some() title=tl.browser.clone().unwrap_or_default()>
                                    <i></i>{browser_name(tl.browser.as_deref(), lang.get())}
                                </span>
                                <span class="badge" class:ok=tl.ffmpeg.is_some()
                                    title=format!("{}\n{}", tl.ffmpeg.clone().unwrap_or_default(), tl.ffmpeg_version.clone().unwrap_or_default())>
                                    <i></i>{if tl.ffmpeg.is_some() { "ffmpeg" } else { tx.ffmpeg_missing }}
                                </span>
                            }.into_any(),
                        }
                    }}
                </div>
            </header>

            // ---------------- Animation commune aux deux onglets
            <div class="filebar-wrap">
                <section class="filebar" class:empty=move || source_path().is_empty() aria-label=move || t().bar_label>
                    <div class="fb-icon" aria-hidden="true"><span></span></div>
                    <div class="fb-main">
                        <div class="fb-head">
                            <strong class="fb-name">{move || {
                                if source_path().is_empty() {
                                    t().bar_empty.to_string()
                                } else {
                                    files.get().and_then(|f| f.html).unwrap_or_else(|| file_name(&source_path()))
                                }
                            }}</strong>
                            {move || {
                                let l = lang.get();
                                let tx = texts(l);
                                if probing.get() {
                                    return Some(view! { <span class="chip"><span class="spin"></span>{tx.analysing}</span> }.into_any());
                                }
                                probe_info.get().map(|i| {
                                    let size = i.width.zip(i.height).map(|(w, h)| format!("{w}×{h}"));
                                    let secs = i.duration.map(|d| format!("{} s", (d * 100.0).round() / 100.0));
                                    let label = [size, secs].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                                    (!label.is_empty()).then(|| view! { <span class="chip">{label}</span> })
                                }).map(|v| v.into_any())
                            }}
                            {move || files.get().and_then(|f| f.audio).map(|a| view! {
                                <span class="chip ok" title=a.clone()>"♪ "{file_name(&a)}</span>
                            })}
                            {move || {
                                let m = i18n::motion_texts(lang.get());
                                copy_status.get().filter(|(src, _)| *src == input.get()).map(|(_, r)| match r {
                                    Ok(p) => view! { <span class="chip ok" title=p>{m.copy_saved.trim_end_matches([' ', ':'])}" ✓"</span> }.into_any(),
                                    Err(e) => view! { <span class="chip err" title=e>{m.copy_failed}</span> }.into_any(),
                                })
                            }}
                        </div>
                        <input class="fb-path" type="text" spellcheck="false" placeholder=move || t().path_placeholder
                            title=move || input.get() prop:value=move || input.get() disabled=rendering
                            on:change=move |ev| set_input(event_target_value(&ev)) />
                        <Show when=move || source_path().is_empty()>
                            <p class="fb-hint">{move || t().bar_hint}</p>
                        </Show>
                    </div>
                    <div class="fb-actions">
                        <div class="fb-group">
                            <button class="btn" title=move || t().btn_file_title on:click=move |_| pick("html", input, true) disabled=rendering>
                                {move || t().btn_file}
                            </button>
                            <button class="btn" title=move || t().btn_folder_title on:click=move |_| pick_folder() disabled=rendering>
                                {move || t().btn_folder}
                            </button>
                        </div>
                        <Show when=move || !source_path().is_empty()>
                            <div class="fb-group">
                                {move || {
                                    let m = i18n::motion_texts(lang.get());
                                    view! {
                                        <button class="btn ghost" title=m.open_title disabled=move || !is_local()
                                            on:click=move |_| open_path("open_file", source_path())>{m.open_browser}</button>
                                        <button class="btn ghost" title=m.reveal_title disabled=move || !is_local()
                                            on:click=move |_| open_path("reveal", source_path())>{m.reveal}</button>
                                        <button class="btn ghost" title=m.save_title disabled=move || !is_html_file()
                                            on:click=move |_| save_copy()>{m.save}</button>
                                    }
                                }}
                            </div>
                        </Show>
                    </div>
                </section>
            </div>

            {crate::motion::motion_tab(
                lang,
                Signal::derive(move || tab.get() == Tab::Motion),
                dropped_doc,
                dropped_audio,
                Signal::derive(move || input.get()),
                source_format,
                move |path: String| {
                    // Pendant un rendu, la source ne change pas.
                    if phase.get_untracked() != Phase::Rendering && input.get_untracked().trim().trim_matches('"') != path {
                        set_input(path);
                    }
                },
                move |html: String, track: String| {
                    // Nouvelle bande son de l'animation courante : l'export la reprend.
                    if source_path() == html && phase.get_untracked() != Phase::Rendering {
                        audio_auto.set(true);
                        audio.set(track);
                        refresh_files();
                    }
                },
                move || tab.set(Tab::Convert),
            )}

            <main class="grid" class:hidden=move || tab.get() != Tab::Convert>
                <section class="col settings">
                    // ---------------- Source
                    <div class="card">
                        <div class="card-h"><span class="step">"1"</span><h2>{move || t().source}</h2></div>
                        // Script compagnon de même nom que la page (le fichier lui-même est dans la barre du haut).
                        {move || {
                            let tx = t();
                            files.get().and_then(|f| {
                                let name = f.expected_script.clone()?;
                                Some(if f.script_found {
                                    let note = if f.script_linked { tx.script_linked } else { tx.script_injected };
                                    view! { <ul class="files"><li class="ok"><b>{name}</b><span>{note}</span></li></ul> }.into_any()
                                } else {
                                    view! { <ul class="files"><li class="warn"><b>{name}</b><span>{tx.script_missing}</span></li></ul> }.into_any()
                                })
                            })
                        }}
                        <div class="query">
                            <label class="lbl" for="query">{move || t().query_label}</label>
                            <div class="query-field">
                                <span>"?"</span>
                                <input id="query" type="text" placeholder="render=1" prop:value=move || query.get()
                                    on:change=move |ev| {
                                        query.set(event_target_value(&ev).trim().trim_start_matches('?').to_string());
                                        run_probe();
                                    } />
                            </div>
                        </div>
                        <div class="probe">
                            {move || {
                                let l = lang.get();
                                let tx = texts(l);
                                if probing.get() {
                                    return view! { <span class="chip"><span class="spin"></span>{tx.analysing}</span> }.into_any();
                                }
                                if let Some(e) = probe_error.get() {
                                    return view! { <span class="chip err">{e}</span> }.into_any();
                                }
                                match probe_info.get() {
                                    None if input.get().trim().is_empty() => view! { <span class="hint">{tx.need_input}</span> }.into_any(),
                                    None => view! { <span class="hint">{tx.probe_hint}</span> }.into_any(),
                                    Some(i) => view! {
                                        {(!i.title.is_empty()).then(|| view! { <span class="chip strong">{i.title.clone()}</span> })}
                                        <span class="chip">{i18n::animations(l, i.animations)}</span>
                                        {i.gsap.then(|| view! { <span class="chip">"GSAP"</span> })}
                                        {i.seek_api.then(|| view! { <span class="chip">{tx.seek_chip}</span> })}
                                        {i.frame.as_ref().map(|f| view! { <span class="chip ok">{i18n::framed_on(l, &f.label)}</span> })}
                                        {match (i.duration, i.duration_source) {
                                            (Some(d), Some(src)) => view! { <span class="chip ok">{format!("{d:.2} s ({})", src.label(l))}</span> }.into_any(),
                                            _ => view! { <span class="chip warn">{tx.duration_unknown}</span> }.into_any(),
                                        }}
                                        {i.infinite.then(|| view! { <span class="chip warn">{tx.infinite_hint}</span> })}
                                        {
                                            let blocking = i.blocking_missing();
                                            i.missing_files.iter().filter(|f| !blocking.contains(f)).map(|f| view! {
                                                <span class="chip warn">{i18n::file_not_found(l, f)}</span>
                                            }).collect_view()
                                        }
                                        {i.rejection().map(|r| view! { <span class="chip err reject">{r.message(l)}</span> })}
                                    }.into_any(),
                                }
                            }}
                            <button class="link" on:click=move |_| run_probe() disabled=move || input.get().is_empty() || probing.get() || rendering()>{move || t().reanalyse}</button>
                        </div>
                    </div>

                    // ---------------- Format
                    <div class="card">
                        <div class="card-h"><span class="step">"2"</span><h2>{move || t().format}</h2></div>
                        <label class="lbl">{move || t().presets}</label>
                        <div class="seg">
                            {PRESETS.into_iter().map(|(name, w, h)| view! {
                                <button class="pill" class:active=move || width.get() == w && height.get() == h
                                    on:click=move |_| { width.set(w); height.set(h); }>
                                    <b>{name}</b><small>{format!("{w}×{h}")}</small>
                                </button>
                            }).collect_view()}
                        </div>
                        <div class="fields">
                            <div class="field">
                                <label class="lbl">{move || t().width}</label>
                                <input type="number" min="16" max="8192" prop:value=move || width.get()
                                    on:change=move |ev| if let Ok(v) = event_target_value(&ev).parse() { width.set(v) } />
                            </div>
                            <div class="field">
                                <label class="lbl">{move || t().height}</label>
                                <input type="number" min="16" max="8192" prop:value=eff_height
                                    disabled=move || framed().is_some()
                                    title=move || if framed().is_some() { t().height_from_frame } else { "" }
                                    on:change=move |ev| if let Ok(v) = event_target_value(&ev).parse() { height.set(v) } />
                            </div>
                            <div class="field">
                                <label class="lbl">{move || t().duration_s}</label>
                                <input type="number" min="0.1" step="0.1" prop:value=move || duration.get()
                                    on:change=move |ev| if let Ok(v) = parse_f64(event_target_value(&ev)) { if v > 0.0 { duration.set(v) } } />
                            </div>
                            <div class="field">
                                <label class="lbl">{move || t().start_s}</label>
                                <input type="number" min="0" step="0.1" prop:value=move || start.get()
                                    on:change=move |ev| if let Ok(v) = parse_f64(event_target_value(&ev)) { start.set(v.max(0.0)) } />
                            </div>
                        </div>
                        <div class="field">
                            <label class="lbl">{move || t().framing_label}</label>
                            <div class="seg tight framing">
                                {[Framing::Auto, Framing::Page].into_iter().map(|f| {
                                    let f2 = f.clone();
                                    let f3 = f.clone();
                                    view! {
                                        <button class="pill" class:active=move || framing.get() == f2
                                            on:click=move |_| { framing.set(f3.clone()); run_probe(); }>
                                            {move || if f == Framing::Auto { t().framing_auto } else { t().framing_page }}
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                        </div>
                        <div class="fields two">
                            <div class="field">
                                <label class="lbl">{move || t().fps_label}</label>
                                <div class="seg tight">
                                    {FPS.into_iter().map(|f| view! {
                                        <button class="pill" class:active=move || fps.get() == f on:click=move |_| fps.set(f)>{f}</button>
                                    }).collect_view()}
                                </div>
                            </div>
                            <div class="field">
                                <label class="lbl">{move || t().sharpness}</label>
                                <div class="seg tight">
                                    {SCALES.into_iter().map(|s| view! {
                                        <button class="pill" class:active=move || (scale.get() - s).abs() < 1e-6 on:click=move |_| scale.set(s)>{format!("{s}×")}</button>
                                    }).collect_view()}
                                </div>
                            </div>
                        </div>
                    </div>

                    // ---------------- Export
                    <div class="card">
                        <div class="card-h"><span class="step">"3"</span><h2>{move || t().export}</h2></div>
                        <div class="codecs">
                            {Codec::ALL.into_iter().map(|c| view! {
                                <button class="codec" class:active=move || codec.get() == c on:click=move |_| set_codec(c)>
                                    <b>{c.label()}</b>
                                    <small>{move || c.description(lang.get())}</small>
                                </button>
                            }).collect_view()}
                        </div>
                        <Show when=move || codec.get().uses_crf()>
                            <div class="field">
                                <label class="lbl">{move || t().quality}" "<em>{move || format!("CRF {} ({})", crf.get(), i18n::crf_label(lang.get(), crf.get()))}</em></label>
                                <input type="range" min="0" max="40" prop:value=move || crf.get()
                                    on:input=move |ev| if let Ok(v) = event_target_value(&ev).parse() { crf.set(v) } />
                            </div>
                        </Show>
                        <label class="toggle" class:disabled=move || !codec.get().supports_alpha()>
                            <input type="checkbox" prop:checked=move || transparent.get()
                                disabled=move || !codec.get().supports_alpha()
                                on:change=move |ev| transparent.set(event_target_checked(&ev)) />
                            <span class="sw"></span>
                            <span>{move || t().transparent}<small>{move || t().transparent_hint}</small></span>
                        </label>
                        <div class="field">
                            <label class="lbl">
                                {move || t().audio_label}
                                <Show when=move || audio_auto.get() && !audio.get().is_empty()>
                                    <span class="lbl-tag">{move || t().audio_companion}</span>
                                </Show>
                            </label>
                            <div class="row">
                                <input class="grow" type="text" placeholder=move || t().audio_none prop:value=move || audio.get()
                                    on:change=move |ev| { audio_auto.set(false); audio.set(event_target_value(&ev)); } />
                                <button class="btn" on:click=move |_| pick("audio", audio, false)>{move || t().btn_choose}</button>
                                <Show when=move || !audio.get().is_empty()>
                                    <button class="btn ghost" on:click=move |_| { audio_auto.set(false); audio.set(String::new()); }>"✕"</button>
                                </Show>
                            </div>
                        </div>
                        <div class="field">
                            <label class="lbl">{move || t().output_label}</label>
                            <div class="row">
                                <input class="grow" type="text" prop:value=move || output.get()
                                    on:change=move |ev| { output.set(event_target_value(&ev)); output_touched.set(true); } />
                                <button class="btn" on:click=move |_| pick_output()>{move || t().btn_save_as}</button>
                            </div>
                        </div>
                        <button class="link" on:click=move |_| show_advanced.update(|v| *v = !*v)>
                            {move || if show_advanced.get() { t().advanced_hide } else { t().advanced_show }}
                        </button>
                        <Show when=move || show_advanced.get()>
                            <div class="advanced">
                                <div class="field">
                                    <label class="lbl">{move || t().wait_label}</label>
                                    <input type="number" min="0" step="50" prop:value=move || wait_ms.get()
                                        on:change=move |ev| if let Ok(v) = event_target_value(&ev).parse() { wait_ms.set(v) } />
                                </div>
                                <div class="field">
                                    <label class="lbl">{move || t().browser_label}</label>
                                    <div class="row">
                                        <input class="grow" type="text" prop:value=move || browser_path.get()
                                            on:change=move |ev| { browser_path.set(event_target_value(&ev)); refresh_tools(); } />
                                        <button class="btn" on:click=move |_| pick("exe", browser_path, false)>"…"</button>
                                    </div>
                                </div>
                                <div class="field">
                                    <label class="lbl">{move || t().ffmpeg_label}</label>
                                    <div class="row">
                                        <input class="grow" type="text" prop:value=move || ffmpeg_path.get()
                                            on:change=move |ev| { ffmpeg_path.set(event_target_value(&ev)); refresh_tools(); } />
                                        <button class="btn" on:click=move |_| pick("exe", ffmpeg_path, false)>"…"</button>
                                    </div>
                                </div>
                            </div>
                        </Show>
                    </div>
                </section>

                // ---------------- Aperçu + rendu
                <section class="col side">
                    <div class="card preview-card">
                        <div class="viewport" style=move || format!("aspect-ratio: {} / {}", width.get().max(1), height.get().max(1))>
                            {move || match preview.get() {
                                Some(src) => view! { <img src=src alt=t().preview_alt /> }.into_any(),
                                None => view! { <div class="vp-empty">{t().preview}</div> }.into_any(),
                            }}
                            <span class="vp-tag">{move || { let (w, h) = out_size(); format!("{w}×{h}") }}</span>
                        </div>
                        <dl class="summary">
                            <div><dt>{move || t().frames}</dt><dd>{total_frames}</dd></div>
                            <div><dt>{move || t().rate}</dt><dd>{move || format!("{} fps", fps.get())}</dd></div>
                            <div><dt>{move || t().duration}</dt><dd>{move || format!("{:.2} s", duration.get())}</dd></div>
                            <div><dt>{move || t().format_short}</dt><dd>{move || codec.get().extension().to_uppercase()}</dd></div>
                        </dl>
                    </div>

                    <div class="card render-card">
                        {move || {
                            let l = lang.get();
                            let tx = texts(l);
                            let p = progress.get();
                            match phase.get() {
                                Phase::Rendering => {
                                    let (frame, total) = p.as_ref().map(|p| (p.frame, p.total)).unwrap_or((0, total_frames()));
                                    let pct = frame as f64 / total.max(1) as f64 * 100.0;
                                    let stage = p.as_ref().map(|p| p.stage).unwrap_or(Stage::Launching);
                                    view! {
                                        <div class="prog-h">
                                            <strong>{stage.label(l)}</strong>
                                            <span>{format!("{pct:.0} %")}</span>
                                        </div>
                                        <div class="bar"><div class="fill" class:indet=stage != Stage::Capturing style=format!("width:{pct:.2}%")></div></div>
                                        <dl class="stats">
                                            <div><dt>{tx.frame}</dt><dd>{format!("{frame} / {total}")}</dd></div>
                                            <div><dt>{tx.speed}</dt><dd>{p.as_ref().map(|p| format!("{:.1} {}", p.render_fps, tx.speed_unit)).unwrap_or("—".into())}</dd></div>
                                            <div><dt>{tx.elapsed}</dt><dd>{p.as_ref().map(|p| fmt_ms(p.elapsed_ms)).unwrap_or("—".into())}</dd></div>
                                            <div><dt>{tx.remaining}</dt><dd>{p.as_ref().and_then(|p| p.eta_ms).map(fmt_ms).unwrap_or("—".into())}</dd></div>
                                        </dl>
                                        <button class="btn danger wide" on:click=move |_| cancel()>{tx.cancel}</button>
                                    }.into_any()
                                }
                                Phase::Done(path) => {
                                    let p1 = path.clone();
                                    let p2 = path.clone();
                                    view! {
                                        <div class="result ok">
                                            <strong>{tx.video_ready}</strong>
                                            <span>{path.clone()}</span>
                                            {p.map(|p| view! { <small>{i18n::frames_in(l, p.total, &fmt_ms(p.elapsed_ms))}</small> })}
                                        </div>
                                        <div class="row">
                                            <button class="btn primary grow" on:click=move |_| open_path("open_file", p1.clone())>{tx.play}</button>
                                            <button class="btn grow" on:click=move |_| open_path("reveal", p2.clone())>{tx.reveal}</button>
                                        </div>
                                        <button class="btn wide ghost" on:click=move |_| start_render() disabled=move || !can_render()>{tx.rerender}</button>
                                    }.into_any()
                                }
                                other => view! {
                                    {match other {
                                        Phase::Error(e) => Some(view! { <div class="result err"><strong>{tx.render_failed}</strong><pre>{e}</pre></div> }.into_any()),
                                        Phase::Cancelled => Some(view! { <div class="result"><strong>{tx.render_cancelled}</strong></div> }.into_any()),
                                        _ => None,
                                    }}
                                    <button class="btn primary big wide" on:click=move |_| start_render() disabled=move || !can_render()>
                                        {tx.export_btn}
                                    </button>
                                    <p class="hint center" class:refused=move || probe_info.get().is_some_and(|i| i.rejection().is_some())>{move || {
                                        let l = lang.get();
                                        let tx = texts(l);
                                        if !tools_ok() { return tx.need_tools.to_string(); }
                                        if input.get().trim().is_empty() { return tx.need_input.to_string(); }
                                        if probing.get() { return tx.analysing.to_string(); }
                                        match probe_info.get() {
                                            None => tx.need_probe.to_string(),
                                            Some(i) => match i.rejection() {
                                                Some(r) => format!("{} {}", tx.export_refused, r.message(l)),
                                                None => tx.ready_hint.to_string(),
                                            },
                                        }
                                    }}</p>
                                }.into_any(),
                            }
                        }}
                    </div>
                </section>
            </main>
            <div class="drop-overlay"><div>{move || t().drop_overlay}</div></div>
        </div>
    }
}
