//! HTTPS calls to the pillkit server. Everything returned is a signed envelope;
//! verification happens in the controller, not here.

use std::time::Duration;

use serde_json::{json, Value};

pub const DEFAULT_SERVER: &str = "https://api.pillkit.dev";

fn agent() -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new();
    // The OS TLS stack (SChannel / Security.framework / OpenSSL) and its trust store.
    if let Ok(tls) = native_tls::TlsConnector::new() {
        builder = builder.tls_connector(std::sync::Arc::new(tls));
    }
    builder
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("pillkit-host/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Release builds only talk HTTPS (plain HTTP is allowed for localhost while developing).
pub fn server_allowed(server: &str) -> bool {
    let s = server.to_ascii_lowercase();
    s.starts_with("https://")
        || s.starts_with("http://localhost")
        || s.starts_with("http://127.0.0.1")
        || (cfg!(feature = "dev-keys") && s.starts_with("http://"))
}

fn envelope(resp: Result<ureq::Response, ureq::Error>) -> Result<String, String> {
    match resp {
        Ok(r) => {
            let v: Value = r.into_json().map_err(|e| format!("bad response: {e}"))?;
            v.get("token")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| "response has no token".into())
        }
        Err(ureq::Error::Status(code, r)) => {
            let body: Value = r.into_json().unwrap_or(Value::Null);
            Err(format!(
                "{code} {}",
                body.get("error").and_then(Value::as_str).unwrap_or("error")
            ))
        }
        Err(e) => Err(format!("network: {e}")),
    }
}

pub fn fetch_license(
    server: &str,
    app_id: &str,
    key: &str,
    bundle_id: &str,
) -> Result<String, String> {
    let body = json!({"app_id": app_id, "bundle_id": bundle_id, "platform": std::env::consts::OS});
    envelope(
        agent()
            .post(&format!("{}/v1/license", server.trim_end_matches('/')))
            .set("Authorization", &format!("Bearer {key}"))
            .send_json(body),
    )
}

pub fn fetch_feed(server: &str, app_id: &str, key: &str) -> Result<String, String> {
    envelope(
        agent()
            .get(&format!("{}/v1/feed", server.trim_end_matches('/')))
            .query("app_id", app_id)
            .set("Authorization", &format!("Bearer {key}"))
            .call(),
    )
}

pub fn post_events(server: &str, app_id: &str, key: &str, events: &[Value]) -> Result<(), String> {
    agent()
        .post(&format!("{}/v1/events", server.trim_end_matches('/')))
        .set("Authorization", &format!("Bearer {key}"))
        .send_json(json!({"app_id": app_id, "events": events}))
        .map(|_| ())
        .map_err(|e| e.to_string())
}
