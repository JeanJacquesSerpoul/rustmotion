//! Motion design par IA : appel du proxy OpenRouter (API compatible OpenAI, en streaming),
//! lecture d'un brief (.txt / .md / .pdf) et enregistrement de l'animation HTML produite.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustmotion_engine::tr;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::blocking;

/// Taille maximale d'un brief importé.
const MAX_PROMPT_FILE: u64 = 50 * 1024 * 1024;

#[derive(Default)]
pub struct AiState {
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
pub struct AiRequest {
    base_url: String,
    api_key: Option<String>,
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize, Clone)]
struct AiProgress {
    /// Caractères de réponse reçus.
    chars: usize,
    /// Caractères de « réflexion » reçus (modèles qui raisonnent avant de répondre).
    reasoning: usize,
    /// Fin de la réponse reçue, pour l'afficher en direct.
    tail: String,
    elapsed_ms: u64,
}

#[derive(Serialize)]
pub struct AiResult {
    html: String,
    /// La réponse a été coupée (limite de jetons atteinte) : l'animation est probablement incomplète.
    truncated: bool,
    model: Option<String>,
    /// Coût réel facturé par OpenRouter (USD), s'il est communiqué.
    cost: Option<f64>,
}

/// Coût réel d'un appel, communiqué par OpenRouter dans `usage.cost`.
fn usage_cost(v: &Value) -> Option<f64> {
    v["usage"]["cost"].as_f64().filter(|c| *c >= 0.0)
}

#[derive(Serialize)]
pub struct PromptFile {
    name: String,
    text: String,
}

/// Racine de l'API (`…/v1`) à partir de l'URL saisie.
fn api_base(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let base = base.strip_suffix("/chat/completions").unwrap_or(base);
    if base.ends_with("/v1") {
        base.to_string()
    } else {
        format!("{base}/v1")
    }
}

/// URL complète de `/chat/completions`.
fn endpoint(base: &str) -> String {
    format!("{}/chat/completions", api_base(base))
}

/// Modèles proposés par le proxy (vide si la route `/models` n'est pas autorisée).
/// Arguments en snake_case : le front envoie `base_url` / `api_key`.
#[tauri::command(rename_all = "snake_case")]
pub async fn ai_models(base_url: String, api_key: Option<String>) -> Result<Vec<ModelInfo>, String> {
    blocking(move || {
        let url = format!("{}/models", api_base(&base_url));
        let mut call = ureq::get(&url).timeout(Duration::from_secs(15));
        if let Some(key) = api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
            call = call.set("Authorization", &format!("Bearer {key}"));
        }
        let text = call.call().map_err(|e| http_error(&url, e))?.into_string().map_err(|e| e.to_string())?;
        let v: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", tr!("réponse illisible", "unreadable response")))?;
        let mut models: Vec<ModelInfo> = v["data"]
            .as_array()
            .map(|a| a.iter().filter_map(model_info).collect())
            .unwrap_or_default();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    })
    .await
}

/// Modèle proposé par le proxy, avec son tarif OpenRouter (USD par jeton) quand il est connu.
#[derive(Serialize)]
pub struct ModelInfo {
    id: String,
    prompt: Option<f64>,
    completion: Option<f64>,
}

fn model_info(m: &Value) -> Option<ModelInfo> {
    // OpenRouter donne les tarifs en chaînes ("0.000003") ; "-1" signale un tarif variable.
    let price = |k: &str| {
        let p = &m["pricing"][k];
        p.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| p.as_f64()).filter(|v| *v >= 0.0)
    };
    Some(ModelInfo { id: m["id"].as_str()?.to_string(), prompt: price("prompt"), completion: price("completion") })
}

/// Isole le document HTML dans la réponse (ignore les balises ``` et le texte autour).
fn extract_html(s: &str) -> Option<String> {
    let lower = s.to_ascii_lowercase();
    let start = lower.find("<!doctype").or_else(|| lower.find("<html"))?;
    let end = lower.rfind("</html>").map(|i| i + "</html>".len()).filter(|&e| e > start).unwrap_or(s.len());
    Some(s[start..end].trim().to_string())
}

fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect()
}

fn api_error(v: &Value) -> Option<String> {
    let e = v.get("error")?;
    Some(e.get("message").and_then(Value::as_str).map(String::from).unwrap_or_else(|| e.to_string()))
}

