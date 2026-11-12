//! Liaisons minimales vers l'API JS globale de Tauri (`withGlobalTauri: true`).

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn tauri_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen)]
    async fn tauri_listen(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> JsValue;
}

fn to_js<T: Serialize>(v: &T) -> JsValue {
    v.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .unwrap_or(JsValue::UNDEFINED)
}

/// Appelle une commande Tauri et désérialise sa réponse.
pub async fn invoke<A: Serialize, R: DeserializeOwned>(cmd: &str, args: &A) -> Result<R, String> {
    match tauri_invoke(cmd, to_js(args)).await {
        Ok(v) => serde_wasm_bindgen::from_value(v).map_err(|e| e.to_string()),
        Err(e) => Err(e.as_string().unwrap_or_else(|| format!("{e:?}"))),
    }
}

/// Écoute un événement Tauri pendant toute la vie de l'application.
pub fn listen<T: DeserializeOwned + 'static>(event: &'static str, mut f: impl FnMut(T) + 'static) {
    let cb = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
        let payload = js_sys::Reflect::get(&ev, &JsValue::from_str("payload")).unwrap_or(JsValue::NULL);
        if let Ok(v) = serde_wasm_bindgen::from_value::<T>(payload) {
            f(v);
        }
    });
    wasm_bindgen_futures::spawn_local(async move {
        tauri_listen(event, &cb).await;
        cb.forget();
    });
}

/// Tauri est-il disponible (sinon : ouverture dans un simple navigateur) ?
pub fn available() -> bool {
    let w = web_window();
    js_sys::Reflect::get(&w, &JsValue::from_str("__TAURI__"))
        .map(|v| !v.is_undefined())
        .unwrap_or(false)
}

fn web_window() -> JsValue {
    js_sys::global().into()
}
