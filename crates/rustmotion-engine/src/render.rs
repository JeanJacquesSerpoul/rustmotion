//! Orchestration : chargement de la page, analyse (probe) et rendu image par image.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use rustmotion_types::{ProbeInfo, Progress, RenderSettings, Stage};
use serde_json::{json, Value};

use crate::browser::Browser;
use crate::cdp::{Cdp, Page};
use crate::encoder::{find_ffmpeg, Encoder};
use crate::server::StaticServer;

const SHIM: &str = include_str!("shim.js");

/// Issue d'un rendu.
#[derive(Debug)]
pub enum Outcome {
    Done(PathBuf),
    Cancelled,
}

/// Convertit l'entrée utilisateur en URL (en démarrant un serveur local si besoin).
pub fn resolve_input(input: &str) -> Result<(String, Option<StaticServer>)> {
    let input = input.trim().trim_matches('"').trim();
    if input.is_empty() {
        bail!("aucune source indiquée");
    }
    let lower = input.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok((input.to_string(), None));
    }
    let path_str = if lower.starts_with("file:///") {
        let rest = &input[8..];
        let decoded = percent_encoding::percent_decode_str(rest).decode_utf8_lossy().to_string();
        if cfg!(windows) { decoded } else { format!("/{decoded}") }
    } else {
        input.to_string()
    };
    let path = PathBuf::from(&path_str);
    let path = path
        .canonicalize()
        .with_context(|| format!("fichier introuvable : {path_str}"))?;
    let (root, file) = if path.is_dir() {
        if !path.join("index.html").is_file() {
            bail!("le dossier ne contient pas d'index.html : {}", path.display());
        }
        (path.clone(), "index.html".to_string())
    } else {
        let root = path.parent().ok_or_else(|| anyhow!("chemin invalide"))?.to_path_buf();
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        (root, file)
    };
    let server = StaticServer::start(&root)?;
    let url = server.url_for(&file);
    Ok((url, Some(server)))
}

/// Une page chargée dans un navigateur headless, horloge virtuelle prête.
/// L'ordre des champs fixe l'ordre de destruction : CDP, puis navigateur, puis serveur.
pub struct Session {
    pub cdp: Cdp,
    pub page: Page,
    _browser: Browser,
    _server: Option<StaticServer>,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

pub struct OpenOptions<'a> {
    pub input: &'a str,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub transparent: bool,
    pub wait_ms: u32,
    pub browser_path: Option<&'a str>,
}

