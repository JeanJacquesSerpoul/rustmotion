# RustMotion

Convertisseur d'animations **HTML / CSS / JS → vidéo** (MP4, MOV, WebM), en Rust + Tauri v2 + Leptos.

Chaque image est calculée à un instant précis grâce à une **horloge virtuelle** : le résultat est fluide et
identique d'un rendu à l'autre, même si la page est lourde ou la machine lente.

## Fonctionnalités

- Sources : fichier `.html`, dossier contenant `index.html`, ou URL `http(s)`.
  Les fichiers locaux sont servis par un petit serveur HTTP intégré : modules ES, `fetch()`, polices, JSON Lottie… fonctionnent.
- Moteurs d'animation pris en charge : animations et transitions CSS, Web Animations API (y compris `await anim.finished`),
  `requestAnimationFrame` (canvas, WebGL, three.js, Lottie…), `setTimeout` / `setInterval`, `Date` / `performance.now`,
  GSAP, anime.js, `<video>` (recalée à chaque image).
- Détection automatique de la durée (meta de la page, timeline GSAP, ou animations CSS/WAAPI finies) et miniature d'aperçu.
- Formats : **MP4 H.264**, **MP4 H.265**, **MOV ProRes 4444** et **WebM VP9** (ces deux derniers avec **transparence**).
- Préréglages 1080p / 720p / 4K / 9:16 / 1:1 / 4:5, cadence 24 → 60 fps, rendu haute densité (devicePixelRatio 1×, 1,5×, 2×).
- Qualité réglable (CRF), conversion couleur BT.709 correcte, piste audio optionnelle.
- Progression en direct (vitesse, temps restant, aperçu), annulation propre.
- Glisser-déposer d'une page HTML (ou d'un fichier audio) sur la fenêtre.
- Une CLI (`rustmotion-cli`) pour les scripts et l'intégration continue.

## Prérequis

| Outil | Rôle |
|---|---|
| Rust (stable) + cible `wasm32-unknown-unknown` | compilation |
| [Trunk](https://trunkrs.dev) et `tauri-cli` v2 | build du frontend et de l'app |
| Google Chrome, Microsoft Edge, Chromium ou Brave | rendu des pages (détecté automatiquement) |
| ffmpeg (avec libx264 / libx265 / libvpx) | encodage (PATH, ou chemin à indiquer dans l'app) |

```bash
rustup target add wasm32-unknown-unknown
```

```bash
cargo install trunk tauri-cli --locked
```

## Lancer l'application

```bash
cd src-tauri && cargo tauri dev
```

Paquet d'installation (MSI / NSIS sous Windows) :

```bash
cd src-tauri && cargo tauri build
```

## CLI

```bash
cargo run -p rustmotion-engine --release -- examples/demo.html -o demo.mp4
```

```text
rustmotion-cli <entrée> [options]
  -o, --output <fichier>   -w, --width <px>   -h, --height <px>   -s, --scale <x>
  -r, --fps <n>            -d, --duration <s> --start <s>
  -c, --codec h264|h265|prores4444|vp9      --crf <n>   --transparent
  --audio <fichier>        --wait <ms>        --browser <chemin>   --ffmpeg <chemin>
  --probe                  --tools            --help
```

Exemple : incrustation transparente en ProRes 4444

```bash
cargo run -p rustmotion-engine --release -- examples/gsap-transparent.html -c prores4444 --transparent -o lower-third.mov
```

## Préparer une page

Aucune modification n'est nécessaire. Vous pouvez toutefois déclarer les réglages, qui seront repris par l'analyse :

```html
<meta name="rustmotion:duration" content="6">   <!-- secondes -->
<meta name="rustmotion:fps" content="30">
<meta name="rustmotion:width" content="1920">
<meta name="rustmotion:height" content="1080">
```

ou en JavaScript : `window.RUSTMOTION = { duration: 6, fps: 30 }`.

Pour un export transparent, la page doit avoir un fond transparent (`html, body { background: transparent }`).

## Fonctionnement

```
page HTML ──► serveur HTTP local ──► Chromium headless (CDP)
                                        │  shim.js injecté avant tout script :
                                        │  rAF, timers, Date, performance.now virtualisés,
                                        │  timeline des animations CSS/WAAPI gelée
                                        ▼
                     pour chaque image i : __rm.seek(i / fps) → Page.captureScreenshot (PNG)
                                        ▼
                         ffmpeg (image2pipe sur stdin) ──► MP4 / MOV / WebM
```

| Dossier | Contenu |
|---|---|
| `crates/rustmotion-types` | types partagés (natif + wasm) : réglages, progression, analyse |
| `crates/rustmotion-engine` | moteur : client CDP minimal, lancement du navigateur, serveur statique, horloge virtuelle (`shim.js`), encodeur ffmpeg, CLI |
| `src-tauri` | application Tauri v2 : commandes `probe`, `render`, `cancel_render`, dialogues de fichiers |
| `src` | interface Leptos 0.8 (CSR, compilée par Trunk) |
| `examples` | pages de démonstration |

## Limites connues

- Le temps n'est pas virtualisé dans les Web Workers, ni dans les iframes d'une autre origine.
- L'audio joué par la page n'est pas capturé : ajoutez une piste audio séparée.
- Une simulation physique qui intègre `dt` entre deux `requestAnimationFrame` avance par pas de 1/fps (comme dans un vrai navigateur à cette cadence).
- La vitesse dépend surtout de la capture d'écran de Chromium (environ 10 à 15 images/s en 1080p).
