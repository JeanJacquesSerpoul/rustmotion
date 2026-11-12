//! Encodage : un processus ffmpeg reçoit les PNG sur stdin (image2pipe).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use rustmotion_types::{Codec, RenderSettings};

use crate::util::{hide_console, which};

/// Cherche ffmpeg : chemin explicite, variable d'env, à côté de l'exécutable, PATH.
pub fn find_ffmpeg(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = explicit.filter(|s| !s.trim().is_empty()) {
        let p = PathBuf::from(p.trim());
        return p.is_file().then_some(p);
    }
    if let Ok(p) = std::env::var("RUSTMOTION_FFMPEG") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            for p in [dir.join(exe_name), dir.join("bin").join(exe_name)] {
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    if let Some(p) = which("ffmpeg") {
        return Some(p);
    }
    #[cfg(windows)]
    for p in [r"C:\ffmpeg\bin\ffmpeg.exe", r"C:\Program Files\ffmpeg\bin\ffmpeg.exe"] {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    #[cfg(not(windows))]
    for p in ["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/opt/homebrew/bin/ffmpeg"] {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn ffmpeg_version(ffmpeg: &Path) -> Option<String> {
    let mut cmd = Command::new(ffmpeg);
    cmd.arg("-version").stdout(Stdio::piped()).stderr(Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next()?;
    Some(line.trim_start_matches("ffmpeg version ").split(" Copyright").next()?.trim().to_string())
}

/// Construit la ligne de commande ffmpeg.
pub fn ffmpeg_args(s: &RenderSettings, output: &Path) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    let mut push = |xs: &[&str]| a.extend(xs.iter().map(|x| x.to_string()));
    let fps = s.fps.to_string();
    push(&["-hide_banner", "-loglevel", "error", "-y"]);
    push(&["-f", "image2pipe", "-framerate", &fps, "-c:v", "png", "-i", "-"]);

    let audio = s.audio.as_deref().map(str::trim).filter(|p| !p.is_empty());
    if let Some(audio) = audio {
        if s.start > 0.0 {
            push(&["-ss", &format!("{:.3}", s.start)]);
        }
        push(&["-i", audio]);
    }
    push(&["-map", "0:v"]);
    if audio.is_some() {
        push(&["-map", "1:a?"]);
    }

    let alpha = s.transparent && s.codec.supports_alpha();
    // Conversion RGB -> YUV explicitement en BT.709 (sinon ffmpeg utilise BT.601 : couleurs décalées).
    let (pix_fmt, even) = match s.codec {
        Codec::H264 | Codec::H265 => ("yuv420p", true),
        Codec::Vp9 => (if alpha { "yuva420p" } else { "yuv420p" }, true),
        Codec::ProRes4444 => (if alpha { "yuva444p10le" } else { "yuv444p10le" }, false),
    };
    let size = if even { "trunc(iw/2)*2:trunc(ih/2)*2" } else { "iw:ih" };
    let vf = format!("scale={size}:out_color_matrix=bt709:out_range=tv:flags=lanczos+accurate_rnd+full_chroma_int,format={pix_fmt}");
    push(&["-vf", &vf]);

    let crf = s.crf.min(51).to_string();
    match s.codec {
        Codec::H264 => push(&["-c:v", "libx264", "-preset", "medium", "-crf", &crf, "-movflags", "+faststart"]),
        Codec::H265 => push(&["-c:v", "libx265", "-preset", "medium", "-crf", &crf, "-tag:v", "hvc1", "-movflags", "+faststart", "-x265-params", "log-level=error"]),
        Codec::ProRes4444 => push(&["-c:v", "prores_ks", "-profile:v", "4444", "-vendor", "apl0"]),
        Codec::Vp9 => push(&["-c:v", "libvpx-vp9", "-crf", &crf, "-b:v", "0", "-row-mt", "1", "-deadline", "good", "-cpu-used", "2"]),
    }
    push(&["-pix_fmt", pix_fmt, "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"]);

    if audio.is_some() {
        match s.codec {
            Codec::Vp9 => push(&["-c:a", "libopus", "-b:a", "160k"]),
            _ => push(&["-c:a", "aac", "-b:a", "192k"]),
        }
        push(&["-shortest"]);
    }
    push(&["-r", &fps, "-t", &format!("{:.3}", s.duration)]);
    a.push(output.to_string_lossy().to_string());
    a
}

/// Processus ffmpeg alimenté par un thread d'écriture (décodage base64 + écriture stdin).
pub struct Encoder {
    child: Child,
    tx: Option<SyncSender<String>>,
    writer: Option<JoinHandle<std::io::Result<()>>>,
    log: Arc<Mutex<VecDeque<String>>>,
    log_thread: Option<JoinHandle<()>>,
}

impl Encoder {
    pub fn spawn(ffmpeg: &Path, s: &RenderSettings, output: &Path) -> Result<Self> {
        let mut cmd = Command::new(ffmpeg);
        cmd.args(ffmpeg_args(s, output))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        hide_console(&mut cmd);
        let mut child = cmd.spawn().with_context(|| format!("lancement de {}", ffmpeg.display()))?;

        let log = Arc::new(Mutex::new(VecDeque::new()));
        let stderr = child.stderr.take().unwrap();
        let log2 = log.clone();
        let log_thread = std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let mut l = log2.lock().unwrap();
                if l.len() >= 40 {
                    l.pop_front();
                }
                l.push_back(line);
            }
        });

        let mut stdin: ChildStdin = child.stdin.take().unwrap();
        let (tx, rx) = sync_channel::<String>(12);
        let writer = std::thread::spawn(move || -> std::io::Result<()> {
            let b64 = base64::engine::general_purpose::STANDARD;
            for frame in rx {
                let bytes = b64
                    .decode(frame.as_bytes())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                stdin.write_all(&bytes)?;
            }
            stdin.flush()?;
            Ok(()) // stdin est fermé ici -> ffmpeg termine.
        });

        Ok(Self { child, tx: Some(tx), writer: Some(writer), log, log_thread: Some(log_thread) })
    }

    /// Envoie une image PNG encodée en base64.
    pub fn push_b64(&mut self, png_b64: String) -> Result<()> {
        let tx = self.tx.as_ref().ok_or_else(|| anyhow!("encodeur fermé"))?;
        if tx.send(png_b64).is_err() {
            // Le thread d'écriture est mort : ffmpeg a probablement échoué.
            let err = self.finish_inner().err();
            bail!("{}", err.map(|e| e.to_string()).unwrap_or_else(|| "ffmpeg s'est arrêté".into()));
        }
        Ok(())
    }

    fn log_text(&self) -> String {
        self.log.lock().unwrap().iter().cloned().collect::<Vec<_>>().join("\n")
    }

    fn finish_inner(&mut self) -> Result<()> {
        drop(self.tx.take());
        let write_res = self.writer.take().map(|h| h.join());
        let status = self.child.wait()?;
        if let Some(h) = self.log_thread.take() {
            let _ = h.join();
        }
        if !status.success() {
            let log = self.log_text();
            bail!("ffmpeg a échoué ({status}) :\n{}", if log.is_empty() { "(aucun message)".into() } else { log });
        }
        if let Some(Ok(Err(e))) = write_res {
            bail!("écriture vers ffmpeg : {e}");
        }
        Ok(())
    }

    /// Ferme stdin et attend la fin de l'encodage.
    pub fn finish(mut self) -> Result<()> {
        self.finish_inner()
    }

    /// Interrompt l'encodage.
    pub fn abort(mut self) {
        drop(self.tx.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(h) = self.writer.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        if self.tx.is_some() {
            drop(self.tx.take());
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
