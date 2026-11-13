# RustMotion

[![Release](https://img.shields.io/github/v/release/JeanJacquesSerpoul/rustmotion?label=release)](https://github.com/JeanJacquesSerpoul/rustmotion/releases/latest)

**Français** · [English](README.en.md)

Convertisseur d'animations **HTML / CSS / JS → vidéo** (MP4, MOV, WebM), en Rust + Tauri v2 + Leptos.

Chaque image est calculée à un instant précis grâce à une **horloge virtuelle** : le résultat est fluide et
identique d'un rendu à l'autre, même si la page est lourde ou la machine lente.

## Fonctionnalités

L'application a deux onglets :

1. **Motion design** (optionnel) : un brief, saisi ou importé depuis un **PDF** ou un fichier **texte**, est envoyé à l'IA
   via le [proxy OpenRouter](server/readme_openrouter.md). L'IA écrit une animation HTML au format et à la durée choisis,
   prévisualisée aussitôt dans un lecteur (lecture, pause, timeline) qui utilise la même horloge virtuelle que l'export :
   ce qu'on voit est ce qui sera rendu. On peut demander des ajustements (chaque version est conservée et enregistrée
   dans `Vidéos/RustMotion`), puis exporter la vidéo directement, ou passer à l'onglet de conversion pour la régler.
   Le coût réellement facturé s'affiche pour chaque génération, avec le total de la session.
   Une **bande son** peut l'accompagner : un fichier audio importé, ou une musique composée par l'IA
   (Google Lyria 3 : 0,04 $ jusqu'à 30 s, 0,08 $ au-delà), à la création de l'animation ou plus tard.
   Elle est jouée avec l'aperçu et reprise automatiquement à l'export.
2. **Conversion vidéo** : transforme n'importe quelle animation HTML en vidéo (détails ci-dessous).

L'animation courante est partagée par les deux onglets, dans une barre commune (fichier, dossier, récents, ouvrir,
afficher, sauvegarder — en .zip avec la bande son et le script compagnon s'il y en a).

Le proxy est attendu par défaut sur `http://127.0.0.1:9018/v1` ; l'URL, la clé du proxy (`PROXY_API_KEYS`) et le modèle
se règlent dans le panneau **Réglages ⚙** de l'en-tête, avec les chemins de Chrome et de ffmpeg, et sont mémorisés.
La clé OpenRouter reste sur le proxy.

Raccourcis : **Ctrl+Entrée** générer ou ajuster (ouvre la confirmation), **Échap** annuler, **Espace** lecture / pause,
**Ctrl+O** ouvrir une animation, **Ctrl+S** en sauvegarder une copie.

- Sources : fichier `.html`, dossier contenant `index.html`, ou URL `http(s)`.
  Les fichiers locaux sont servis par un petit serveur HTTP intégré : modules ES, `fetch()`, polices, JSON Lottie… fonctionnent.
- Moteurs d'animation pris en charge : animations et transitions CSS, Web Animations API (y compris `await anim.finished`),
  `requestAnimationFrame` (canvas, WebGL, three.js, Lottie…), `setTimeout` / `setInterval`, `Date` / `performance.now`,
  GSAP, anime.js, `<video>` (recalée à chaque image).
- Détection automatique de la durée (meta de la page, `window.DURATION`, lecteur intégré « 0:00 / 1:08 », vidéo ou son, timeline GSAP, ou animations CSS/WAAPI finies) et miniature d'aperçu.
- Pages pilotées par script (`.html` + `.js`) : `window.seek(t)` ou `window.__seek(t)` est appelé à chaque image ; paramètres d'URL (`?render=1`) sur les fichiers locaux.
- Formats : **MP4 H.264**, **MP4 H.265**, **MOV ProRes 4444** et **WebM VP9** (ces deux derniers avec **transparence**).
- Préréglages 1080p / 720p / 4K / 9:16 / 1:1 / 4:5, cadence 24 → 60 fps, rendu haute densité (devicePixelRatio 1×, 1,5×, 2×).
- Qualité réglable (CRF), conversion couleur BT.709 correcte, piste audio optionnelle.
- Progression en direct (vitesse, temps restant, aperçu), annulation propre.
- Cadrage automatique sur l'élément principal (canvas, svg…) : ni lecteur ni marges dans la vidéo.
- Glisser-déposer d'une page HTML (ou d'un fichier audio) sur la fenêtre.
- **Bilingue français / anglais** : interface, dialogues et messages d'erreur. La langue suit celle du système
  au premier lancement, puis le choix fait dans l'en-tête est mémorisé.
- Une CLI (`rustmotion-cli`) pour les scripts et l'intégration continue, elle aussi bilingue (`--lang fr|en`).

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

### Exécutables Windows en local

Le script [`scripts/build-windows.ps1`](scripts/build-windows.ps1) construit tout et le range dans
`release\windows` : installateurs `.msi` et `-setup.exe`, exécutable portable (sans installation) et CLI en `.zip`.

```powershell
.\scripts\build-windows.ps1
```

Options : `-Portable` (exécutable seul, plus rapide), `-SkipCli`, `-Clean` (repart d'un frontend propre),
`-OutDir <dossier>`. Aide complète : `Get-Help .\scripts\build-windows.ps1 -Detailed`.
Fermez `cargo tauri dev` avant : les deux utilisent le même dossier `dist`.
Si PowerShell refuse d'exécuter le script : `powershell -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1`.

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
  --frame auto|page|<css>  --lang fr|en       --probe            --tools              --force   --help
```

Exemple : incrustation transparente en ProRes 4444

```bash
cargo run -p rustmotion-engine --release -- examples/gsap-transparent.html -c prores4444 --transparent -o lower-third.mov
```

## Publier une version

Le workflow [`.github/workflows/release.yml`](.github/workflows/release.yml) fabrique les installateurs
et la CLI pour Windows (x64), macOS (Apple Silicon) et Linux (x64), puis les joint à une
**release brouillon** sur GitHub.

1. Mettez à jour la version dans `src-tauri/tauri.conf.json` (et, par cohérence, dans les `Cargo.toml`).
2. Committez, puis poussez un tag du même numéro :

```bash
git tag v0.4.0
```

```bash
git push origin v0.4.0
```

3. Suivez la compilation dans l'onglet **Actions** (environ 15 à 25 minutes).
4. Ouvrez la release brouillon dans **Releases**, relisez-la, puis cliquez sur **Publish release**.

Le workflow peut aussi être lancé à la main (**Actions → Release → Run workflow**) : le tag est alors
tiré de `tauri.conf.json`. Un tag qui ne correspond pas à cette version fait échouer le workflow.
Relancer le workflow sur une version encore en brouillon remplace les fichiers déjà joints.

Les binaires ne sont pas signés : Windows SmartScreen et macOS Gatekeeper affichent un avertissement
au premier lancement.

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

### Fichier HTML + fichier JS

Un motion design est souvent livré en deux fichiers : `coulisses.html` et `coulisses.js`.
RustMotion prend les deux par défaut :

- l'application affiche la page et son script de même nom ;
- si la page ne charge pas ce script elle-même, il est **ajouté automatiquement** en fin de page ;
- s'il est absent, une **alerte** l'indique : la page HTML doit alors contenir toute l'animation.

De la même façon, `coulisses.mp3` (ou `.wav`, `.m4a`, `.aac`, `.ogg`, `.flac`, `.opus`) est sa **bande son** :
elle devient la piste audio de l'export, sauf si une autre piste est choisie à la main. Une bande son ajoutée
depuis l'application est copiée sous ce nom ; celle qu'elle remplace est renommée, jamais supprimée.

Avant l'export, la page est contrôlée ; l'export est **refusé** si :

- un script ou une feuille de style demandés par la page sont introuvables (page incomplète) ;
- aucune animation n'est détectée (ni animation CSS / Web Animations, ni `requestAnimationFrame`,
  ni timer, ni GSAP, ni `window.seek`, ni vidéo) : ce n'est pas un motion design complet.

Les autres fichiers manquants (son, image…) donnent une simple alerte. En CLI, `--force` passe outre le refus.

### Page pilotée par script (HTML + JS)

Une page dont l'animation est entièrement calculée par un script (souvent dans un fichier `.js` à côté du `.html`)
peut exposer deux globales ; RustMotion les détecte et s'en sert :

```js
window.DURATION = 100        // durée totale, en secondes
window.seek = (t) => { … }   // dessine l'image à l'instant t (secondes)
window.READY = true          // facultatif : mettre false pendant le chargement, true quand tout est prêt
```

À chaque image, RustMotion avance son horloge virtuelle **puis** appelle `window.seek(t)`.
`window.WIDTH`, `window.HEIGHT` et `window.FPS` sont lus comme réglages suggérés s'ils existent.

### Cadrage

Beaucoup d'animations sont présentées dans une page avec un lecteur (boutons, curseur) et des marges.
Par défaut (**Élément principal**), RustMotion filme seulement l'élément principal de la page : le plus
grand canvas, svg, vidéo, image ou iframe visible, s'il occupe entre 40 % et 97 % de la fenêtre.
La fenêtre est ajustée pour que l'élément fasse exactement la largeur demandée : la page le redessine
à cette taille, l'image reste nette, et la hauteur suit les proportions de l'élément. Ses coins arrondis
sont neutralisés pendant la capture. Une page qui remplit déjà la fenêtre est filmée en entier.

**Page entière** filme toute la fenêtre. En CLI : `--frame auto`, `--frame page`, ou un sélecteur CSS
(`--frame "#stage"`).

### Paramètres d'URL

Beaucoup de pages ont un mode de rendu sans lecteur ni commandes (`?render=1`, `#scene3`…).
Dans l'application, saisissez-les dans **Paramètres d'URL** ; en CLI, ajoutez-les au chemin :

```bash
rustmotion-cli "coulisses.html?render=1" -w 1280 -h 720 -s 1.5 --audio bande-son.m4a
```

La miniature d'aperçu est recalculée quand le format change : elle montre le cadrage exact de la vidéo.
Une scène de taille fixe (ici 1280 × 720) se rend avec ces mêmes dimensions, et le facteur 1,5× donne du 1080p net.

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
| `crates/rustmotion-engine` | moteur : client CDP minimal, lancement du navigateur, serveur statique, horloge virtuelle (`shim.js`), encodeur ffmpeg, messages FR/EN (`i18n.rs`, macro `tr!`), CLI |
| `src-tauri` | application Tauri v2 : commandes `probe`, `render`, `cancel_render`, dialogues de fichiers |
| `src` | interface Leptos 0.8 (CSR, compilée par Trunk), textes FR/EN dans `i18n.rs` |
| `examples` | pages de démonstration |

## Limites connues

- Le temps n'est pas virtualisé dans les Web Workers, ni dans les iframes d'une autre origine.
- L'audio joué par la page n'est pas capturé : ajoutez une piste audio séparée.
- Une simulation physique qui intègre `dt` entre deux `requestAnimationFrame` avance par pas de 1/fps (comme dans un vrai navigateur à cette cadence).
- La vitesse dépend surtout de la capture d'écran de Chromium (environ 10 à 15 images/s en 1080p).

## Licence

Distribué sous licence [MIT](LICENSE).