fn http_error(url: &str, err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&body).ok().and_then(|v| api_error(&v)).unwrap_or(body);
            let hint = match code {
                401 => tr!("clé du proxy absente ou refusée", "proxy key missing or rejected"),
                402 => tr!("crédit OpenRouter insuffisant", "insufficient OpenRouter credit"),
                403 => tr!("route refusée par le proxy", "route refused by the proxy"),
                404 => tr!("route ou modèle introuvable", "route or model not found"),
                408 | 504 => tr!("délai dépassé", "timed out"),
                413 => tr!("requête trop volumineuse (brief trop long ?)", "request too large (brief too long?)"),
                429 => tr!("trop de requêtes, réessayez plus tard", "too many requests, try again later"),
                _ => "",
            };
            let mut msg = format!("HTTP {code}");
            if !hint.is_empty() {
                msg.push_str(&format!(" — {hint}"));
            }
            if !detail.trim().is_empty() {
                msg.push_str(&format!("\n{}", detail.trim()));
            }
            msg
        }
        ureq::Error::Transport(t) => format!(
            "{} {url}\n{t}",
            tr!("Proxy IA injoignable :", "AI proxy unreachable:")
        ),
    }
}

/// Envoie la conversation au proxy et renvoie le document HTML produit (`None` si annulé).
#[tauri::command]
pub async fn ai_generate(app: AppHandle, state: State<'_, AiState>, req: AiRequest) -> Result<Option<AiResult>, String> {
    if req.model.trim().is_empty() {
        return Err(tr!("choisissez un modèle", "choose a model").into());
    }
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err(tr!("une génération est déjà en cours", "a generation is already running").into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let cancel = state.cancel.clone();
    let busy = state.busy.clone();
    let res = blocking(move || {
        generate(&req, &cancel, |p| {
            let _ = app.emit("ai-progress", p);
        })
    })
    .await;
    busy.store(false, Ordering::SeqCst);
    res
}

/// Réponse complète d'un modèle de texte.
struct Chat {
    content: String,
    model: Option<String>,
    finish: Option<String>,
    cost: Option<f64>,
}

fn generate(req: &AiRequest, cancel: &AtomicBool, on_progress: impl FnMut(AiProgress)) -> Result<Option<AiResult>, String> {
    let Some(Chat { content, model, finish, cost }) = chat(req, cancel, on_progress)? else { return Ok(None) };
    let html = extract_html(&content).ok_or_else(|| {
        format!(
            "{}\n\n{}",
            tr!("la réponse ne contient pas de document HTML :", "the response contains no HTML document:"),
            tail(&content, 600)
        )
    })?;
    let truncated = finish.as_deref() == Some("length") || !html.to_ascii_lowercase().contains("</html>");
    Ok(Some(AiResult { html, truncated, model, cost }))
}

/// Envoie la conversation au proxy (en streaming si possible) et renvoie la réponse complète.
fn chat(req: &AiRequest, cancel: &AtomicBool, mut on_progress: impl FnMut(AiProgress)) -> Result<Option<Chat>, String> {
    let url = endpoint(&req.base_url);
    let body = json!({
        "model": req.model.trim(),
        "messages": req.messages,
        "stream": true,
        "usage": {"include": true},
    });
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        // Délai entre deux paquets : un modèle peut réfléchir longtemps avant d'écrire.
        .timeout_read(Duration::from_secs(600))
        .build();
    let mut call = agent.post(&url).set("Content-Type", "application/json").set("Accept", "text/event-stream");
    if let Some(key) = req.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        call = call.set("Authorization", &format!("Bearer {key}"));
    }
    let t0 = Instant::now();
    let resp = call.send_string(&body.to_string()).map_err(|e| http_error(&url, e))?;
    if cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }

    let streamed = resp.content_type().contains("event-stream");
    let mut content = String::new();
    let mut model = None;
    let mut finish = None::<String>;
    let mut cost = None;

    if streamed {
        let mut reasoning = 0usize;
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let reader = BufReader::new(resp.into_reader());
        for line in reader.lines() {
            if cancel.load(Ordering::SeqCst) {
                return Ok(None);
            }
            let line = line.map_err(|e| format!("{}: {e}", tr!("flux interrompu", "stream interrupted")))?;
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
            if let Some(e) = api_error(&v) {
                return Err(e);
            }
            if model.is_none() {
                model = v.get("model").and_then(Value::as_str).map(String::from);
            }
            cost = usage_cost(&v).or(cost);
            let choice = &v["choices"][0];
            if let Some(s) = choice["delta"]["content"].as_str() {
                content.push_str(s);
            }
            if let Some(s) = choice["delta"]["reasoning"].as_str() {
                reasoning += s.chars().count();
            }
            if let Some(f) = choice["finish_reason"].as_str() {
                finish = Some(f.to_string());
            }
            if last_emit.elapsed() >= Duration::from_millis(150) {
                last_emit = Instant::now();
                on_progress(AiProgress {
                    chars: content.chars().count(),
                    reasoning,
                    tail: tail(&content, 1200),
                    elapsed_ms: t0.elapsed().as_millis() as u64,
                });
            }
        }
    } else {
        // Proxy ou modèle sans streaming : réponse JSON complète.
        let mut text = String::new();
        resp.into_reader().read_to_string(&mut text).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", tr!("réponse illisible", "unreadable response")))?;
        if let Some(e) = api_error(&v) {
            return Err(e);
        }
        model = v.get("model").and_then(Value::as_str).map(String::from);
        cost = usage_cost(&v);
        content = v["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_string();
        finish = v["choices"][0]["finish_reason"].as_str().map(String::from);
    }

    if content.trim().is_empty() {
        return Err(tr!("le modèle n'a renvoyé aucun contenu", "the model returned no content").into());
    }
    Ok(Some(Chat { content, model, finish, cost }))
}

