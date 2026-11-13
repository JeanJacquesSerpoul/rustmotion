//! Orchestration : chargement de la page, analyse (probe) et rendu image par image.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use rustmotion_types::{DurationSource, FrameInfo, Framing, ProbeInfo, Progress, RenderSettings, Stage};
use serde_json::{json, Value};

use crate::browser::Browser;
use crate::cdp::{Cdp, Page};
use crate::encoder::{find_ffmpeg, Encoder};
use crate::server::StaticServer;
pub use crate::source::split_query;
use crate::source::locate;
use crate::tr;

const SHIM: &str = include_str!("shim.js");

/// Issue d'un rendu.
#[derive(Debug)]
pub enum Outcome {
    Done(PathBuf),
    Cancelled,
}

/// Convertit l'entrée utilisateur en URL (en démarrant un serveur local si besoin).
/// Le script compagnon d'une page (`page.html` → `page.js`) est ajouté s'il n'est pas déjà chargé.
pub fn resolve_input(input: &str) -> Result<(String, Option<StaticServer>)> {
    match locate(input)? {
        None => Ok((input.trim().trim_matches('"').trim().to_string(), None)),
        Some(p) => {
            let inject = p.inject_script.map(|script| (p.root.join(&p.file), script));
            let server = StaticServer::start(&p.root, inject)?;
            let url = format!("{}{}", server.url_for(&p.file), p.suffix);
            Ok((url, Some(server)))
        }
    }
}

/// Une page chargée dans un navigateur headless, horloge virtuelle prête.
/// L'ordre des champs fixe l'ordre de destruction : CDP, puis navigateur, puis serveur.
pub struct Session {
    pub cdp: Cdp,
    pub page: Page,
    _browser: Browser,
    server: Option<StaticServer>,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    /// Zone filmée (pixels CSS) quand la vidéo est cadrée sur un élément.
    pub clip: Option<Clip>,
}

/// Zone de la page filmée, en pixels CSS.
#[derive(Debug, Clone)]
pub struct Clip {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub label: String,
}

pub struct OpenOptions<'a> {
    pub input: &'a str,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub transparent: bool,
    pub wait_ms: u32,
    pub browser_path: Option<&'a str>,
    pub framing: &'a Framing,
}

impl Session {
    pub fn open(o: OpenOptions<'_>, mut stage: impl FnMut(Stage)) -> Result<Self> {
        if o.width == 0 || o.height == 0 {
            bail!(tr!("dimensions invalides", "invalid dimensions"));
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
            bail!(tr!(format!("impossible de charger {url} : {err}"), format!("cannot load {url}: {err}")));
        }
        cdp.wait_event(Some(&page.session), "Page.loadEventFired", Duration::from_secs(90))
            .context(tr!("chargement de la page", "loading the page"))?;
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
            bail!(tr!(
                "l'horloge virtuelle n'a pas pu être injectée dans la page",
                "the virtual clock could not be injected into the page"
            ));
        }
        page.eval(&mut cdp, "window.__rm.init(false)")?;
        // Une page qui pose `window.READY = false` signale qu'elle se prépare encore (30 s au plus).
        page.eval(
            &mut cdp,
            "(async () => { for (let i = 0; i < 300 && window.READY === false; i++) await window.__rm.sleep(100); return true; })()",
        )?;

