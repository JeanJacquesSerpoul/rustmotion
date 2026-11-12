//! Client CDP (Chrome DevTools Protocol) minimal et synchrone, sur WebSocket.
//!
//! Une seule connexion « navigateur » ; les pages sont pilotées via des sessions
//! aplaties (`Target.attachToTarget { flatten: true }`).

use std::collections::VecDeque;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use crate::tr;

pub struct Cdp {
    ws: WebSocket<TcpStream>,
    next_id: u64,
    /// Événements reçus pendant l'attente d'une réponse.
    events: VecDeque<Value>,
}

impl Cdp {
    pub fn connect(ws_url: &str) -> Result<Self> {
        let rest = ws_url
            .strip_prefix("ws://")
            .ok_or_else(|| anyhow!(tr!(format!("URL DevTools inattendue : {ws_url}"), format!("unexpected DevTools URL: {ws_url}"))))?;
        let host = rest.split('/').next().unwrap_or(rest);
        let stream = TcpStream::connect(host).with_context(|| tr!(format!("connexion à {host}"), format!("connecting to {host}")))?;
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(Duration::from_secs(120))).ok();
        let config = WebSocketConfig::default()
            .max_message_size(Some(1 << 30))
            .max_frame_size(Some(1 << 30));
        let (ws, _) = tungstenite::client::client_with_config(ws_url, stream, Some(config))
            .map_err(|e| anyhow!(tr!(format!("handshake WebSocket DevTools : {e}"), format!("DevTools WebSocket handshake: {e}"))))?;
        Ok(Self { ws, next_id: 1, events: VecDeque::new() })
    }

    fn read_json(&mut self) -> Result<Value> {
        loop {
            let msg = self.ws.read().context(tr!("lecture WebSocket DevTools", "reading DevTools WebSocket"))?;
            match msg {
                Message::Text(t) => return Ok(serde_json::from_str(t.as_str())?),
                Message::Binary(b) => return Ok(serde_json::from_slice(&b)?),
                Message::Close(_) => bail!(tr!("le navigateur a fermé la connexion DevTools", "the browser closed the DevTools connection")),
                _ => continue,
            }
        }
    }

    /// Envoie une commande et attend sa réponse. `session` = sessionId de la page, ou None pour le navigateur.
    pub fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        self.ws
            .send(Message::Text(msg.to_string().into()))
            .with_context(|| tr!(format!("envoi de {method}"), format!("sending {method}")))?;
        loop {
            let v = self.read_json()?;
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(err) = v.get("error") {
                    let m = err.get("message").and_then(Value::as_str).unwrap_or(tr!("erreur inconnue", "unknown error"));
                    bail!(tr!(format!("{method} : {m}"), format!("{method}: {m}")));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
            if v.get("method").is_some() {
                // On garde un historique borné des événements.
                if self.events.len() > 256 {
                    self.events.pop_front();
                }
                self.events.push_back(v);
            }
        }
    }

    /// Attend un événement donné (déjà reçu ou à venir).
    pub fn wait_event(&mut self, session: Option<&str>, method: &str, timeout: Duration) -> Result<Value> {
        let matches = |v: &Value| {
            v.get("method").and_then(Value::as_str) == Some(method)
                && (session.is_none() || v.get("sessionId").and_then(Value::as_str) == session)
        };
        if let Some(pos) = self.events.iter().position(matches) {
            return Ok(self.events.remove(pos).unwrap());
        }
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() > deadline {
                bail!(tr!(format!("délai dépassé en attendant {method}"), format!("timed out waiting for {method}")));
            }
            let v = self.read_json()?;
            if matches(&v) {
                return Ok(v);
            }
        }
    }

    /// Vide les événements mémorisés.
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    /// Événements console/exception mémorisés (pour diagnostic).
    pub fn take_errors(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        self.events.retain(|v| {
            let m = v.get("method").and_then(Value::as_str).unwrap_or("");
            if m == "Runtime.exceptionThrown" {
                let d = &v["params"]["exceptionDetails"];
                let text = d["exception"]["description"]
                    .as_str()
                    .or_else(|| d["text"].as_str())
                    .unwrap_or("exception");
                out.push(text.lines().next().unwrap_or("").to_string());
                false
            } else {
                m != "Runtime.consoleAPICalled"
            }
        });
        out
    }
}

/// Une page (onglet) attachée.
pub struct Page {
    pub session: String,
    pub target_id: String,
}

impl Page {
    pub fn create(cdp: &mut Cdp) -> Result<Self> {
        let r = cdp.call(None, "Target.createTarget", json!({ "url": "about:blank" }))?;
        let target_id = r["targetId"].as_str().ok_or_else(|| anyhow!(tr!("pas de targetId", "no targetId")))?.to_string();
        let r = cdp.call(None, "Target.attachToTarget", json!({ "targetId": target_id, "flatten": true }))?;
        let session = r["sessionId"].as_str().ok_or_else(|| anyhow!(tr!("pas de sessionId", "no sessionId")))?.to_string();
        Ok(Self { session, target_id })
    }

    pub fn call(&self, cdp: &mut Cdp, method: &str, params: Value) -> Result<Value> {
        cdp.call(Some(&self.session), method, params)
    }

    /// Évalue une expression JS (en attendant les promesses) et renvoie sa valeur JSON.
    pub fn eval(&self, cdp: &mut Cdp, expr: &str) -> Result<Value> {
        let r = self.call(
            cdp,
            "Runtime.evaluate",
            json!({ "expression": expr, "awaitPromise": true, "returnByValue": true }),
        )?;
        if let Some(ex) = r.get("exceptionDetails") {
            let text = ex["exception"]["description"]
                .as_str()
                .or_else(|| ex["text"].as_str())
                .unwrap_or("JS exception");
            bail!(tr!(format!("erreur JS : {text}"), format!("JS error: {text}")));
        }
        Ok(r["result"]["value"].clone())
    }

    /// Capture d'écran ; renvoie le base64 brut.
    pub fn screenshot_b64(&self, cdp: &mut Cdp, params: Value) -> Result<String> {
        let r = self.call(cdp, "Page.captureScreenshot", params)?;
        r["data"].as_str().map(str::to_string).ok_or_else(|| anyhow!(tr!("capture vide", "empty screenshot")))
    }
}