#[tauri::command]
pub fn ai_cancel(state: State<'_, AiState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// Lit un brief : texte brut (.txt, .md…) ou texte extrait d'un PDF.
#[tauri::command]
pub async fn read_prompt_file(path: String) -> Result<PromptFile, String> {
    blocking(move || {
        let p = PathBuf::from(path.trim().trim_matches('"'));
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let size = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?.len();
        if size > MAX_PROMPT_FILE {
            return Err(tr!("fichier trop volumineux (50 Mo max)", "file too large (50 MB max)").into());
        }
        let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let is_pdf = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) || bytes.starts_with(b"%PDF");
        let text = if is_pdf {
            // Certaines bibliothèques PDF paniquent sur des fichiers atypiques : on isole l'extraction.
            let out = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(&bytes));
            match out {
                Ok(Ok(t)) => t,
                Ok(Err(e)) => return Err(format!("{}: {e}", tr!("lecture du PDF impossible", "cannot read the PDF"))),
                Err(_) => return Err(tr!("lecture du PDF impossible (format non pris en charge)", "cannot read the PDF (unsupported format)").into()),
            }
        } else {
            String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes)).into_owned()
        };
        let text = tidy(&text);
        if text.is_empty() {
            return Err(if is_pdf {
                tr!("aucun texte extractible (PDF scanné ?)", "no extractable text (scanned PDF?)")
            } else {
                tr!("le fichier est vide", "the file is empty")
            }
            .into());
        }
        Ok(PromptFile { name, text })
    })
    .await
}

/// Normalise les fins de ligne et réduit les longues suites de lignes vides.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank = 0;
    for line in s.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

/// Nom de fichier tiré du `<title>` du document.
fn slug_from_title(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let title = lower
        .find("<title>")
        .map(|i| i + "<title>".len())
        .and_then(|s| lower[s..].find("</title>").map(|e| &html[s..s + e]))
        .unwrap_or("");
    let mut slug = String::new();
    for c in title.to_lowercase().chars() {
        let c = match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "motion".into()
    } else {
        slug
    }
}

fn motion_dir(app: &AppHandle) -> PathBuf {
    let base = app
        .path()
        .video_dir()
        .or_else(|_| app.path().document_dir())
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("RustMotion")
}

fn unique_path(dir: &Path, stem: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.html"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}-{n}.html"));
        n += 1;
    }
    candidate
}

