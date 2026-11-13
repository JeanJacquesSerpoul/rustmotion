//! Bande son écrite en code : une fonction `compose(ctx, duration)` (API Web Audio) est exécutée
//! hors écran dans le navigateur headless, et le résultat est rendu en WAV.
//!
//! Le code vient d'un modèle d'IA : il tourne dans un navigateur isolé (profil temporaire, page
//! vierge), avec un délai maximal, et le processus est tué à la fin quoi qu'il arrive.

use anyhow::{anyhow, bail, Result};
use base64::Engine as _;
use serde_json::json;

use crate::browser::Browser;
use crate::cdp::{Cdp, Page};
use crate::tr;

/// Fréquence d'échantillonnage du rendu.
pub const SAMPLE_RATE: u32 = 44_100;
/// Durée maximale d'exécution du code (ms).
const EVAL_TIMEOUT_MS: u64 = 90_000;

/// Isole le code JavaScript d'une réponse de modèle (sans les balises ``` éventuelles).
pub fn extract_js(answer: &str) -> Option<String> {
    let s = answer.trim();
    let code = match s.find("```") {
        Some(start) => {
            let after = &s[start + 3..];
            // Saute l'étiquette de langage (« js », « javascript »…).
            let body = after.split_once('\n').map(|(_, b)| b).unwrap_or(after);
            body.split("```").next().unwrap_or(body)
        }
        None => s,
    };
    let code = code.trim();
    code.contains("compose").then(|| code.to_string())
}

/// Exécute `code` (qui définit `async function compose(ctx, duration)`) et renvoie un WAV
/// 16 bits stéréo de `duration` secondes, normalisé sans écrêtage.
pub fn render_code(code: &str, duration: f64, browser_path: Option<&str>) -> Result<Vec<u8>> {
    if !(duration > 0.0 && duration <= 600.0) {
        bail!(tr!("durée de bande son invalide", "invalid soundtrack duration"));
    }
    let browser = Browser::launch(browser_path)?;
    let mut cdp = Cdp::connect(&browser.ws_url)?;
    let page = Page::create(&mut cdp)?;
    let expr = harness(code, duration);
    let r = page.call(
        &mut cdp,
        "Runtime.evaluate",
        json!({ "expression": expr, "awaitPromise": true, "returnByValue": true, "timeout": EVAL_TIMEOUT_MS }),
    )?;
    if let Some(ex) = r.get("exceptionDetails") {
        let text = ex["exception"]["description"].as_str().or_else(|| ex["text"].as_str()).unwrap_or("JS exception");
        bail!(tr!(
            format!("le code de la bande son a échoué : {text}"),
            format!("the soundtrack code failed: {text}")
        ));
    }
    let b64 = r["result"]["value"].as_str().ok_or_else(|| anyhow!(tr!("aucun son produit", "no sound produced")))?;
    let wav = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| anyhow!(tr!(format!("son illisible : {e}"), format!("unreadable sound: {e}"))))?;
    drop(browser);
    Ok(wav)
}

