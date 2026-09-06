//! sitnn.dog HTTP client (Rust only — never from the webview).

use crate::config::{self, AppConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const BASE: &str = "https://sitnn.dog";
const TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiOutcome {
    /// Parsed JSON body when the server responded with JSON.
    pub body: Option<Value>,
    pub http_status: Option<u16>,
    /// True when network/DNS/timeout/non-JSON failure occurred.
    pub site_down: bool,
    pub error: Option<String>,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())
}

fn apply_app_key(
    mut req: reqwest::RequestBuilder,
    cfg: &AppConfig,
) -> reqwest::RequestBuilder {
    let key = cfg.app_key.trim();
    if !key.is_empty() {
        req = req.header("X-App-Key", key);
    }
    req
}

async fn parse_response(res: reqwest::Response) -> ApiOutcome {
    let http_status = Some(res.status().as_u16());
    let text = match res.text().await {
        Ok(t) => t,
        Err(_) => {
            return ApiOutcome {
                body: None,
                http_status,
                site_down: true,
                error: Some("Site not ready".into()),
            };
        }
    };

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return ApiOutcome {
            body: None,
            http_status,
            site_down: true,
            error: Some("Site not ready".into()),
        };
    }

    // HTML error pages (site down / hosting 404 page)
    if trimmed.starts_with('<') {
        return ApiOutcome {
            body: None,
            http_status,
            site_down: true,
            error: Some("Site not ready".into()),
        };
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(body) => ApiOutcome {
            body: Some(body),
            http_status,
            site_down: false,
            error: None,
        },
        Err(_) => ApiOutcome {
            body: None,
            http_status,
            site_down: true,
            error: Some("Site not ready".into()),
        },
    }
}

fn network_fail(err: reqwest::Error) -> ApiOutcome {
    let _ = err; // never log secrets; message may contain URL only
    ApiOutcome {
        body: None,
        http_status: None,
        site_down: true,
        error: Some("Site not ready".into()),
    }
}

pub async fn get_key_status(key: String) -> ApiOutcome {
    let key = key.trim().to_string();
    if key.is_empty() {
        return ApiOutcome {
            body: Some(serde_json::json!({
                "ok": false,
                "error": "Invalid key"
            })),
            http_status: Some(404),
            site_down: false,
            error: None,
        };
    }

    let cfg = config::load();
    let mut next = cfg.clone();
    next.last_key = key.clone();
    let _ = config::save(&next);
    let _ = config::mask_key(&key); // ensure helper is used (no full-key logs)

    let client = match client() {
        Ok(c) => c,
        Err(_) => {
            return ApiOutcome {
                body: None,
                http_status: None,
                site_down: true,
                error: Some("Site not ready".into()),
            };
        }
    };

    let url = format!(
        "{BASE}/api/status?key={}",
        urlencoding_encode(&key)
    );

    let req = apply_app_key(client.get(&url), &cfg);
    match req.send().await {
        Ok(res) => parse_response(res).await,
        Err(e) => network_fail(e),
    }
}

pub async fn redeem_key(key: String) -> ApiOutcome {
    let key = key.trim().to_string();
    if key.is_empty() {
        return ApiOutcome {
            body: Some(serde_json::json!({
                "ok": false,
                "error": "Invalid key"
            })),
            http_status: Some(404),
            site_down: false,
            error: None,
        };
    }

    let cfg = config::load();
    let mut next = cfg.clone();
    next.last_key = key.clone();
    let _ = config::save(&next);

    let client = match client() {
        Ok(c) => c,
        Err(_) => {
            return ApiOutcome {
                body: None,
                http_status: None,
                site_down: true,
                error: Some("Site not ready".into()),
            };
        }
    };

    let req = apply_app_key(client.post(format!("{BASE}/api/redeem")), &cfg)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "key": key }));

    match req.send().await {
        Ok(res) => parse_response(res).await,
        Err(e) => network_fail(e),
    }
}

pub async fn request_replacement(key: String, reason: String) -> ApiOutcome {
    let key = key.trim().to_string();
    let reason = reason.trim().to_string();

    if key.is_empty() {
        return ApiOutcome {
            body: Some(serde_json::json!({
                "ok": false,
                "error": "Invalid key"
            })),
            http_status: Some(404),
            site_down: false,
            error: None,
        };
    }
    if reason.is_empty() {
        return ApiOutcome {
            body: Some(serde_json::json!({
                "ok": false,
                "error": "Reason is required"
            })),
            http_status: Some(400),
            site_down: false,
            error: None,
        };
    }
    if reason.chars().count() > 500 {
        return ApiOutcome {
            body: Some(serde_json::json!({
                "ok": false,
                "error": "Reason must be 500 characters or fewer"
            })),
            http_status: Some(400),
            site_down: false,
            error: None,
        };
    }

    let cfg = config::load();
    let mut next = cfg.clone();
    next.last_key = key.clone();
    let _ = config::save(&next);

    let client = match client() {
        Ok(c) => c,
        Err(_) => {
            return ApiOutcome {
                body: None,
                http_status: None,
                site_down: true,
                error: Some("Site not ready".into()),
            };
        }
    };

    let req = apply_app_key(client.post(format!("{BASE}/api/replacement")), &cfg)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "key": key, "reason": reason }));

    match req.send().await {
        Ok(res) => parse_response(res).await,
        Err(e) => network_fail(e),
    }
}

/// Minimal percent-encoding for query values (keys are alphanumeric-ish).
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn get_last_key() -> String {
    config::load().last_key
}

pub fn save_last_key(key: String) -> Result<(), String> {
    let mut cfg = config::load();
    cfg.last_key = key.trim().to_string();
    config::save(&cfg)
}