/// Enregistre l'animation générée (chaque version dans un nouveau fichier) et renvoie son chemin.
#[tauri::command]
pub fn save_motion(app: AppHandle, html: String) -> Result<String, String> {
    let dir = motion_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = unique_path(&dir, &slug_from_title(&html));
    std::fs::write(&path, html).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

/// Motion design lu sur disque, avec le format déclaré dans ses balises `rustmotion:`.
#[derive(Serialize)]
pub struct OpenedMotion {
    html: String,
    title: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<f64>,
    /// Bande son compagnon (même nom, extension audio), si elle existe.
    audio: Option<String>,
}

/// Contenu de `<meta name="rustmotion:{key}" content="…">` (ordre des attributs indifférent).
fn rustmotion_meta(html: &str, key: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let name = format!("rustmotion:{key}");
    let mut from = 0;
    while let Some(i) = lower[from..].find("<meta") {
        let start = from + i;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        let tag = &lower[start..end];
        from = end;
        let named = ["\"", "'"].iter().any(|q| tag.contains(&format!("name={q}{name}{q}")));
        if !named {
            continue;
        }
        let c = tag.find("content=")? + "content=".len();
        let quote = tag[c..].chars().next().filter(|q| *q == '"' || *q == '\'')?;
        let v = c + 1;
        let len = tag[v..].find(quote)?;
        // Valeur reprise du document d'origine (les indices ASCII coïncident).
        return Some(html[start + v..start + v + len].trim().to_string());
    }
    None
}

fn html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let s = lower.find("<title>")? + "<title>".len();
    let e = s + lower[s..].find("</title>")?;
    Some(html[s..e].trim().to_string()).filter(|t| !t.is_empty())
}

/// Lit l'animation choisie dans la barre commune (fichier HTML, ou dossier contenant
/// `index.html`) pour l'afficher dans l'onglet Motion design. Le format vient des balises
/// `rustmotion:` quand elles existent. Renvoie `None` pour une URL ou un autre type de fichier.
#[tauri::command]
pub async fn open_motion(path: String) -> Result<Option<OpenedMotion>, String> {
    blocking(move || {
        let mut p = PathBuf::from(path.trim().trim_matches('"'));
        if p.is_dir() {
            p = p.join("index.html");
        }
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm")) || !p.is_file() {
            return Ok(None);
        }
        let size = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?.len();
        if size > MAX_PROMPT_FILE {
            return Err(tr!("fichier trop volumineux (50 Mo max)", "file too large (50 MB max)").into());
        }
        let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let html = String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes)).into_owned();
        let num = |k: &str| rustmotion_meta(&html, k);
        let (width, height) = (num("width").and_then(|v| v.parse().ok()), num("height").and_then(|v| v.parse().ok()));
        let duration = num("duration").and_then(|v| v.parse().ok()).filter(|d: &f64| *d > 0.0);
        let audio = companion_of(&p).map(|a| a.display().to_string());
        Ok(Some(OpenedMotion { title: html_title(&html), html, width, height, duration, audio }))
    })
    .await
}

