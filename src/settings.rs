//! Réglages de l'application : proxy et modèle IA, chemins de Chrome et de ffmpeg.
//! Ils sont regroupés dans un panneau ouvert depuis l'en-tête (⚙) et mémorisés sur le poste.

use leptos::prelude::*;
use leptos::task::spawn_local;
use rustmotion_types::{Lang, ToolStatus};
use serde::{Deserialize, Serialize};

use crate::i18n::{self, motion_texts, texts};
use crate::tauri::{self, invoke};

const KEY_URL: &str = "rustmotion.ai.url";
const KEY_MODEL: &str = "rustmotion.ai.model";
const KEY_KEY: &str = "rustmotion.ai.key";
pub const DEFAULT_URL: &str = "http://127.0.0.1:9018/v1";
const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-5.5";

// --- Stockage local (préférences de ce poste) -------------------------------------------

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn load(key: &str, default: &str) -> String {
    storage().and_then(|s| s.get_item(key).ok().flatten()).unwrap_or_else(|| default.to_string())
}

pub fn store(key: &str, value: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, value);
    }
}

// --- Réglages IA ------------------------------------------------------------------------

/// Modèle proposé par le proxy, avec son tarif (USD par jeton) quand il est connu.
#[derive(Deserialize, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub prompt: Option<f64>,
    pub completion: Option<f64>,
}

/// Proxy et modèle IA, partagés par l'onglet Motion design et le panneau de réglages.
#[derive(Clone, Copy)]
pub struct AiSettings {
    pub url: RwSignal<String>,
    pub model: RwSignal<String>,
    pub key: RwSignal<String>,
    /// Modèles proposés par le proxy (`None` pendant l'interrogation).
    pub models: RwSignal<Option<Result<Vec<ModelInfo>, String>>>,
}

impl AiSettings {
    pub fn new() -> Self {
        let s = Self {
            url: RwSignal::new(load(KEY_URL, DEFAULT_URL)),
            model: RwSignal::new(load(KEY_MODEL, DEFAULT_MODEL)),
            key: RwSignal::new(load(KEY_KEY, "")),
            models: RwSignal::new(None),
        };
        s.refresh_models();
        s
    }

    /// URL du proxy et clé facultative, pour un appel.
    pub fn auth(self) -> (String, Option<String>) {
        let key = self.key.get_untracked();
        (self.url.get_untracked(), (!key.trim().is_empty()).then_some(key))
    }

    /// Interroge le proxy pour la liste des modèles (et leurs tarifs).
    pub fn refresh_models(self) {
        if !tauri::available() {
            return;
        }
        #[derive(Serialize)]
        struct A {
            base_url: String,
            api_key: Option<String>,
        }
        let (base_url, api_key) = self.auth();
        let models = self.models;
        models.set(None);
        spawn_local(async move {
            models.set(Some(invoke::<_, Vec<ModelInfo>>("ai_models", &A { base_url, api_key }).await));
        });
    }

    fn set_model(self, v: String) {
        store(KEY_MODEL, &v);
        self.model.set(v);
    }

    fn set_url(self, v: String) {
        let v = if v.is_empty() { DEFAULT_URL.to_string() } else { v };
        store(KEY_URL, &v);
        self.url.set(v);
        self.refresh_models();
    }

    fn set_key(self, v: String) {
        store(KEY_KEY, &v);
        self.key.set(v);
        self.refresh_models();
    }
}

// --- Panneau ----------------------------------------------------------------------------

