//! Icônes SVG de l'interface : tracés simples au trait (grille 24×24), qui prennent la couleur
//! du texte (`currentColor`). Un bouton qui n'affiche qu'une icône porte toujours un `title`
//! et un `aria-label`.

use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    File,
    Folder,
    History,
    ChevronDown,
    External,
    Reveal,
    Save,
    Settings,
    Play,
    Pause,
    Restart,
    Volume,
    VolumeOff,
    Close,
    Trash,
    Upload,
    Refresh,
    Sparkles,
    Music,
    Video,
    Wand,
}

impl Icon {
    fn paths(self) -> &'static str {
        match self {
            Icon::File => r#"<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>"#,
            Icon::Folder => r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#,
            Icon::History => r#"<path d="M3.5 12a8.5 8.5 0 1 0 2.6-6.1L3.5 8.5"/><path d="M3.5 3.5v5h5"/><path d="M12 7.5V12l3 2"/>"#,
            Icon::ChevronDown => r#"<path d="m7 10 5 5 5-5"/>"#,
            Icon::External => r#"<path d="M14 4h6v6"/><path d="M20 4l-9 9"/><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/>"#,
            Icon::Reveal => r#"<path d="M21 11V9a2 2 0 0 0-2-2h-8l-2-2H5a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h6"/><circle cx="17" cy="16" r="3"/><path d="m21.5 20.5-2.3-2.3"/>"#,
            Icon::Save => r#"<path d="M12 4v11"/><path d="m7 10 5 5 5-5"/><path d="M5 20h14"/>"#,
            Icon::Settings => r#"<path d="M4 6h9"/><path d="M19 6h1"/><circle cx="16" cy="6" r="2.5"/><path d="M4 12h3"/><path d="M13 12h7"/><circle cx="10" cy="12" r="2.5"/><path d="M4 18h11"/><circle cx="18" cy="18" r="2.5"/>"#,
            Icon::Play => r#"<path d="M7 4.8v14.4a.8.8 0 0 0 1.2.7l11.3-7.2a.8.8 0 0 0 0-1.4L8.2 4.1a.8.8 0 0 0-1.2.7z" fill="currentColor" stroke="none"/>"#,
            Icon::Pause => r#"<rect x="6" y="4.5" width="4" height="15" rx="1" fill="currentColor" stroke="none"/><rect x="14" y="4.5" width="4" height="15" rx="1" fill="currentColor" stroke="none"/>"#,
            Icon::Restart => r#"<path d="M3.5 12a8.5 8.5 0 1 0 2.6-6.1L3.5 8.5"/><path d="M3.5 3.5v5h5"/>"#,
            Icon::Volume => r#"<path d="M11 5 6 9H3v6h3l5 4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/><path d="M18.5 5.5a9 9 0 0 1 0 13"/>"#,
            Icon::VolumeOff => r#"<path d="M11 5 6 9H3v6h3l5 4z"/><path d="m16 9 6 6"/><path d="m22 9-6 6"/>"#,
            Icon::Close => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
            Icon::Trash => r#"<path d="M4 7h16"/><path d="M9 7V4.5h6V7"/><path d="m6 7 1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12"/><path d="M10 11v6"/><path d="M14 11v6"/>"#,
            Icon::Upload => r#"<path d="M12 16V4"/><path d="m7 9 5-5 5 5"/><path d="M5 20h14"/>"#,
            Icon::Refresh => r#"<path d="M20.5 12a8.5 8.5 0 1 1-2.6-6.1l2.6 2.6"/><path d="M20.5 3.5v5h-5"/>"#,
            Icon::Sparkles => r#"<path d="M11 3.5 12.6 8l4.4 1.6-4.4 1.6L11 15.7 9.4 11.2 5 9.6 9.4 8z"/><path d="M18 14.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"/>"#,
            Icon::Music => r#"<path d="M9 18V5.5l11-2V16"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="17.5" cy="16" r="2.5"/>"#,
            Icon::Video => r#"<rect x="3" y="5" width="18" height="14" rx="2.5"/><path d="M10 9.2v5.6l4.6-2.8z" fill="currentColor" stroke="none"/>"#,
            Icon::Wand => r#"<path d="m4 20 10-10"/><path d="m12.5 8.5 3 3"/><path d="M17 3.5v3"/><path d="M15.5 5h3"/><path d="M20 9.5v2"/><path d="M19 10.5h2"/>"#,
        }
    }
}

/// Icône décorative (masquée aux lecteurs d'écran : le bouton porte son propre libellé).
pub fn icon(i: Icon) -> impl IntoView {
    view! { <svg class="ico" viewBox="0 0 24 24" aria-hidden="true" focusable="false" inner_html=i.paths()></svg> }
}