/// Enregistre une copie du fichier d'animation à l'emplacement choisi par l'utilisateur
/// (dossier et nom proposés d'après l'original). Renvoie `None` si la boîte est annulée.
#[tauri::command]
pub async fn save_copy(app: AppHandle, path: String) -> Result<Option<String>, String> {
    let src = PathBuf::from(path.trim().trim_matches('"'));
    if !src.is_file() {
        return Err(format!("{}: {}", src.display(), tr!("fichier introuvable", "file not found")));
    }
    let ext = src.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "html".into());
    let mut d = app
        .dialog()
        .file()
        .set_title(tr!("Sauvegarder l'animation", "Save the animation"))
        .add_filter(tr!("Page HTML", "HTML page"), &[ext.as_str()]);
    if let Some(dir) = src.parent().filter(|d| d.is_dir()) {
        d = d.set_directory(dir);
    }
    if let Some(name) = src.file_name() {
        d = d.set_file_name(name.to_string_lossy());
    }
    blocking(move || {
        let Some(dest) = d.blocking_save_file().and_then(|p| p.into_path().ok()) else {
            return Ok(None);
        };
        let dest = if dest.extension().is_some() { dest } else { dest.with_extension(&ext) };
        // Même fichier : rien à copier (une copie sur lui-même le viderait).
        let same = matches!((dest.canonicalize(), src.canonicalize()), (Ok(a), Ok(b)) if a == b);
        if !same {
            std::fs::copy(&src, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
        Ok(Some(dest.display().to_string()))
    })
    .await
}

// --- Bande son ----------------------------------------------------------------------------
// Elle est rattachée à l'animation par son nom : « Mon anim.html » + « Mon anim.mp3 ».

const MAX_AUDIO_FILE: u64 = 100 * 1024 * 1024;

fn resolve_html(path: &str) -> Result<PathBuf, String> {
    let mut p = PathBuf::from(path.trim().trim_matches('"'));
    if p.is_dir() {
        p = p.join("index.html");
    }
    if !p.is_file() {
        return Err(format!("{}: {}", p.display(), tr!("fichier introuvable", "file not found")));
    }
    Ok(p)
}

/// Bande son compagnon d'une page HTML.
fn companion_of(html: &Path) -> Option<PathBuf> {
    let stem = html.file_stem()?.to_string_lossy();
    rustmotion_engine::source::companion_audio(html.parent()?, &stem)
}

/// Écarte les bandes son compagnon existantes (renommées, jamais supprimées) avant d'en
/// installer une nouvelle, pour que la détection ne retombe pas sur l'ancienne.
fn set_aside_companions(html: &Path) -> Result<Vec<PathBuf>, String> {
    let mut moved = Vec::new();
    let (Some(dir), Some(stem)) = (html.parent(), html.file_stem().map(|s| s.to_string_lossy().into_owned())) else {
        return Ok(moved);
    };
    for ext in rustmotion_types::AUDIO_EXTS {
        let old = dir.join(format!("{stem}.{ext}"));
        if !old.is_file() {
            continue;
        }
        let label = tr!("ancienne bande son", "previous soundtrack");
        let mut dest = dir.join(format!("{stem} ({label}).{ext}"));
        let mut n = 2;
        while dest.exists() {
            dest = dir.join(format!("{stem} ({label} {n}).{ext}"));
            n += 1;
        }
        std::fs::rename(&old, &dest).map_err(|e| format!("{}: {e}", old.display()))?;
        moved.push(dest);
    }
    Ok(moved)
}

/// Retire la bande son de l'animation : le fichier est renommé (jamais supprimé).
/// Renvoie son nouveau chemin.
#[tauri::command(rename_all = "snake_case")]
pub async fn detach_audio(html_path: String) -> Result<Option<String>, String> {
    blocking(move || {
        let html = resolve_html(&html_path)?;
        Ok(set_aside_companions(&html)?.into_iter().next().map(|p| p.display().to_string()))
    })
    .await
}

/// Installe `bytes` comme bande son compagnon de la page et renvoie son chemin.
fn install_audio(html: &Path, bytes: &[u8], ext: &str) -> Result<PathBuf, String> {
    set_aside_companions(html)?;
    let stem = html.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "animation".into());
    let dest = html.with_file_name(format!("{stem}.{ext}"));
    std::fs::write(&dest, bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(dest)
}

/// Extension d'après la signature du fichier audio (MP3 par défaut).
fn audio_ext(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        "wav"
    } else if bytes.starts_with(b"OggS") {
        "ogg"
    } else if bytes.starts_with(b"fLaC") {
        "flac"
    } else if bytes.get(4..8) == Some(b"ftyp") {
        "m4a"
    } else {
        "mp3"
    }
}

fn audio_mime(ext: &str) -> &'static str {
    match ext {
        "wav" => "audio/wav",
        "ogg" | "opus" => "audio/ogg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        _ => "audio/mpeg",
    }
}

/// Utilise un fichier audio existant comme bande son de l'animation (copié à côté d'elle,
/// sous son nom). Renvoie le chemin de la bande son installée.
#[tauri::command(rename_all = "snake_case")]
pub async fn attach_audio(html_path: String, audio_path: String) -> Result<String, String> {
    blocking(move || {
        let html = resolve_html(&html_path)?;
        let src = PathBuf::from(audio_path.trim().trim_matches('"'));
        let ext = src.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if !rustmotion_types::AUDIO_EXTS.contains(&ext.as_str()) {
            return Err(format!("{}: {}", src.display(), tr!("format audio non pris en charge", "unsupported audio format")));
        }
        // Déjà la bande son de cette animation : rien à faire.
        if let Some(cur) = companion_of(&html) {
            if matches!((cur.canonicalize(), src.canonicalize()), (Ok(a), Ok(b)) if a == b) {
                return Ok(cur.display().to_string());
            }
        }
        let size = std::fs::metadata(&src).map_err(|e| format!("{}: {e}", src.display()))?.len();
        if size > MAX_AUDIO_FILE {
            return Err(tr!("fichier audio trop volumineux (100 Mo max)", "audio file too large (100 MB max)").into());
        }
        let bytes = std::fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        Ok(install_audio(&html, &bytes, &ext)?.display().to_string())
    })
    .await
}