/// Programme exécuté dans la page : rendu hors écran, normalisation, encodage WAV en base64.
fn harness(code: &str, duration: f64) -> String {
    format!(
        r#"(async () => {{
  const DURATION = {duration};
  const SR = {SAMPLE_RATE};
  const ctx = new OfflineAudioContext(2, Math.ceil(DURATION * SR), SR);
  // Code fourni par le modèle (dans sa propre portée).
  const compose = (() => {{
{code}
;
    return typeof compose === 'function' ? compose : null;
  }})();
  if (!compose) throw new Error('compose(ctx, duration) is missing');
  await compose(ctx, DURATION);
  const buf = await ctx.startRendering();

  // Normalisation : crête à -1 dBFS, sans amplifier un silence (gain plafonné à x4).
  const chans = [];
  for (let c = 0; c < 2; c++) chans.push(buf.numberOfChannels > c ? buf.getChannelData(c) : buf.getChannelData(0));
  let peak = 0;
  for (const ch of chans) for (let i = 0; i < ch.length; i++) {{ const a = Math.abs(ch[i]); if (a > peak && isFinite(a)) peak = a; }}
  if (peak < 1e-4) throw new Error('the soundtrack is silent');
  const gain = Math.min(4, 0.89 / peak);

  // WAV PCM 16 bits stéréo.
  const n = chans[0].length, bytes = 44 + n * 4;
  const out = new DataView(new ArrayBuffer(bytes));
  const str = (o, s) => {{ for (let i = 0; i < s.length; i++) out.setUint8(o + i, s.charCodeAt(i)); }};
  str(0, 'RIFF'); out.setUint32(4, bytes - 8, true); str(8, 'WAVE'); str(12, 'fmt ');
  out.setUint32(16, 16, true); out.setUint16(20, 1, true); out.setUint16(22, 2, true);
  out.setUint32(24, SR, true); out.setUint32(28, SR * 4, true); out.setUint16(32, 4, true); out.setUint16(34, 16, true);
  str(36, 'data'); out.setUint32(40, n * 4, true);
  let o = 44;
  for (let i = 0; i < n; i++) for (let c = 0; c < 2; c++) {{
    let v = chans[c][i] * gain; if (!isFinite(v)) v = 0;
    v = Math.max(-1, Math.min(1, v));
    out.setInt16(o, v < 0 ? v * 0x8000 : v * 0x7fff, true); o += 2;
  }}
  // Base64 par blocs (btoa sur une chaîne binaire).
  const u8 = new Uint8Array(out.buffer);
  let bin = '';
  for (let i = 0; i < u8.length; i += 0x8000) bin += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
  return btoa(bin);
}})()"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_code() {
        let a = "Voici :\n```javascript\nasync function compose(ctx, duration) {}\n```\nFin";
        assert_eq!(extract_js(a).unwrap(), "async function compose(ctx, duration) {}");
        assert_eq!(extract_js("async function compose(c, d) { }").unwrap(), "async function compose(c, d) { }");
        assert!(extract_js("Désolé, je ne peux pas.").is_none());
    }

    /// Rendu réel dans le navigateur (lancer avec `--ignored` : il faut Chrome ou Edge).
    #[test]
    #[ignore]
    fn renders_a_tone() {
        let code = "async function compose(ctx, duration) {\n  const o = ctx.createOscillator(); const g = ctx.createGain();\n  g.gain.value = 0.2; o.connect(g).connect(ctx.destination); o.start(0); o.stop(duration);\n}";
        let wav = render_code(code, 1.0, None).unwrap();
        assert!(wav.starts_with(b"RIFF") && &wav[8..12] == b"WAVE");
        assert_eq!(wav.len(), 44 + SAMPLE_RATE as usize * 4);
    }

    /// Composition plus riche, du type de celles qu'écrit un modèle (lancer avec `--ignored`).
    #[test]
    #[ignore]
    fn renders_a_rich_score() {
        let code = r#"async function compose(ctx, duration) {
  let seed = 7; const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  const master = ctx.createGain(); master.gain.value = 0.7;
  const comp = ctx.createDynamicsCompressor(); master.connect(comp).connect(ctx.destination);
  const ir = ctx.createBuffer(2, ctx.sampleRate * 2, ctx.sampleRate);
  for (let c = 0; c < 2; c++) { const d = ir.getChannelData(c); for (let i = 0; i < d.length; i++) d[i] = (rnd() * 2 - 1) * Math.pow(1 - i / d.length, 3); }
  const verb = ctx.createConvolver(); verb.buffer = ir; const wet = ctx.createGain(); wet.gain.value = 0.25; verb.connect(wet).connect(master);
  const noise = ctx.createBuffer(1, ctx.sampleRate, ctx.sampleRate); const nd = noise.getChannelData(0); for (let i = 0; i < nd.length; i++) nd[i] = rnd() * 2 - 1;
  const bpm = 110, beat = 60 / bpm;
  for (let t = 0; t < duration - 0.5; t += beat) {
    const k = ctx.createOscillator(), g = ctx.createGain(); k.frequency.setValueAtTime(140, t); k.frequency.exponentialRampToValueAtTime(40, t + 0.2);
    g.gain.setValueAtTime(0.9, t); g.gain.exponentialRampToValueAtTime(0.001, t + 0.3); k.connect(g).connect(master); k.start(t); k.stop(t + 0.32);
    const h = ctx.createBufferSource(), hg = ctx.createGain(), hf = ctx.createBiquadFilter(); h.buffer = noise; hf.type = 'highpass'; hf.frequency.value = 7000;
    hg.gain.setValueAtTime(0.15, t + beat / 2); hg.gain.exponentialRampToValueAtTime(0.001, t + beat / 2 + 0.05); h.connect(hf).connect(hg).connect(master); h.start(t + beat / 2, 0, 0.06);
  }
  const chords = [[220, 277.2, 329.6], [196, 246.9, 293.7], [174.6, 220, 261.6], [196, 246.9, 293.7]];
  for (let bar = 0, t = 0; t < duration - 1; bar++, t += beat * 4) for (const f of chords[bar % 4]) {
    const o = ctx.createOscillator(), g = ctx.createGain(); o.type = 'sawtooth'; o.frequency.value = f;
    const lp = ctx.createBiquadFilter(); lp.frequency.value = 1200;
    g.gain.setValueAtTime(0, t); g.gain.linearRampToValueAtTime(0.05, t + 0.1); g.gain.linearRampToValueAtTime(0, t + beat * 4);
    o.connect(lp).connect(g); g.connect(master); g.connect(verb); o.start(t); o.stop(t + beat * 4);
  }
}"#;
        let t0 = std::time::Instant::now();
        let wav = render_code(code, 20.0, None).unwrap();
        assert_eq!(wav.len(), 44 + SAMPLE_RATE as usize * 4 * 20);
        eprintln!("20 s rendues en {:?}", t0.elapsed());
    }
}
