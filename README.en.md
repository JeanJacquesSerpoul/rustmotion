# RustMotion

[Français](README.md) · **English**

Converts **HTML / CSS / JS animations → video** (MP4, MOV, WebM), built with Rust + Tauri v2 + Leptos.

Every frame is computed at an exact moment thanks to a **virtual clock**: the result is smooth and
identical from one render to the next, even when the page is heavy or the machine is slow.

## Features

- Inputs: an `.html` file, a folder containing `index.html`, or an `http(s)` URL.
  Local files are served by a small built-in HTTP server, so ES modules, `fetch()`, fonts, Lottie JSON… all work.
- Supported animation engines: CSS animations and transitions, the Web Animations API (including `await anim.finished`),
  `requestAnimationFrame` (canvas, WebGL, three.js, Lottie…), `setTimeout` / `setInterval`, `Date` / `performance.now`,
  GSAP, anime.js, `<video>` (re-synced on every frame).
- Automatic duration detection (page meta tags, `window.DURATION`, built-in player « 0:00 / 1:08 », video or audio, GSAP timeline, or finite CSS/WAAPI animations) with a preview thumbnail.
- Script-driven pages (`.html` + `.js`): `window.seek(t)` or `window.__seek(t)` is called on every frame; URL parameters (`?render=1`) on local files.
- Formats: **MP4 H.264**, **MP4 H.265**, **MOV ProRes 4444** and **WebM VP9** (the last two with **transparency**).
- Presets 1080p / 720p / 4K / 9:16 / 1:1 / 4:5, 24 → 60 fps, high-density rendering (devicePixelRatio 1×, 1.5×, 2×).
- Adjustable quality (CRF), correct BT.709 colour conversion, optional audio track.
- Live progress (speed, time left, preview) and clean cancellation.
- Automatic framing on the main element (canvas, svg…): no player or margins in the video.
- Drag and drop an HTML page (or an audio file) onto the window.
- **Bilingual French / English**: interface, dialogs and error messages. The language follows the system
  on first launch, then the choice made in the header is remembered.
- A CLI (`rustmotion-cli`) for scripts and CI, also bilingual (`--lang fr|en`).

## Requirements