/// Bande son en data URL, pour l'écouter dans l'aperçu avec l'animation.
#[tauri::command]
pub async fn read_audio(path: String) -> Result<String, String> {
    use base64::Engine;
    blocking(move || {
        let p = PathBuf::from(path.trim().trim_matches('"'));
        let size = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?.len();
        if size > MAX_AUDIO_FILE {
            return Err(tr!("fichier audio trop volumineux (100 Mo max)", "audio file too large (100 MB max)").into());
        }
        let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        Ok(format!("data:{};base64,{}", audio_mime(&ext), base64::engine::general_purpose::STANDARD.encode(bytes)))
    })
    .await
}

#[derive(Deserialize)]
pub struct MusicRequest {
    base_url: String,
    api_key: Option<String>,
    model: String,
    prompt: String,
    /// Animation à laquelle rattacher la bande son.
    html_path: String,
}

#[derive(Serialize)]
pub struct MusicResult {
    path: String,
    /// Coût réel facturé par OpenRouter (USD), s'il est communiqué.
    cost: Option<f64>,
}

/// Compose une bande son avec un modèle musical (Lyria) et l'installe à côté de l'animation.
/// Renvoie son chemin et son coût, ou `None` si la génération est annulée.
#[tauri::command]
pub async fn ai_soundtrack(state: State<'_, AiState>, req: MusicRequest) -> Result<Option<MusicResult>, String> {
    let html = resolve_html(&req.html_path)?;
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err(tr!("une génération est déjà en cours", "a generation is already running").into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let cancel = state.cancel.clone();
    let busy = state.busy.clone();
    let res = blocking(move || {
        let Some((bytes, cost)) = compose(&req, &cancel)? else { return Ok(None) };
        let path = install_audio(&html, &bytes, audio_ext(&bytes))?.display().to_string();
        Ok(Some(MusicResult { path, cost }))
    })
    .await;
    busy.store(false, Ordering::SeqCst);
    res
}

#[derive(Deserialize)]
pub struct MusicCodeRequest {
    /// Conversation pour le modèle de l'animation (consignes + animation).
    chat: AiRequest,
    /// Animation à laquelle rattacher la bande son.
    html_path: String,
    duration: f64,
    /// Navigateur imposé dans les réglages (sinon détecté).
    browser_path: Option<String>,
}

/// Bande son écrite par le modèle de l'animation : il produit du code Web Audio, exécuté hors
/// écran dans le navigateur headless ; le WAV obtenu est installé à côté de l'animation.
#[tauri::command]
pub async fn ai_soundtrack_code(state: State<'_, AiState>, req: MusicCodeRequest) -> Result<Option<MusicResult>, String> {
    let html = resolve_html(&req.html_path)?;
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err(tr!("une génération est déjà en cours", "a generation is already running").into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let cancel = state.cancel.clone();
    let busy = state.busy.clone();
    let res = blocking(move || {
        let Some(answer) = chat(&req.chat, &cancel, |_| {})? else { return Ok(None) };
        if answer.finish.as_deref() == Some("length") {
            return Err(tr!(
                "réponse coupée par la limite du modèle : le code de la bande son est incomplet",
                "response cut by the model's limit: the soundtrack code is incomplete"
            )
            .into());
        }
        let code = rustmotion_engine::audio::extract_js(&answer.content).ok_or_else(|| {
            format!(
                "{}\n\n{}",
                tr!("la réponse ne contient pas de fonction compose() :", "the response contains no compose() function:"),
                tail(&answer.content, 600)
            )
        })?;
        if cancel.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let wav = rustmotion_engine::audio::render_code(&code, req.duration, req.browser_path.as_deref().filter(|p| !p.trim().is_empty()))
            .map_err(|e| format!("{e:#}"))?;
        let path = install_audio(&html, &wav, "wav")?.display().to_string();
        Ok(Some(MusicResult { path, cost: answer.cost }))
    })
    .await;
    busy.store(false, Ordering::SeqCst);
    res
}

/// Appel du modèle musical. OpenRouter renvoie l'audio en base64 dans le flux SSE
/// (`choices[0].delta.audio.data`, éventuellement en plusieurs morceaux).
fn compose(req: &MusicRequest, cancel: &AtomicBool) -> Result<Option<(Vec<u8>, Option<f64>)>, String> {
    use base64::Engine;
    let url = endpoint(&req.base_url);
    let body = json!({
        "model": req.model.trim(),
        "messages": [{"role": "user", "content": req.prompt}],
        "modalities": ["text", "audio"],
        "audio": {"format": "mp3"},
        "stream": true,
        "usage": {"include": true},
    });
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(600))
        .build();
    let mut call = agent.post(&url).set("Content-Type", "application/json").set("Accept", "text/event-stream");
    if let Some(key) = req.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        call = call.set("Authorization", &format!("Bearer {key}"));
    }
    let resp = call.send_string(&body.to_string()).map_err(|e| http_error(&url, e))?;
    let mut b64 = String::new();
    let mut text = String::new();
    let mut cost = None;
    if resp.content_type().contains("event-stream") {
        let reader = BufReader::new(resp.into_reader());
        for line in reader.lines() {
            // Le proxy envoie des commentaires d'attente pendant la composition : l'annulation reste réactive.
            if cancel.load(Ordering::SeqCst) {
                return Ok(None);
            }
            let line = line.map_err(|e| format!("{}: {e}", tr!("flux interrompu", "stream interrupted")))?;
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
            if let Some(e) = api_error(&v) {
                return Err(e);
            }
            cost = usage_cost(&v).or(cost);
            let delta = &v["choices"][0]["delta"];
            if let Some(s) = delta["audio"]["data"].as_str() {
                b64.push_str(s);
            }
            if let Some(s) = delta["content"].as_str() {
                text.push_str(s);
            }
        }
    } else {
        let mut raw = String::new();
        resp.into_reader().read_to_string(&mut raw).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", tr!("réponse illisible", "unreadable response")))?;
        if let Some(e) = api_error(&v) {
            return Err(e);
        }
        cost = usage_cost(&v);
        let msg = &v["choices"][0]["message"];
        b64 = msg["audio"]["data"].as_str().unwrap_or_default().to_string();
        text = msg["content"].as_str().unwrap_or_default().to_string();
    }
    if cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }
    if b64.trim().is_empty() {
        return Err(format!(
            "{}\n{}",
            tr!("le modèle n'a renvoyé aucun audio", "the model returned no audio"),
            tail(&text, 400)
        ));
    }
    base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map(|bytes| Some((bytes, cost)))
        .map_err(|e| format!("{}: {e}", tr!("audio illisible", "unreadable audio")))
}

