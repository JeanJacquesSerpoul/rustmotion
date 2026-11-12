//! Application Tauri : expose le moteur RustMotion à l'interface Leptos.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rustmotion_engine::Outcome;
use rustmotion_types::{Codec, ProbeInfo, RenderSettings, ToolStatus};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct AppState {
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
}

/// Exécute un travail bloquant hors du thread principal.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("tâche interrompue : {e}"))?
}

#[tauri::command]
async fn tool_status(browser: Option<String>, ffmpeg: Option<String>) -> Result<ToolStatus, String> {
    blocking(move || Ok(rustmotion_engine::tool_status(browser.as_deref(), ffmpeg.as_deref()))).await
}

#[tauri::command]
async fn pick_file(app: AppHandle, kind: String) -> Result<Option<String>, String> {
    let d = app.dialog().file();
    let d = match kind.as_str() {
        "html" => d.set_title("Choisir une animation HTML").add_filter("Pages HTML", &["html", "htm"]),
        "audio" => d
            .set_title("Choisir une piste audio")
            .add_filter("Audio", &["mp3", "wav", "aac", "m4a", "ogg", "flac", "opus"]),
        "exe" => d.set_title("Choisir un exécutable").add_filter("Exécutable", &["exe", ""]),
        _ => d,
    };
    let picked = blocking(move || Ok(d.blocking_pick_file())).await?;
    Ok(picked.and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    let d = app.dialog().file().set_title("Choisir un dossier contenant index.html");
    let picked = blocking(move || Ok(d.blocking_pick_folder())).await?;
    Ok(picked.and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

#[tauri::command]
async fn pick_output(app: AppHandle, codec: Codec, current: Option<String>) -> Result<Option<String>, String> {
    let ext = codec.extension();
    let mut d = app
        .dialog()
        .file()
        .set_title("Enregistrer la vidéo")
        .add_filter(codec.label(), &[ext]);
    if let Some(cur) = current.filter(|c| !c.trim().is_empty()) {
        let p = PathBuf::from(cur);
        if let Some(dir) = p.parent().filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        if let Some(name) = p.file_name() {
            d = d.set_file_name(name.to_string_lossy());
        }
    }
    let picked = blocking(move || Ok(d.blocking_save_file())).await?;
    Ok(picked.and_then(|p| p.into_path().ok()).map(|p| {
        // Force l'extension adaptée au codec.
        if p.extension().map(|e| e.eq_ignore_ascii_case(ext)) == Some(true) {
            p.display().to_string()
        } else {
            p.with_extension(ext).display().to_string()
        }
    }))
}

/// Propose un chemin de sortie à côté de la source, sans écraser un fichier existant.
#[tauri::command]
fn suggest_output(app: AppHandle, input: String, codec: Codec) -> String {
    let input = input.trim().trim_matches('"');
    let normalized = if cfg!(windows) { input.replace('/', "\\") } else { input.to_string() };
    let ext = codec.extension();
    let is_url = input.starts_with("http://") || input.starts_with("https://");
    let (dir, stem) = if is_url || input.is_empty() {
        let dir = app.path().video_dir().or_else(|_| app.path().download_dir()).unwrap_or_else(|_| std::env::temp_dir());
        (dir, "rustmotion".to_string())
    } else {
        let p = PathBuf::from(&normalized);
        if p.is_dir() {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "rustmotion".into());
            (p.parent().map(Path::to_path_buf).unwrap_or(p.clone()), name)
        } else {
            let stem = p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "rustmotion".into());
            (p.parent().map(Path::to_path_buf).unwrap_or_default(), stem)
        }
    };
    let mut candidate = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    candidate.display().to_string()
}

#[tauri::command]
async fn probe(input: String, width: u32, height: u32, browser: Option<String>) -> Result<ProbeInfo, String> {
    blocking(move || {
        rustmotion_engine::probe(&input, width, height, browser.as_deref()).map_err(|e| format!("{e:#}"))
    })
    .await
}

/// Lance un rendu. Renvoie le chemin produit, ou `None` si annulé.
#[tauri::command]
async fn render(app: AppHandle, state: State<'_, AppState>, settings: RenderSettings) -> Result<Option<String>, String> {
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("un rendu est déjà en cours".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let cancel = state.cancel.clone();
    let busy = state.busy.clone();
    let res = blocking(move || {
        let out = rustmotion_engine::render(&settings, &cancel, |p| {
            let _ = app.emit("render-progress", &p);
        });
        match out {
            Ok(Outcome::Done(p)) => Ok(Some(p.display().to_string())),
            Ok(Outcome::Cancelled) => Ok(None),
            Err(e) => Err(format!("{e:#}")),
        }
    })
    .await;
    busy.store(false, Ordering::SeqCst);
    res
}

#[tauri::command]
fn cancel_render(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_file(path: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            tool_status,
            pick_file,
            pick_folder,
            pick_output,
            suggest_output,
            probe,
            render,
            cancel_render,
            reveal,
            open_file
        ])
        .on_window_event(|window, event| {
            // Fermer la fenêtre annule un rendu en cours (le navigateur et ffmpeg sont arrêtés).
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                window.state::<AppState>().cancel.store(true, Ordering::SeqCst);
            }
        })
        .run(tauri::generate_context!())
        .expect("erreur au démarrage de RustMotion");
}
