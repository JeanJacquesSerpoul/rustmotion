//! CLI : `rustmotion-cli <entrée.html|dossier|url> [options]`

use std::io::Write;
use std::sync::atomic::AtomicBool;

use anyhow::{anyhow, bail, Result};
use rustmotion_engine::types::{Codec, Lang, RenderSettings, Stage};
use rustmotion_engine::{lang, probe, render, set_lang, tool_status, tr, Outcome};

const HELP_FR: &str = "\
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
        --lang <fr|en>       Langue des messages (défaut : langue du système)
        --probe              Analyse la page sans rendre
        --tools              Affiche les outils détectés
        --help               Affiche cette aide
";

const HELP_EN: &str = "\
RustMotion — turns an HTML animation into a video

USAGE:
    rustmotion-cli <input> [options]
    rustmotion-cli --probe <input>
    rustmotion-cli --tools

INPUT: an .html file, a folder containing index.html, or an http(s) URL

OPTIONS:
    -o, --output <file>      Output file (default: <input>.mp4)
    -w, --width <px>         Viewport width (default 1920)
    -h, --height <px>        Viewport height (default 1080)
    -s, --scale <x>          Render scale / devicePixelRatio (default 1)
    -r, --fps <n>            Frames per second (default 30)
    -d, --duration <s>       Duration in seconds (default: detected, else 5)
        --start <s>          Start offset (default 0)
    -c, --codec <c>          h264 | h265 | prores4444 | vp9 (default h264)
        --crf <n>            Quality 0-51 (default 18)
        --transparent        Transparent background (prores4444 / vp9)
        --audio <file>       Audio track to add
        --wait <ms>          Wait after page load (default 250)
        --browser <path>     Chromium browser to use
        --ffmpeg <path>      ffmpeg binary to use
        --lang <fr|en>       Message language (default: system language)
        --probe              Analyse the page without rendering
        --tools              Show detected tools
        --help               Show this help
";