        let mut sess =
            Self { cdp, page, _browser: browser, server, width: o.width, height: o.height, scale: o.scale, clip: None };
        sess.frame(o.framing)?;
        Ok(sess)
    }

    /// Cadre la vidéo sur un élément : la fenêtre est agrandie ou réduite jusqu'à ce que l'élément
    /// fasse exactement la largeur demandée (la page le redessine elle-même à cette taille, donc net),
    /// puis seule sa zone est capturée. Sans élément à cadrer, la page entière est filmée.
    fn frame(&mut self, framing: &Framing) -> Result<()> {
        let selector = match framing {
            Framing::Page => return Ok(()),
            Framing::Auto => None,
            Framing::Selector(sel) => Some(sel.as_str()),
        };
        let call = format!("window.__rm.frameRect({})", serde_json::to_string(&selector)?);
        let target = self.width as f64;
        let (mut vw, mut vh) = (self.width as f64, self.height as f64);
        let mut rect = Value::Null;
        for _ in 0..6 {
            rect = self.page.eval(&mut self.cdp, &call)?;
            if rect["missing"].as_bool() == Some(true) {
                let sel = selector.unwrap_or_default();
                bail!(tr!(format!("élément introuvable pour le cadrage : {sel}"), format!("no element matches the framing selector: {sel}")));
            }
            let Some(w) = rect["w"].as_f64() else { return Ok(()) };
            if (w - target).abs() < 0.5 {
                break;
            }
            // L'élément suit (à peu près) la taille de la fenêtre : on corrige proportionnellement.
            let k = target / w;
            (vw, vh) = ((vw * k).round().clamp(64.0, 8192.0), (vh * k).round().clamp(64.0, 8192.0));
            self.page.call(
                &mut self.cdp,
                "Emulation.setDeviceMetricsOverride",
                json!({ "width": vw, "height": vh, "deviceScaleFactor": self.scale, "mobile": false }),
            )?;
            // Laisse la page réagir au redimensionnement (événement resize, ResizeObserver, mise en page).
            self.page.eval(&mut self.cdp, "window.__rm.sleep(120)")?;
        }
        let num = |k: &str| rect[k].as_f64().unwrap_or(0.0);
        if num("w") < 2.0 || num("h") < 2.0 {
            return Ok(());
        }
        (self.width, self.height) = (vw as u32, vh as u32);
        self.clip = Some(Clip {
            x: num("x").round(),
            y: num("y").round(),
            w: num("w").round(),
            h: num("h").round(),
            label: rect["label"].as_str().unwrap_or("").to_string(),
        });
        Ok(())
    }

    fn clip_json(&self, scale: f64) -> Value {
        match &self.clip {
            Some(c) => json!({ "x": c.x, "y": c.y, "width": c.w, "height": c.h, "scale": scale }),
            None => json!({ "x": 0, "y": 0, "width": self.width, "height": self.height, "scale": scale }),
        }
    }

    /// Avance l'horloge virtuelle à `t_ms`.
    pub fn seek(&mut self, t_ms: f64) -> Result<()> {
        self.page.eval(&mut self.cdp, &format!("window.__rm.seek({t_ms:.4})"))?;
        Ok(())
    }

    /// Capture PNG pleine résolution (base64).
    pub fn capture_png(&mut self) -> Result<String> {
        let mut params = json!({ "format": "png", "fromSurface": true, "captureBeyondViewport": false, "optimizeForSpeed": true });
        if self.clip.is_some() {
            params["clip"] = self.clip_json(1.0);
        }
        self.page.screenshot_b64(&mut self.cdp, params)
    }

    /// Miniature JPEG (data URL) d'environ `max_w` pixels de large.
    pub fn thumbnail(&mut self, max_w: u32) -> Result<String> {
        let w = self.clip.as_ref().map(|c| c.w).unwrap_or(self.width as f64);
        let s = (max_w as f64 / (w * self.scale)).min(1.0);
        let params = json!({ "format": "jpeg", "quality": 70, "fromSurface": true, "clip": self.clip_json(s) });
        let b64 = self.page.screenshot_b64(&mut self.cdp, params)?;
        Ok(format!("data:image/jpeg;base64,{b64}"))
    }
}

/// Analyse une page : durée, nombre d'animations, GSAP, réglages déclarés, miniature.
pub fn probe(input: &str, width: u32, height: u32, browser_path: Option<&str>, framing: &Framing) -> Result<ProbeInfo> {
    let mut sess = Session::open(
        OpenOptions { input, width, height, scale: 1.0, transparent: false, wait_ms: 200, browser_path, framing },
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
        seek_api: raw["seekApi"].as_bool().unwrap_or(false),
        ..Default::default()
    };
    (info.duration, info.duration_source) = detect_duration(&raw).unzip();

    // Miniature au milieu de l'animation (ou à 1 s).
    let t = info.duration.map(|d| d * 500.0).unwrap_or(1000.0).min(10_000.0);
    sess.seek(t)?;
    // Beaucoup de lecteurs n'affichent leur durée (« 0:00 / 1:08 ») qu'après leur premier rafraîchissement :
    // sans durée trouvée au chargement, on relit la page une fois l'animation lancée.
    if info.duration.is_none() {
        if let Ok(again) = sess.page.eval(&mut sess.cdp, "window.__rm.probe()") {
            (info.duration, info.duration_source) = detect_duration(&again).unzip();
            if let Some(d) = info.duration {
                let mid = (d * 500.0).min(10_000.0);
                if mid > t {
                    sess.seek(mid)?;
                }
            }
        }
    }
    info.thumbnail = sess.thumbnail(480).ok();

    // Contrôles : la page anime-t-elle quelque chose (y compris ce qui démarre en cours de route),
    // et tous les fichiers qu'elle demande existent-ils ?
    let late = sess.page.eval(&mut sess.cdp, "window.__rm.isMotion()").ok();
    info.frame = sess.clip.as_ref().map(|c| FrameInfo { label: c.label.clone(), width: c.w, height: c.h });
    info.is_motion = raw["isMotion"].as_bool().unwrap_or(false) || late == Some(Value::Bool(true));
    info.missing_files = sess.server.as_ref().map(|s| s.missing_files()).unwrap_or_default();
    Ok(info)
}

