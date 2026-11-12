//! Mini serveur HTTP statique local : permet aux pages d'utiliser les modules ES,
//! `fetch()` (Lottie JSON…), les polices, etc., ce que `file://` interdit.

use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::{anyhow, Result};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use tiny_http::{Header, Response, Server};

use crate::tr;

pub struct StaticServer {
    server: Arc<Server>,
    handle: Option<JoinHandle<()>>,
    missing: Arc<Mutex<Vec<String>>>,
    pub port: u16,
}

const PATH_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

impl StaticServer {
    /// Sert `root`. `inject` = (page, script) : le script est ajouté en fin de page s'il ne le charge pas.
    pub fn start(root: &Path, inject: Option<(PathBuf, String)>) -> Result<Self> {
        let root = root.canonicalize()?;
        let inject = inject.and_then(|(page, script)| page.canonicalize().ok().map(|p| (p, script)));
        let server = Arc::new(Server::http("127.0.0.1:0").map_err(|e| anyhow!(tr!(format!("serveur local : {e}"), format!("local server: {e}"))))?);
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| anyhow!(tr!("adresse du serveur local inconnue", "unknown local server address")))?;
        let missing = Arc::new(Mutex::new(Vec::<String>::new()));
        let (srv, missing2) = (server.clone(), missing.clone());
        let handle = std::thread::spawn(move || {
            for req in srv.incoming_requests() {
                let url = req.url().split(['?', '#']).next().unwrap_or("/").to_string();
                let decoded = percent_decode_str(&url).decode_utf8_lossy().to_string();
                let resp = match resolve(&root, &decoded) {
                    Some(path) => {
                        let injected = inject
                            .as_ref()
                            .filter(|(page, _)| *page == path)
                            .and_then(|(_, script)| std::fs::read_to_string(&path).ok().map(|html| with_script(&html, script)));
                        match injected {
                            Some(html) => Response::from_string(html)
                                .with_header(header("Content-Type", mime_for(&path)))
                                .with_header(header("Cache-Control", "no-store"))
                                .boxed(),
                            None => match File::open(&path) {
                                Ok(f) => Response::from_file(f)
                                    .with_header(header("Content-Type", mime_for(&path)))
                                    .with_header(header("Cache-Control", "no-store"))
                                    .with_header(header("Access-Control-Allow-Origin", "*"))
                                    .boxed(),
                                Err(_) => Response::from_string("403").with_status_code(403).boxed(),
                            },
                        }
                    }
                    None => {
                        let rel = decoded.trim_start_matches('/').to_string();
                        // Le navigateur demande favicon.ico de lui-même : ce n'est pas un fichier de la page.
                        if !rel.is_empty() && rel != "favicon.ico" {
                            let mut m = missing2.lock().unwrap();
                            if !m.contains(&rel) {
                                m.push(rel);
                            }
                        }
                        Response::from_string("404").with_status_code(404).boxed()
                    }
                };
                let _ = req.respond(resp);
            }
        });
        Ok(Self { server, handle: Some(handle), missing, port })
    }

    /// Fichiers demandés par la page mais absents du dossier.
    pub fn missing_files(&self) -> Vec<String> {
        self.missing.lock().unwrap().clone()
    }

    /// URL servie pour un chemin relatif à la racine.
    pub fn url_for(&self, rel: &str) -> String {
        let rel = rel.replace('\\', "/");
        let enc: Vec<String> = rel
            .split('/')
            .map(|seg| utf8_percent_encode(seg, PATH_SET).to_string())
            .collect();
        format!("http://127.0.0.1:{}/{}", self.port, enc.join("/"))
    }
}

impl Drop for StaticServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Ajoute `<script src="…">` juste avant `</body>` (ou en fin de document).
fn with_script(html: &str, script: &str) -> String {
    let tag = format!("<script src=\"{}\"></script>\n", utf8_percent_encode(script, PATH_SET));
    match html.to_ascii_lowercase().rfind("</body>") {
        Some(i) => format!("{}{tag}{}", &html[..i], &html[i..]),
        None => format!("{html}\n{tag}"),
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("en-tête valide")
}

/// Résout un chemin d'URL dans la racine, sans permettre d'en sortir.
fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let mut p = root.to_path_buf();
    for comp in Path::new(url_path.trim_start_matches('/')).components() {
        match comp {
            Component::Normal(c) => p.push(c),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if p.is_dir() {
        p.push("index.html");
    }
    let p = p.canonicalize().ok()?;
    (p.starts_with(root) && p.is_file()).then_some(p)
}

fn mime_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "wasm" => "application/wasm",
        "glsl" | "txt" | "frag" | "vert" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