| Tool | Purpose |
|---|---|
| Rust (stable) + the `wasm32-unknown-unknown` target | compilation |
| [Trunk](https://trunkrs.dev) and `tauri-cli` v2 | building the frontend and the app |
| Google Chrome, Microsoft Edge, Chromium or Brave | rendering pages (detected automatically) |
| ffmpeg (with libx264 / libx265 / libvpx) | encoding (on the PATH, or set its path in the app) |

```bash
rustup target add wasm32-unknown-unknown
```

```bash
cargo install trunk tauri-cli --locked
```

## Running the app

```bash
cd src-tauri && cargo tauri dev
```

Installer package (MSI / NSIS on Windows):

```bash
cd src-tauri && cargo tauri build
```

### Windows executables locally

The [`scripts/build-windows.ps1`](scripts/build-windows.ps1) script builds everything into `release\windows`:
`.msi` and `-setup.exe` installers, a portable executable (no install) and the CLI as a `.zip`.

```powershell
.\scripts\build-windows.ps1
```

Options: `-Portable` (executable only, faster), `-SkipCli`, `-Clean` (starts from a clean frontend),
`-OutDir <folder>`. Full help: `Get-Help .\scripts\build-windows.ps1 -Detailed`.
Close `cargo tauri dev` first: both use the same `dist` folder.
If PowerShell refuses to run the script: `powershell -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1`.

## CLI

```bash
cargo run -p rustmotion-engine --release -- examples/demo.html -o demo.mp4 --lang en
```

```text
rustmotion-cli <input> [options]
  -o, --output <file>      -w, --width <px>   -h, --height <px>   -s, --scale <x>
  -r, --fps <n>            -d, --duration <s> --start <s>
  -c, --codec h264|h265|prores4444|vp9      --crf <n>   --transparent
  --audio <file>           --wait <ms>        --browser <path>     --ffmpeg <path>
  --frame auto|page|<css>  --lang fr|en       --probe            --tools              --force   --help
```

Example: a transparent overlay in ProRes 4444

```bash
cargo run -p rustmotion-engine --release -- examples/gsap-transparent.html -c prores4444 --transparent -o lower-third.mov
```

## Publishing a version

The [`.github/workflows/release.yml`](.github/workflows/release.yml) workflow builds the installers
and the CLI for Windows (x64), macOS (Apple Silicon) and Linux (x64), then attaches them to a
**draft release** on GitHub.

1. Update the version in `src-tauri/tauri.conf.json` (and, for consistency, in the `Cargo.toml` files).
2. Commit, then push a tag with the same number:

```bash
git tag v0.2.0
```

```bash
git push origin v0.2.0
```

3. Follow the build in the **Actions** tab (about 15 to 25 minutes).
4. Open the draft in **Releases**, review it, then click **Publish release**.

The workflow can also be started by hand (**Actions → Release → Run workflow**): the tag then comes
from `tauri.conf.json`. A tag that does not match that version makes the workflow fail.
Re-running the workflow on a version that is still a draft replaces the files already attached.

Binaries are not signed: Windows SmartScreen and macOS Gatekeeper show a warning on first launch.

## Preparing a page

No changes are required. You can, however, declare settings that the analysis will pick up:

```html
<meta name="rustmotion:duration" content="6">   <!-- seconds -->
<meta name="rustmotion:fps" content="30">
<meta name="rustmotion:width" content="1920">
<meta name="rustmotion:height" content="1080">
```

or in JavaScript: `window.RUSTMOTION = { duration: 6, fps: 30 }`.

For a transparent export, the page must have a transparent background (`html, body { background: transparent }`).

### HTML file + JS file

A motion design often ships as two files: `coulisses.html` and `coulisses.js`.
RustMotion takes both by default:

- the app shows the page and its script with the same name;
- if the page does not load that script itself, it is **added automatically** at the end of the page;
- if it is missing, a **warning** says so: the HTML page must then contain the whole animation.

The page is checked before exporting; the export is **refused** when:

- a script or stylesheet requested by the page cannot be found (incomplete page);
- no animation is detected (no CSS / Web Animations, `requestAnimationFrame`, timer, GSAP,
  `window.seek` or video): it is not a complete motion design.

Other missing files (sound, image…) only raise a warning. With the CLI, `--force` overrides the refusal.

### Script-driven pages (HTML + JS)

A page whose animation is fully computed by a script (often a `.js` file next to the `.html`)
can expose two globals; RustMotion detects and uses them:

```js
window.DURATION = 100        // total duration, in seconds
window.seek = (t) => { … }   // draws the frame at time t (seconds)
window.READY = true          // optional: false while loading, true once everything is ready
```

On every frame, RustMotion advances its virtual clock **then** calls `window.seek(t)`.
`window.WIDTH`, `window.HEIGHT` and `window.FPS` are read as suggested settings when present.

### Framing

Many animations sit in a page with a player (buttons, slider) and margins. By default (**Main element**),
RustMotion films only the page's main element: the largest visible canvas, svg, video, image or iframe,
when it covers between 40 % and 97 % of the window. The window is resized so the element is exactly the
requested width: the page redraws it at that size, the image stays sharp, and the height follows the
element's proportions. Its rounded corners are neutralised during capture. A page that already fills the
window is filmed whole.

**Whole page** films the entire window. With the CLI: `--frame auto`, `--frame page`, or a CSS selector
(`--frame "#stage"`).

### URL parameters

Many pages have a render mode without player or controls (`?render=1`, `#scene3`…).
In the app, type them into **URL parameters**; with the CLI, append them to the path:

```bash
rustmotion-cli "coulisses.html?render=1" -w 1280 -h 720 -s 1.5 --audio soundtrack.m4a
```

The preview thumbnail is refreshed when the format changes, so it shows the video's exact framing.
A fixed-size stage (here 1280 × 720) is rendered at those dimensions, and the 1.5× scale gives sharp 1080p.

## How it works

```
HTML page ──► local HTTP server ──► headless Chromium (CDP)
                                        │  shim.js injected before any page script:
                                        │  rAF, timers, Date, performance.now virtualised,
                                        │  CSS/WAAPI animation timeline frozen
                                        ▼
                     for each frame i: __rm.seek(i / fps) → Page.captureScreenshot (PNG)
                                        ▼
                         ffmpeg (image2pipe on stdin) ──► MP4 / MOV / WebM
```

| Folder | Contents |
|---|---|
| `crates/rustmotion-types` | shared types (native + wasm): settings, progress, analysis, language |
| `crates/rustmotion-engine` | engine: minimal CDP client, browser launcher, static server, virtual clock (`shim.js`), ffmpeg encoder, FR/EN messages (`i18n.rs`, `tr!` macro), CLI |
| `src-tauri` | Tauri v2 app: `probe`, `render`, `cancel_render`, `set_lang` commands, file dialogs |
| `src` | Leptos 0.8 interface (CSR, built by Trunk), FR/EN strings in `i18n.rs` |
| `examples` | demo pages |

## Known limitations

- Time is not virtualised inside Web Workers, nor in cross-origin iframes.
- Audio played by the page is not captured: add a separate audio track.
- A physics simulation that integrates `dt` between two `requestAnimationFrame` calls advances in 1/fps steps (as in a real browser at that frame rate).
- Speed mostly depends on Chromium's screenshot capture (about 10 to 15 frames/s at 1080p).

## License

Released under the [MIT](LICENSE) license.
