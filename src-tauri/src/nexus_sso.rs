use crate::error::{AppError, AppResult};
use crate::nexus::ValidateResult;
use crate::state::AppState;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::tungstenite::Message;

const SSO_WS: &str = "wss://sso.nexusmods.com";
/// Application slug shown on the Nexus authorization page. Slugs must be
/// registered by Nexus staff; "vortex" is the slug their own SSO demo uses
/// (https://github.com/Nexus-Mods/sso-integration-demo), so the authorize
/// page will display "Vortex". Swap for our own slug if Nexus grants one.
const APP_SLUG: &str = "vortex";
/// How long to wait for the user to authorize in their browser.
const AUTH_TIMEOUT: Duration = Duration::from_secs(300);

/// Single-sign-on login: opens the browser to authorize, then receives the
/// API key over a websocket — no manual key entry. Vortex/MO2-style flow.
pub async fn login(state: &AppState, app: &AppHandle) -> AppResult<ValidateResult> {
    let id = uuid::Uuid::new_v4().to_string();

    let (mut ws, _) = tokio_tungstenite::connect_async(SSO_WS)
        .await
        .map_err(|e| AppError::msg(format!("could not reach Nexus SSO: {e}")))?;

    // Ask the SSO server to register this session.
    let req = serde_json::json!({ "id": id, "token": serde_json::Value::Null, "protocol": 2 });
    ws.send(Message::Text(req.to_string()))
        .await
        .map_err(|e| AppError::msg(format!("SSO handshake failed: {e}")))?;

    let mut browser_opened = false;
    let api_key = loop {
        let next = tokio::time::timeout(AUTH_TIMEOUT, ws.next())
            .await
            .map_err(|_| AppError::msg("Nexus login timed out — please try again"))?;
        let msg = match next {
            Some(Ok(m)) => m,
            Some(Err(e)) => return Err(AppError::msg(format!("SSO connection error: {e}"))),
            None => return Err(AppError::msg("Nexus closed the login connection")),
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Ping(p) => {
                let _ = ws.send(Message::Pong(p)).await;
                continue;
            }
            Message::Close(_) => return Err(AppError::msg("Nexus closed the login connection")),
            _ => continue,
        };

        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            if !err.is_empty() {
                return Err(AppError::msg(format!("Nexus SSO: {err}")));
            }
        }
        let data = v.get("data");

        // Once the session is registered, send the user to the authorize page.
        if !browser_opened && data.and_then(|d| d.get("connection_token")).is_some() {
            let url = format!("https://www.nexusmods.com/sso?id={id}&application={APP_SLUG}");
            let _ = app.emit("nexus-sso://url", &url);
            let _ = open::that(&url);
            browser_opened = true;
        }

        // The key arrives after the user authorizes.
        if let Some(key) = data.and_then(|d| d.get("api_key")).and_then(|k| k.as_str()) {
            break key.to_string();
        }
    };

    let _ = ws.close(None).await;

    // Validate and persist exactly like a manually-entered key.
    let result = crate::nexus::validate(state, &api_key).await?;
    if result.valid {
        let mut s = state.settings();
        s.nexus_api_key = Some(api_key);
        s.is_premium = result.is_premium;
        s.nexus_user = result.name.clone();
        state.save_settings(s)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Confirms the real Nexus SSO server accepts our handshake and returns a
    // session token. The browser/api_key step needs a human, so it's not tested.
    // Run with: cargo test -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn sso_handshake_returns_connection_token() {
        let id = uuid::Uuid::new_v4().to_string();
        let (mut ws, _) = tokio_tungstenite::connect_async(SSO_WS).await.expect("connect");
        let req = serde_json::json!({ "id": id, "token": serde_json::Value::Null, "protocol": 2 });
        ws.send(Message::Text(req.to_string())).await.expect("send");

        let mut got_token = false;
        for _ in 0..5 {
            let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
                .await
                .expect("timeout")
                .expect("stream end")
                .expect("ws error");
            if let Message::Text(t) = msg {
                println!("SSO reply: {t}");
                let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
                if v.pointer("/data/connection_token").is_some() {
                    got_token = true;
                    break;
                }
            }
        }
        let _ = ws.close(None).await;
        assert!(got_token, "expected a connection_token from Nexus SSO");
    }
}