/// Fichier récent encore présent sur le disque.
#[derive(Serialize)]
pub struct RecentFile {
    path: String,
    /// Nom affiché (fichier HTML, ou dossier contenant index.html).
    name: String,
    /// Dossier parent.
    dir: String,
    /// Une bande son compagnon existe.
    audio: bool,
    /// Dernière modification (secondes depuis 1970).
    modified: Option<u64>,
}

/// Informations sur les animations récentes ; celles qui n'existent plus sont omises.
#[tauri::command]
pub async fn recent_files(paths: Vec<String>) -> Result<Vec<RecentFile>, String> {
    blocking(move || {
        Ok(paths
            .into_iter()
            .filter_map(|path| {
                let p = PathBuf::from(path.trim().trim_matches('"'));
                let html = if p.is_dir() { p.join("index.html") } else { p.clone() };
                if !html.is_file() {
                    return None;
                }
                let modified = std::fs::metadata(&html)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                Some(RecentFile {
                    name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone()),
                    dir: p.parent().map(|d| d.display().to_string()).unwrap_or_default(),
                    audio: companion_of(&html).is_some(),
                    modified,
                    path,
                })
            })
            .collect())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_signatures() {
        assert_eq!(audio_ext(b"ID3\x03\x00\x00"), "mp3");
        assert_eq!(audio_ext(b"RIFF\x00\x00\x00\x00WAVEfmt "), "wav");
        assert_eq!(audio_ext(b"OggS\x00"), "ogg");
        assert_eq!(audio_ext(b"\x00\x00\x00\x20ftypM4A "), "m4a");
    }

    #[test]
    fn endpoints() {
        assert_eq!(endpoint("http://127.0.0.1:9018/v1"), "http://127.0.0.1:9018/v1/chat/completions");
        assert_eq!(endpoint("http://127.0.0.1:9018/"), "http://127.0.0.1:9018/v1/chat/completions");
        assert_eq!(endpoint("https://x.fr/openrouter/v1/"), "https://x.fr/openrouter/v1/chat/completions");
        assert_eq!(endpoint("https://x.fr/v1/chat/completions"), "https://x.fr/v1/chat/completions");
    }

    #[test]
    fn html_extraction() {
        let r = "Voici :\n```html\n<!DOCTYPE html><html><body>x</body></html>\n```\nBonne journée";
        assert_eq!(extract_html(r).unwrap(), "<!DOCTYPE html><html><body>x</body></html>");
        assert_eq!(extract_html("<html><body>coupé").unwrap(), "<html><body>coupé");
        assert!(extract_html("pas de html").is_none());
    }

    #[test]
    fn motion_meta() {
        let h = r#"<head><META content="1080" name="rustmotion:height"><meta name='rustmotion:width' content='1920'>
            <meta name="rustmotion:duration" content=" 8.5 "><title> Saut </title></head>"#;
        assert_eq!(rustmotion_meta(h, "width").as_deref(), Some("1920"));
        assert_eq!(rustmotion_meta(h, "height").as_deref(), Some("1080"));
        assert_eq!(rustmotion_meta(h, "duration").as_deref(), Some("8.5"));
        assert_eq!(rustmotion_meta(h, "fps"), None);
        assert_eq!(html_title(h).as_deref(), Some("Saut"));
    }

    #[test]
    fn model_pricing() {
        let m = model_info(&json!({"id": "a/b", "pricing": {"prompt": "0.000003", "completion": "0.000015"}})).unwrap();
        assert_eq!((m.prompt, m.completion), (Some(0.000003), Some(0.000015)));
        let m = model_info(&json!({"id": "openrouter/auto", "pricing": {"prompt": "-1"}})).unwrap();
        assert_eq!((m.prompt, m.completion), (None, None));
        assert!(model_info(&json!({"name": "sans id"})).is_none());
    }

    #[test]
    fn slugs() {
        assert_eq!(slug_from_title("<title>Lancement Été 2026 !</title>"), "lancement-ete-2026");
        assert_eq!(slug_from_title("<html></html>"), "motion");
    }

    /// Essai réel via le proxy : `RM_AI_OUT=chemin.html cargo test -p rustmotion --lib live -- --ignored --nocapture`
    /// (optionnel : RM_AI_URL, RM_AI_MODEL, RM_AI_KEY, RM_AI_BRIEF).
    #[test]
    #[ignore]
    fn live_generation() {
        use rustmotion_types::motion::{brief_message, system_prompt};
        let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        let brief = env(
            "RM_AI_BRIEF",
            "Annonce de lancement de « RustMotion 0.3 » : nouvel onglet Motion design. Ton premium, sombre, accent doré. Trois messages : le titre, « Du brief à la vidéo », « Généré par IA, rendu image par image ».",
        );
        let req = AiRequest {
            base_url: env("RM_AI_URL", "http://127.0.0.1:9018/v1"),
            api_key: std::env::var("RM_AI_KEY").ok(),
            model: env("RM_AI_MODEL", "anthropic/claude-sonnet-5.5"),
            messages: vec![
                ChatMessage { role: "system".into(), content: system_prompt(1280, 720, 6.0) },
                ChatMessage { role: "user".into(), content: brief_message(&brief) },
            ],
        };
        let mut last = 0;
        let r = generate(&req, &AtomicBool::new(false), |p| {
            if p.chars / 2000 != last {
                last = p.chars / 2000;
                eprintln!("{} caractères, {} ms", p.chars, p.elapsed_ms);
            }
        })
        .expect("génération")
        .expect("non annulée");
        eprintln!("modèle : {:?}, tronqué : {}", r.model, r.truncated);
        assert!(r.html.to_ascii_lowercase().contains("rustmotion:duration"));
        std::fs::write(env("RM_AI_OUT", "motion-live.html"), &r.html).unwrap();
    }

    #[test]
    fn tidy_text() {
        assert_eq!(tidy("a\r\n\r\n\r\n\r\nb  \n"), "a\n\nb");
    }
}