impl Session {
    pub fn open(o: OpenOptions<'_>, mut stage: impl FnMut(Stage)) -> Result<Self> {
        if o.width == 0 || o.height == 0 {
            bail!("dimensions invalides");
        }
        let (url, server) = resolve_input(o.input)?;
        stage(Stage::Launching);
        let browser = Browser::launch(o.browser_path)?;
        let mut cdp = Cdp::connect(&browser.ws_url)?;
        let page = Page::create(&mut cdp)?;

        stage(Stage::Loading);
        page.call(&mut cdp, "Page.enable", json!({}))?;
        page.call(&mut cdp, "Runtime.enable", json!({}))?;
        page.call(
            &mut cdp,
            "Emulation.setDeviceMetricsOverride",
            json!({ "width": o.width, "height": o.height, "deviceScaleFactor": o.scale, "mobile": false }),
        )?;
        if o.transparent {
            page.call(
                &mut cdp,
                "Emulation.setDefaultBackgroundColorOverride",
                json!({ "color": { "r": 0, "g": 0, "b": 0, "a": 0 } }),
            )?;
        }
        page.call(&mut cdp, "Page.addScriptToEvaluateOnNewDocument", json!({ "source": SHIM }))?;
        // Gèle la timeline des animations CSS/WAAPI : seul __rm.seek() les fera avancer.
        let _ = page.call(&mut cdp, "Animation.enable", json!({}));
        let _ = page.call(&mut cdp, "Animation.setPlaybackRate", json!({ "playbackRate": 0 }));

        cdp.clear_events();
        let nav = page.call(&mut cdp, "Page.navigate", json!({ "url": url }))?;
        if let Some(err) = nav.get("errorText").and_then(Value::as_str) {
            bail!("impossible de charger {url} : {err}");
        }
        cdp.wait_event(Some(&page.session), "Page.loadEventFired", Duration::from_secs(90))
            .context("chargement de la page")?;
        let _ = page.call(&mut cdp, "Animation.setPlaybackRate", json!({ "playbackRate": 0 }));

        // Polices et images prêtes.
        // (setTimeout est virtualisé : on utilise __rm.sleep pour un vrai délai.)
        page.eval(
            &mut cdp,
            r#"(async () => {
                const ready = (async () => {
                    if (document.fonts) { try { await document.fonts.ready; } catch (e) {} }
                    const imgs = [...document.images].filter(i => !i.complete);
                    await Promise.all(imgs.map(i => new Promise(r => {
                        i.addEventListener('load', r, { once: true });
                        i.addEventListener('error', r, { once: true });
                    })));
                })();
                await Promise.race([ready, window.__rm ? window.__rm.sleep(15000) : null]);
                return true;
            })()"#,
        )
        .ok();
        if o.wait_ms > 0 {
            std::thread::sleep(Duration::from_millis(o.wait_ms as u64));
        }
        let ok = page.eval(&mut cdp, "typeof window.__rm === 'object'")?;
        if ok != Value::Bool(true) {
            bail!("l'horloge virtuelle n'a pas pu être injectée dans la page");
        }
        page.eval(&mut cdp, "window.__rm.init(false)")?;

        Ok(Self { cdp, page, _browser: browser, _server: server, width: o.width, height: o.height, scale: o.scale })
    }

    /// Avance l'horloge virtuelle à `t_ms`.
    pub fn seek(&mut self, t_ms: f64) -> Result<()> {
        self.page.eval(&mut self.cdp, &format!("window.__rm.seek({t_ms:.4})"))?;
        Ok(())
    }

    /// Capture PNG pleine résolution (base64).
    pub fn capture_png(&mut self) -> Result<String> {
        self.page.screenshot_b64(
            &mut self.cdp,
            json!({ "format": "png", "fromSurface": true, "captureBeyondViewport": false, "optimizeForSpeed": true }),
        )
    }

    /// Miniature JPEG (data URL) d'environ `max_w` pixels de large.
    pub fn thumbnail(&mut self, max_w: u32) -> Result<String> {
        let s = (max_w as f64 / (self.width as f64 * self.scale)).min(1.0);
        let b64 = self.page.screenshot_b64(
            &mut self.cdp,
            json!({
                "format": "jpeg", "quality": 70, "fromSurface": true,
                "clip": { "x": 0, "y": 0, "width": self.width, "height": self.height, "scale": s }
            }),
        )?;
        Ok(format!("data:image/jpeg;base64,{b64}"))
    }
}

/// Analyse une page : durée, nombre d'animations, GSAP, réglages déclarés, miniature.
pub fn probe(input: &str, width: u32, height: u32, browser_path: Option<&str>) -> Result<ProbeInfo> {
    let mut sess = Session::open(
        OpenOptions { input, width, height, scale: 1.0, transparent: false, wait_ms: 200, browser_path },
        |_| {},
    )?;
    let raw = sess.page.eval(&mut sess.cdp, "window.__rm.probe()")?;
    let num = |v: &Value| v.as_f64().filter(|x| x.is_finite() && *x > 0.0);
    let meta = &raw["meta"];

    let mut info = ProbeInfo {
        title: raw["title"].as_str().unwrap_or("").to_string(),
        animations: raw["animations"].as_u64().unwrap_or(0) as u32,
        infinite: raw["infinite"].as_bool().unwrap_or(false),
        gsap: raw["gsap"].as_bool().unwrap_or(false),
        width: num(&meta["width"]).map(|x| x as u32),
        height: num(&meta["height"]).map(|x| x as u32),
        fps: num(&meta["fps"]).map(|x| x as u32),
        ..Default::default()
    };
    if let Some(d) = num(&meta["duration"]) {
        info.duration = Some(d);
        info.duration_source = Some("déclarée par la page".into());
    } else if let Some(d) = num(&raw["gsapDuration"]) {
        info.duration = Some(d);
        info.duration_source = Some("timeline GSAP".into());
    } else if let Some(ms) = num(&raw["maxEnd"]) {
        info.duration = Some((ms / 1000.0 * 100.0).round() / 100.0);
        info.duration_source = Some("animations CSS / Web Animations".into());
    }

    // Miniature au milieu de l'animation (ou à 1 s).
    let t = info.duration.map(|d| d * 500.0).unwrap_or(1000.0).min(10_000.0);
    sess.seek(t)?;
    info.thumbnail = sess.thumbnail(480).ok();
    Ok(info)
}

