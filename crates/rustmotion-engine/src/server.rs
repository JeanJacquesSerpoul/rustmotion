//! Mini serveur HTTP statique local : permet aux pages d'utiliser les modules ES,
//! `fetch()` (Lottie JSON…), les polices, etc., ce que `file://` interdit.

use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::{anyhow, Result};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use tiny_http::{Header, Response, Server};

pub struct StaticServer {
    server: Arc<Server>,
    handle: Option<JoinHandle<()>>,
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
    pub fn start(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let server = Arc::new(Server::http("127.0.0.1:0").map_err(|e| anyhow!("serveur local : {e}"))?);
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| anyhow!("adresse du serveur local inconnue"))?;
        let srv = server.clone();
        let handle = std::thread::spawn(move || {
            for req in srv.incoming_requests() {
                let url = req.url().split(['?', '#']).next().unwrap_or("/").to_string();
                let decoded = percent_decode_str(&url).decode_utf8_lossy().to_string();
                let resp = match resolve(&root, &decoded) {
                    Some(path) => match File::open(&path) {
                        Ok(f) => {
                            let mime = mime_for(&path);
                            Response::from_file(f)
                                .with_header(header("Content-Type", mime))
                                .with_header(header("Cache-Control", "no-store"))
                                .with_header(header("Access-Control-Allow-Origin", "*"))
                                .boxed()
                        }
                        Err(_) => Response::from_string("403").with_status_code(403).boxed(),
                    },
                    None => Response::from_string("404").with_status_code(404).boxed(),
                };
                let _ = req.respond(resp);
            }
        });
        Ok(Self { server, handle: Some(handle), port })
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