/// Panneau latéral des réglages. `on_tools` relance la détection de Chrome / ffmpeg ;
/// `pick_exe` ouvre un sélecteur d'exécutable pour le champ donné.
#[allow(clippy::too_many_arguments)]
pub fn settings_panel(
    lang: RwSignal<Lang>,
    open: RwSignal<bool>,
    ai: AiSettings,
    tools: RwSignal<Option<ToolStatus>>,
    browser_path: RwSignal<String>,
    ffmpeg_path: RwSignal<String>,
    on_tools: impl Fn() + Copy + Send + Sync + 'static,
    pick_exe: impl Fn(RwSignal<String>) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let in_tauri = tauri::available();
    let t = move || texts(lang.get());
    let m = move || motion_texts(lang.get());
    let close = move || open.set(false);

    view! {
        <Show when=move || open.get()>
            <div class="modal-scrim" on:click=move |_| close()></div>
            <aside class="settings-panel" role="dialog" aria-modal="true" aria-labelledby="settings-title">
                <header class="settings-h">
                    <h2 id="settings-title">{move || t().settings_title}</h2>
                    <button class="btn ghost close" title=move || t().close aria-label=move || t().close on:click=move |_| close()>"✕"</button>
                </header>

                <section class="settings-sec">
                    <h3>{move || m().ai}</h3>
                    <div class="field">
                        <label class="lbl" for="ai-model">{move || m().ai_model}</label>
                        <input id="ai-model" type="text" list="ai-models" spellcheck="false" prop:value=move || ai.model.get()
                            on:change=move |ev| ai.set_model(event_target_value(&ev).trim().to_string()) />
                        <datalist id="ai-models">
                            {move || ai.models.get().and_then(|r| r.ok()).unwrap_or_default().into_iter()
                                .map(|i| view! { <option value=i.id></option> }).collect_view()}
                        </datalist>
                        <p class="hint small">{move || {
                            let l = lang.get();
                            match ai.models.get() {
                                None if in_tauri => motion_texts(l).models_loading.to_string(),
                                None => motion_texts(l).outside_tauri.to_string(),
                                Some(Ok(list)) if !list.is_empty() => i18n::models_found(l, list.len()),
                                Some(_) => motion_texts(l).models_unavailable.to_string(),
                            }
                        }}</p>
                    </div>
                    <div class="field">
                        <label class="lbl" for="ai-url">{move || m().ai_url}</label>
                        <input id="ai-url" type="text" spellcheck="false" prop:value=move || ai.url.get()
                            on:change=move |ev| ai.set_url(event_target_value(&ev).trim().to_string()) />
                    </div>
                    <div class="field">
                        <label class="lbl" for="ai-key">{move || m().ai_key}</label>
                        <input id="ai-key" type="password" autocomplete="off" placeholder=move || m().ai_key_placeholder
                            prop:value=move || ai.key.get()
                            on:change=move |ev| ai.set_key(event_target_value(&ev).trim().to_string()) />
                        <p class="hint small">{move || m().proxy_hint}</p>
                    </div>
                </section>

                <section class="settings-sec">
                    <h3>{move || t().settings_tools}</h3>
                    {[(true, browser_path), (false, ffmpeg_path)].into_iter().map(|(is_browser, target)| view! {
                        <div class="field">
                            <label class="lbl">{move || if is_browser { t().browser_label } else { t().ffmpeg_label }}</label>
                            <div class="row">
                                <input class="grow" type="text" spellcheck="false" prop:value=move || target.get()
                                    on:change=move |ev| { target.set(event_target_value(&ev).trim().to_string()); on_tools(); } />
                                <button class="btn" on:click=move |_| pick_exe(target)>"…"</button>
                            </div>
                            // Outil réellement utilisé (détecté automatiquement si le champ est vide).
                            <p class="hint small tool-found">{move || {
                                let tx = t();
                                let found = tools.get().and_then(|s| if is_browser { s.browser } else { s.ffmpeg });
                                match found {
                                    Some(p) => format!("✓ {p}"),
                                    None if tools.get().is_none() => tx.detecting_tools.to_string(),
                                    None => (if is_browser { tx.browser_missing } else { tx.ffmpeg_missing }).to_string(),
                                }
                            }}</p>
                        </div>
                    }).collect_view()}
                </section>
            </aside>
        </Show>
    }
}
