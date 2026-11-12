//! Détection et lancement d'un navigateur Chromium (Chrome, Edge, Chromium, Brave) en mode headless.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use crate::tr;
use crate::util::{hide_console, which};

/// Cherche un exécutable Chromium installé.
pub fn find_browser() -> Option<PathBuf> {
    for var in ["RUSTMOTION_BROWSER", "CHROME_PATH", "BROWSER_PATH"] {
        if let Ok(p) = std::env::var(var) {
            let p = PathBuf::from(p);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        let roots = [
            std::env::var("PROGRAMFILES").ok(),
            std::env::var("PROGRAMFILES(X86)").ok(),
            std::env::var("LOCALAPPDATA").ok(),
        ];
        for root in roots.into_iter().flatten() {
            let r = PathBuf::from(root);
            candidates.push(r.join(r"Google\Chrome\Application\chrome.exe"));
            candidates.push(r.join(r"Chromium\Application\chrome.exe"));
            candidates.push(r.join(r"Microsoft\Edge\Application\msedge.exe"));
            candidates.push(r.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        for p in [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ] {
            candidates.push(PathBuf::from(p));
        }
    }
    if let Some(p) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(p);
    }
    for name in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
        "microsoft-edge",
        "msedge",
    ] {
        if let Some(p) = which(name) {
            return Some(p);
        }
    }
    None
}

/// Processus navigateur headless ; tué et nettoyé au `drop`.
pub struct Browser {
    child: Child,
    profile_dir: PathBuf,
    pub ws_url: String,
}

impl Browser {
    pub fn launch(explicit: Option<&str>) -> Result<Self> {
        let exe = match explicit.filter(|s| !s.trim().is_empty()) {
            Some(p) => PathBuf::from(p),
            None => find_browser().ok_or_else(|| {
                anyhow!(tr!(
                    "aucun navigateur Chromium trouvé (installez Chrome ou Edge, ou définissez RUSTMOTION_BROWSER)",
                    "no Chromium browser found (install Chrome or Edge, or set RUSTMOTION_BROWSER)"
                ))
            })?,
        };
        let profile_dir = std::env::temp_dir().join(format!(
            "rustmotion-profile-{}-{}",
            std::process::id(),
            crate::util::unique_suffix()
        ));
        std::fs::create_dir_all(&profile_dir)?;

        let mut cmd = Command::new(&exe);
        cmd.args([
            "--headless=new",
            "--remote-debugging-port=0",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-extensions",
            "--disable-sync",
            "--disable-default-apps",
            "--hide-scrollbars",
            "--mute-audio",
            "--disable-background-timer-throttling",
            "--disable-backgrounding-occluded-windows",
            "--disable-renderer-backgrounding",
            "--disable-features=Translate,MediaRouter,OptimizationHints,CalculateNativeWinOcclusion",
            "--force-color-profile=srgb",
            "--font-render-hinting=none",
            "--autoplay-policy=no-user-gesture-required",
            "--enable-unsafe-swiftshader",
            "--allow-file-access-from-files",
        ])
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
        hide_console(&mut cmd);

        let mut child = cmd
            .spawn()
            .with_context(|| tr!(format!("impossible de lancer {}", exe.display()), format!("cannot launch {}", exe.display())))?;

        // Chromium écrit « DevTools listening on ws://… » sur stderr.
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            let mut sent = false;
            for line in reader.lines().map_while(Result::ok) {
                if !sent {
                    if let Some(idx) = line.find("ws://") {
                        let _ = tx.send(line[idx..].trim().to_string());
                        sent = true;
                    }
                }
                // On continue de vider stderr pour ne pas bloquer le navigateur.
            }
        });

        let ws_url = match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(u) => u,
            Err(_) => {
                let _ = child.kill();
                return Err(anyhow!(tr!(
                    format!("le navigateur {} n'a pas exposé DevTools à temps", exe.display()),
                    format!("browser {} did not expose DevTools in time", exe.display())
                )));
            }
        };
        Ok(Self { child, profile_dir, ws_url })
    }

    pub fn profile_dir(&self) -> &Path {
        &self.profile_dir
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Le navigateur peut garder des verrous quelques instants.
        for _ in 0..10 {
            if std::fs::remove_dir_all(&self.profile_dir).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