fn help() -> &'static str {
    tr!(HELP_FR, HELP_EN)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("\n{} {e:#}", tr!("erreur :", "error:"));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // La langue est fixée avant tout message.
    if let Some(i) = raw.iter().position(|a| a == "--lang") {
        let code = raw.get(i + 1).ok_or_else(|| anyhow!("--lang : fr | en"))?;
        set_lang(Lang::from_code(code));
    } else {
        let _ = lang();
    }

    let mut args = raw.into_iter();
    let mut s = RenderSettings::default();
    let mut duration_set = false;
    let mut probe_only = false;
    let mut input: Option<String> = None;

    fn val(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
        args.next()
            .ok_or_else(|| anyhow!(tr!(format!("valeur manquante pour {flag}"), format!("missing value for {flag}"))))
    }
    fn num<T: std::str::FromStr>(v: String, flag: &str) -> Result<T> {
        v.parse()
            .map_err(|_| anyhow!(tr!(format!("valeur invalide pour {flag} : {v}"), format!("invalid value for {flag}: {v}"))))
    }

    while let Some(a) = args.next() {
        match a.as_str() {
            "--help" | "-?" => {
                print!("{}", help());
                return Ok(());
            }
            "--lang" => {
                args.next();
            }
            "--tools" => {
                let t = tool_status(None, None);
                let missing = tr!("INTROUVABLE", "NOT FOUND");
                println!("{:<11}: {}", tr!("navigateur", "browser"), t.browser.as_deref().unwrap_or(missing));
                println!("{:<11}: {}", "ffmpeg", t.ffmpeg.as_deref().unwrap_or(missing));
                if let Some(v) = t.ffmpeg_version {
                    println!("{:<11}: {v}", "version");
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
                s.codec = Codec::from_id(&v)
                    .ok_or_else(|| anyhow!(tr!(format!("codec inconnu : {v}"), format!("unknown codec: {v}"))))?;
            }
            "--crf" => s.crf = num(val(&mut args, &a)?, &a)?,
            "--transparent" => s.transparent = true,
            "--audio" => s.audio = Some(val(&mut args, &a)?),
            "--wait" => s.wait_ms = num(val(&mut args, &a)?, &a)?,
            "--browser" => s.browser_path = Some(val(&mut args, &a)?),
            "--ffmpeg" => s.ffmpeg_path = Some(val(&mut args, &a)?),
            x if x.starts_with('-') => {
                bail!(tr!(format!("option inconnue : {x} (voir --help)"), format!("unknown option: {x} (see --help)")))
            }
            _ => {
                if input.replace(a).is_some() {
                    bail!(tr!("une seule entrée attendue", "only one input expected"));
                }
            }
        }
    }
    let Some(input) = input else {
        print!("{}", help());
        return Ok(());
    };
    s.input = input.clone();
    let l = lang();

    if probe_only || !duration_set {
        eprint!("{}", tr!("analyse de la page… ", "analysing page… "));
        let info = probe(&input, s.width, s.height, s.browser_path.as_deref())?;
        eprintln!("ok");
        if probe_only {
            let mut v = serde_json::to_value(&info)?;
            if let Some(t) = v.get_mut("thumbnail") {
                *t = serde_json::Value::from(info.thumbnail.as_ref().map(|t| format!("<{} bytes>", t.len())));
            }
            println!("{}", serde_json::to_string_pretty(&v)?);
            return Ok(());
        }
        match (info.duration, info.duration_source) {
            (Some(d), Some(src)) => {
                eprintln!("{} : {d:.2} s ({})", tr!("durée détectée", "detected duration"), src.label(l));
                s.duration = d;
            }
            _ => {
                let inf = if info.infinite { tr!(" (animation infinie)", " (infinite animation)") } else { "" };
                eprintln!(
                    "{}{inf} : {} s {}",
                    tr!("durée non détectée", "duration not detected"),
                    s.duration,
                    tr!("par défaut", "by default")
                );
            }
        }
        if let Some(f) = info.fps {
            s.fps = f;
        }
    }

    if s.output.is_empty() {
        let (path, _) = rustmotion_engine::render::split_query(&input);
        let base = path.trim_end_matches(['/', '\\']);
        let stem = std::path::Path::new(base).with_extension(s.codec.extension());
        s.output = if base.starts_with("http") {
            format!("rustmotion.{}", s.codec.extension())
        } else {
            stem.display().to_string()
        };
    }

    let (w, h) = s.output_size();
    eprintln!(
        "{} {w}×{h} @ {} fps, {:.2} s ({} {}) -> {}",
        tr!("rendu", "rendering"),
        s.fps,
        s.duration,
        s.total_frames(),
        tr!("images", "frames"),
        s.output
    );
    let cancel = AtomicBool::new(false);
    let mut last_stage = None;
    let (frame_w, left_w, speed_w) = (tr!("image", "frame"), tr!("reste", "left"), tr!("img/s", "fps"));
    let out = render(&s, &cancel, |p| {
        if p.stage == Stage::Capturing {
            let pct = p.frame as f64 / p.total as f64 * 100.0;
            let eta = p.eta_ms.map(|e| format!("{:.0} s", e as f64 / 1000.0)).unwrap_or_default();
            eprint!(
                "\r  {:>5.1} %  {frame_w} {}/{}  {:.1} {speed_w}  {left_w} {eta}     ",
                pct, p.frame, p.total, p.render_fps
            );
            let _ = std::io::stderr().flush();
        } else if last_stage != Some(p.stage) {
            eprintln!("\n{}", p.stage.label(l));
        }
        last_stage = Some(p.stage);
    })?;
    match out {
        Outcome::Done(path) => {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            eprintln!("✔ {} ({:.2} {})", path.display(), size as f64 / 1_048_576.0, tr!("Mo", "MB"));
        }
        Outcome::Cancelled => eprintln!("{}", Stage::Cancelled.label(l)),
    }
    Ok(())
}
