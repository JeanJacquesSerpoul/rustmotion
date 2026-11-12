//! CLI : `rustmotion-cli <entrée.html|dossier|url> [options]`

use std::io::Write;
use std::sync::atomic::AtomicBool;

use anyhow::{anyhow, bail, Result};
use rustmotion_engine::types::{Codec, RenderSettings, Stage};
use rustmotion_engine::{probe, render, tool_status, Outcome};

const HELP: &str = "\
RustMotion — convertit une animation HTML en vidéo

USAGE :
    rustmotion-cli <entrée> [options]
    rustmotion-cli --probe <entrée>
    rustmotion-cli --tools

ENTRÉE : fichier .html, dossier contenant index.html, ou URL http(s)

OPTIONS :
    -o, --output <fichier>   Fichier de sortie (défaut : <entrée>.mp4)
    -w, --width <px>         Largeur du viewport (défaut 1920)
    -h, --height <px>        Hauteur du viewport (défaut 1080)
    -s, --scale <x>          Facteur de rendu / devicePixelRatio (défaut 1)
    -r, --fps <n>            Images par seconde (défaut 30)
    -d, --duration <s>       Durée en secondes (défaut : détectée, sinon 5)
        --start <s>          Décalage de départ (défaut 0)
    -c, --codec <c>          h264 | h265 | prores4444 | vp9 (défaut h264)
        --crf <n>            Qualité 0-51 (défaut 18)
        --transparent        Fond transparent (prores4444 / vp9)
        --audio <fichier>    Piste audio à ajouter
        --wait <ms>          Attente après chargement (défaut 250)
        --browser <chemin>   Navigateur Chromium à utiliser
        --ffmpeg <chemin>    ffmpeg à utiliser
        --probe              Analyse la page sans rendre
        --tools              Affiche les outils détectés
        --help               Affiche cette aide
";

fn main() {
    if let Err(e) = run() {
        eprintln!("\nerreur : {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut s = RenderSettings::default();
    let mut duration_set = false;
    let mut probe_only = false;
    let mut input: Option<String> = None;

    fn val(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
        args.next().ok_or_else(|| anyhow!("valeur manquante pour {flag}"))
    }
    fn num<T: std::str::FromStr>(v: String, flag: &str) -> Result<T> {
        v.parse().map_err(|_| anyhow!("valeur invalide pour {flag} : {v}"))
    }

    while let Some(a) = args.next() {
        match a.as_str() {
            "--help" | "-?" => {
                print!("{HELP}");
                return Ok(());
            }
            "--tools" => {
                let t = tool_status(None, None);
                println!("navigateur : {}", t.browser.as_deref().unwrap_or("INTROUVABLE"));
                println!("ffmpeg     : {}", t.ffmpeg.as_deref().unwrap_or("INTROUVABLE"));
                if let Some(v) = t.ffmpeg_version {
                    println!("version    : {v}");
                }
                return Ok(());
            }
            "--probe" => probe_only = true,
            "-o" | "--output" => s.output = val(&mut args, &a)?,
            "-w" | "--width" => s.width = num(val(&mut args, &a)?, &a)?,
            "-h" | "--height" => s.height = num(val(&mut args, &a)?, &a)?,
            "-s" | "--scale" => s.scale = num(val(&mut args, &a)?, &a)?,
            "-r" | "--fps" => s.fps = num(val(&mut args, &a)?, &a)?,
            "-d" | "--duration" => {
                s.duration = num(val(&mut args, &a)?, &a)?;
                duration_set = true;
            }
            "--start" => s.start = num(val(&mut args, &a)?, &a)?,
            "-c" | "--codec" => {
                let v = val(&mut args, &a)?;
                s.codec = Codec::from_id(&v).ok_or_else(|| anyhow!("codec inconnu : {v}"))?;
            }
            "--crf" => s.crf = num(val(&mut args, &a)?, &a)?,
            "--transparent" => s.transparent = true,
            "--audio" => s.audio = Some(val(&mut args, &a)?),
            "--wait" => s.wait_ms = num(val(&mut args, &a)?, &a)?,
            "--browser" => s.browser_path = Some(val(&mut args, &a)?),
            "--ffmpeg" => s.ffmpeg_path = Some(val(&mut args, &a)?),
            x if x.starts_with('-') => bail!("option inconnue : {x} (voir --help)"),
            _ => {
                if input.replace(a).is_some() {
                    bail!("une seule entrée attendue");
                }
            }
        }
    }
    let Some(input) = input else {
        print!("{HELP}");
        return Ok(());
    };
    s.input = input.clone();

    if probe_only || !duration_set {
        eprint!("analyse de la page… ");
        let info = probe(&input, s.width, s.height, s.browser_path.as_deref())?;
        eprintln!("ok");
        if probe_only {
            let mut v = serde_json::to_value(&info)?;
            if let Some(t) = v.get_mut("thumbnail") {
                *t = serde_json::Value::from(info.thumbnail.as_ref().map(|t| format!("<{} octets>", t.len())));
            }
            println!("{}", serde_json::to_string_pretty(&v)?);
            return Ok(());
        }
        match (info.duration, info.duration_source) {
            (Some(d), Some(src)) => {
                eprintln!("durée détectée : {d:.2} s ({src})");
                s.duration = d;
            }
            _ => eprintln!("durée non détectée{} : {} s par défaut", if info.infinite { " (animation infinie)" } else { "" }, s.duration),
        }
        if let Some(f) = info.fps {
            s.fps = f;
        }
    }

    if s.output.is_empty() {
        let base = input.trim_end_matches(['/', '\\']);
        let stem = std::path::Path::new(base).with_extension(s.codec.extension());
        s.output = if base.starts_with("http") { format!("rustmotion.{}", s.codec.extension()) } else { stem.display().to_string() };
    }

    let (w, h) = s.output_size();
    eprintln!("rendu {w}×{h} @ {} fps, {:.2} s ({} images) -> {}", s.fps, s.duration, s.total_frames(), s.output);
    let cancel = AtomicBool::new(false);
    let mut last_stage = None;
    let out = render(&s, &cancel, |p| {
        if p.stage == Stage::Capturing {
            let pct = p.frame as f64 / p.total as f64 * 100.0;
            let eta = p.eta_ms.map(|e| format!("{:.0} s", e as f64 / 1000.0)).unwrap_or_default();
            eprint!("\r  {:>5.1} %  image {}/{}  {:.1} img/s  reste {eta}     ", pct, p.frame, p.total, p.render_fps);
            let _ = std::io::stderr().flush();
        } else if last_stage != Some(p.stage) {
            eprintln!("\n{}", p.stage.label());
        }
        last_stage = Some(p.stage);
    })?;
    match out {
        Outcome::Done(path) => {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            eprintln!("✔ {} ({:.2} Mo)", path.display(), size as f64 / 1_048_576.0);
        }
        Outcome::Cancelled => eprintln!("annulé"),
    }
    Ok(())
}
