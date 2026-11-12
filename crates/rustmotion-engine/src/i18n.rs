//! Langue des messages du moteur (erreurs, CLI).
//!
//! Réglage global : l'application l'aligne sur la langue de l'interface,
//! la CLI sur `--lang` ou la langue du système.

use std::sync::atomic::{AtomicU8, Ordering};

use rustmotion_types::Lang;

const UNSET: u8 = 0;
const FR: u8 = 1;
const EN: u8 = 2;

static LANG: AtomicU8 = AtomicU8::new(UNSET);

/// Langue du système (« fr-FR » → français, sinon anglais).
pub fn system_lang() -> Lang {
    sys_locale::get_locale().map(|l| Lang::from_code(&l)).unwrap_or(Lang::En)
}

pub fn set_lang(lang: Lang) {
    LANG.store(if lang == Lang::Fr { FR } else { EN }, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        FR => Lang::Fr,
        EN => Lang::En,
        _ => {
            let l = system_lang();
            set_lang(l);
            l
        }
    }
}

/// `tr!("texte français", "English text")` : choisit selon la langue courante.
/// Seule la branche retenue est évaluée (utilisable avec `format!`).
#[macro_export]
macro_rules! tr {
    ($fr:expr, $en:expr $(,)?) => {
        if $crate::i18n::lang() == $crate::types::Lang::Fr {
            $fr
        } else {
            $en
        }
    };
}