/// Rendu complet vers un fichier vidéo.
pub fn render(s: &RenderSettings, cancel: &AtomicBool, mut on_progress: impl FnMut(Progress)) -> Result<Outcome> {
    validate(s)?;
    let ffmpeg = find_ffmpeg(s.ffmpeg_path.as_deref())
        .ok_or_else(|| anyhow!("ffmpeg introuvable (installez-le ou indiquez son chemin)"))?;
    let output = PathBuf::from(s.output.trim().trim_matches('"'));
    if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
    }

    let total = s.total_frames();
    let started = Instant::now();
    let emit = |stage: Stage, frame: u32, preview: Option<String>, f: &mut dyn FnMut(Progress)| {
        let elapsed = started.elapsed();
        let (rfps, eta) = if stage == Stage::Capturing && frame > 0 {
            let rfps = frame as f64 / elapsed.as_secs_f64().max(1e-6);
            (rfps, Some(((total - frame) as f64 / rfps * 1000.0) as u64))
        } else {
            (0.0, None)
        };
        f(Progress { stage, frame, total, elapsed_ms: elapsed.as_millis() as u64, eta_ms: eta, render_fps: rfps, preview });
    };

    let mut sess = Session::open(
        OpenOptions {
            input: &s.input,
            width: s.width,
            height: s.height,
            scale: s.scale,
            transparent: s.transparent && s.codec.supports_alpha(),
            wait_ms: s.wait_ms,
            browser_path: s.browser_path.as_deref(),
        },
        |st| emit(st, 0, None, &mut on_progress),
    )?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(Outcome::Cancelled);
    }

    let mut enc = Encoder::spawn(&ffmpeg, s, &output)?;
    let capture_started = Instant::now();
    let preview_every = (s.fps / 2).max(1);
    let start_ms = s.start * 1000.0;
    let frame_ms = 1000.0 / s.fps as f64;

    for i in 0..total {
        if cancel.load(Ordering::Relaxed) {
            enc.abort();
            let _ = std::fs::remove_file(&output);
            emit(Stage::Cancelled, i, None, &mut on_progress);
            return Ok(Outcome::Cancelled);
        }
        sess.seek(start_ms + i as f64 * frame_ms)
            .with_context(|| format!("image {i}"))?;
        let png = sess.capture_png().with_context(|| format!("capture de l'image {i}"))?;
        if let Err(e) = enc.push_b64(png) {
            let _ = std::fs::remove_file(&output);
            return Err(e);
        }
        let done = i + 1;
        let preview = if i % preview_every == 0 || done == total { sess.thumbnail(360).ok() } else { None };
        // Vitesse et ETA calculées sur la phase de capture uniquement.
        let el = capture_started.elapsed().as_secs_f64().max(1e-6);
        let rfps = done as f64 / el;
        on_progress(Progress {
            stage: Stage::Capturing,
            frame: done,
            total,
            elapsed_ms: started.elapsed().as_millis() as u64,
            eta_ms: Some(((total - done) as f64 / rfps * 1000.0) as u64),
            render_fps: rfps,
            preview,
        });
    }
    drop(sess);

    emit(Stage::Finalizing, total, None, &mut on_progress);
    if let Err(e) = enc.finish() {
        let _ = std::fs::remove_file(&output);
        return Err(e);
    }
    emit(Stage::Done, total, None, &mut on_progress);
    Ok(Outcome::Done(output))
}

fn validate(s: &RenderSettings) -> Result<()> {
    if s.output.trim().is_empty() {
        bail!("aucun fichier de sortie indiqué");
    }
    if s.width < 16 || s.height < 16 || s.width > 8192 || s.height > 8192 {
        bail!("dimensions hors limites (16 – 8192 px)");
    }
    if !(0.25..=4.0).contains(&s.scale) {
        bail!("facteur d'échelle hors limites (0.25 – 4)");
    }
    let (w, h) = s.output_size();
    if w > 8192 || h > 8192 {
        bail!("résolution de sortie trop grande ({w}×{h})");
    }
    if s.fps == 0 || s.fps > 240 {
        bail!("fps hors limites (1 – 240)");
    }
    if !(s.duration > 0.0 && s.duration <= 3600.0) {
        bail!("durée hors limites (0 – 3600 s)");
    }
    if s.start < 0.0 {
        bail!("le décalage de départ doit être positif");
    }
    if let Some(a) = s.audio.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        if !Path::new(a).is_file() {
            bail!("fichier audio introuvable : {a}");
        }
    }
    Ok(())
}
