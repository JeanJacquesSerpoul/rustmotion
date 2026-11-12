//! Fichiers qui composent une source : la page HTML et son script compagnon de même nom
//! (`coulisses.html` + `coulisses.js`), un format très courant pour le motion design.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use rustmotion_types::SourceFiles;

use crate::tr;

/// Une page locale résolue sur le disque.
pub struct LocalPage {
    /// Dossier servi par le serveur local.
    pub root: PathBuf,
    /// Nom du fichier HTML dans ce dossier.
    pub file: String,
    /// Paramètres d'URL (`?render=1`, `#scene`) à transmettre à la page.
    pub suffix: String,
    /// Script compagnon à ajouter à la page parce qu'elle ne le charge pas elle-même.
    pub inject_script: Option<String>,
    pub files: SourceFiles,
}

fn is_url(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn clean(input: &str) -> &str {
    input.trim().trim_matches('"').trim()
}

/// Sépare un chemin local de ses paramètres d'URL (`?…` / `#…`).
/// Un chemin qui existe tel quel n'est jamais découpé (un `#` peut faire partie d'un nom de fichier).
pub fn split_query(input: &str) -> (String, String) {
    if Path::new(input).exists() {
        return (input.to_string(), String::new());
    }
    match input.find(['?', '#']) {
        Some(i) => (input[..i].to_string(), input[i..].to_string()),
        None => (input.to_string(), String::new()),
    }
}

/// Résout une source locale ; `None` pour une URL http(s).
pub fn locate(input: &str) -> Result<Option<LocalPage>> {
    let input = clean(input);
    if input.is_empty() {
        bail!(tr!("aucune source indiquée", "no input given"));
    }
    if is_url(input) {
        return Ok(None);
    }
    let path_str = if input.to_ascii_lowercase().starts_with("file:///") {
        let decoded = percent_encoding::percent_decode_str(&input[8..]).decode_utf8_lossy().to_string();
        if cfg!(windows) { decoded } else { format!("/{decoded}") }
    } else {
        input.to_string()
    };
    // « page.html?render=1#scene » : la partie après ? ou # est transmise telle quelle à la page.
    let (path_str, suffix) = split_query(&path_str);
    let path = PathBuf::from(&path_str)
        .canonicalize()
        .with_context(|| tr!(format!("fichier introuvable : {path_str}"), format!("file not found: {path_str}")))?;
    let (root, file) = if path.is_dir() {
        if !path.join("index.html").is_file() {
            bail!(tr!(
                format!("le dossier ne contient pas d'index.html : {}", path.display()),
                format!("the folder has no index.html: {}", path.display())
            ));
        }
        (path.clone(), "index.html".to_string())
    } else {
        let root = path.parent().ok_or_else(|| anyhow!(tr!("chemin invalide", "invalid path")))?.to_path_buf();
        (root, path.file_name().unwrap().to_string_lossy().to_string())
    };

    // Script compagnon : même nom que la page, extension .js.
    let stem = Path::new(&file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let expected = format!("{stem}.js");
    let script_found = root.join(&expected).is_file();
    let html = std::fs::read_to_string(root.join(&file)).unwrap_or_default();
    let script_linked = script_srcs(&html).iter().any(|src| same_file(src, &expected));
    let files = SourceFiles {
        html: Some(file.clone()),
        expected_script: Some(expected.clone()),
        script_found,
        script_linked,
    };
    let inject_script = files.script_injected().then_some(expected);
    Ok(Some(LocalPage { root, file, suffix, inject_script, files }))
}

/// Fichiers de la source, sans lancer de navigateur (pour l'affichage immédiat).
pub fn inspect_source(input: &str) -> Result<SourceFiles> {
    Ok(locate(input)?.map(|p| p.files).unwrap_or_default())
}

/// Valeurs des attributs `src` des balises `<script>` d'un document HTML.
pub fn script_srcs(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = lower[pos..].find("<script") {
        let start = pos + i;
        let end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &html[start..end];
        let tag_lower = &lower[start..end];
        if let Some(s) = tag_lower.find("src") {
            let rest = tag[s + 3..].trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim_start();
                let value = match rest.chars().next() {
                    Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
                    _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
                };
                if !value.is_empty() {
                    out.push(value.to_string());
                }
            }
        }
        pos = end.max(start + 7);
    }
    out
}

/// `src` désigne-t-il le fichier `name` du même dossier (`./x.js`, `x.js?v=2`) ?
fn same_file(src: &str, name: &str) -> bool {
    let src = src.split(['?', '#']).next().unwrap_or(src);
    let src = src.strip_prefix("./").unwrap_or(src);
    src.eq_ignore_ascii_case(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_script_sources() {
        let html = r#"<head><SCRIPT src="a.js"></SCRIPT><script>var x</script>
            <script type="module" src='./b.mjs?v=1'></script><script src=c.js defer></script>"#;
        assert_eq!(script_srcs(html), vec!["a.js", "./b.mjs?v=1", "c.js"]);
    }

    #[test]
    fn matches_companion() {
        assert!(same_file("./coulisses.js?v=3", "coulisses.js"));
        assert!(same_file("Coulisses.JS", "coulisses.js"));
        assert!(!same_file("lib/coulisses.js", "coulisses.js"));
    }
}