/// Durée de l'animation, par ordre de fiabilité : réglage déclaré par la page, durée du script,
/// lecteur intégré (horloge / curseur), vidéo ou son, timeline GSAP, animations CSS finies.
fn detect_duration(raw: &Value) -> Option<(f64, DurationSource)> {
    let num = |v: &Value| v.as_f64().filter(|x| x.is_finite() && *x > 0.0);
    num(&raw["meta"]["duration"])
        .map(|d| (d, DurationSource::Page))
        .or_else(|| num(&raw["scriptDuration"]).map(|d| (d, DurationSource::Script)))
        .or_else(|| num(&raw["playerDuration"]).map(|d| (d, DurationSource::Player)))
        .or_else(|| num(&raw["mediaDuration"]).map(|d| ((d * 100.0).round() / 100.0, DurationSource::Media)))
        .or_else(|| num(&raw["gsapDuration"]).map(|d| (d, DurationSource::Gsap)))
        .or_else(|| num(&raw["maxEnd"]).map(|ms| ((ms / 1000.0 * 100.0).round() / 100.0, DurationSource::Animations)))
}

/// Rendu complet vers un fichier vidéo.
pub fn render(s: &RenderSettings, cancel: &AtomicBool, mut on_progress: impl FnMut(Progress)) -> Result<Outcome> {
    validate(s)?;
    let ffmpeg = find_ffmpeg(s.ffmpeg_path.as_deref())
        .ok_or_else(|| anyhow!(tr!("ffmpeg introuvable (installez-le ou indiquez son chemin)", "ffmpeg not found (install it or set its path)")))?;
    let output = PathBuf::from(s.output.trim().trim_matches('"'));
    if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| tr!(format!("création de {}", dir.display()), format!("creating {}", dir.display())))?;
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
            framing: &s.framing,
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
            .with_context(|| tr!(format!("image {i}"), format!("frame {i}")))?;
        let png = sess.capture_png().with_context(|| tr!(format!("capture de l'image {i}"), format!("capturing frame {i}")))?;
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
        bail!(tr!("aucun fichier de sortie indiqué", "no output file given"));
    }
    if s.width < 16 || s.height < 16 || s.width > 8192 || s.height > 8192 {
        bail!(tr!("dimensions hors limites (16 – 8192 px)", "dimensions out of range (16 – 8192 px)"));
    }
    if !(0.25..=4.0).contains(&s.scale) {
        bail!(tr!("facteur d'échelle hors limites (0.25 – 4)", "scale factor out of range (0.25 – 4)"));
    }
    let (w, h) = s.output_size();
    if w > 8192 || h > 8192 {
        bail!(tr!(format!("résolution de sortie trop grande ({w}×{h})"), format!("output resolution too large ({w}×{h})")));
    }
    if s.fps == 0 || s.fps > 240 {
        bail!(tr!("fps hors limites (1 – 240)", "fps out of range (1 – 240)"));
    }
    if !(s.duration > 0.0 && s.duration <= 3600.0) {
        bail!(tr!("durée hors limites (0 – 3600 s)", "duration out of range (0 – 3600 s)"));
    }
    if s.start < 0.0 {
        bail!(tr!("le décalage de départ doit être positif", "the start offset must be positive"));
    }
    if let Some(a) = s.audio.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        if !Path::new(a).is_file() {
            bail!(tr!(format!("fichier audio introuvable : {a}"), format!("audio file not found: {a}")));
        }
    }
    Ok(())
}
