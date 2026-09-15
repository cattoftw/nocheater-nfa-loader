//! sitnn.dog HTTP client (Rust only — never from the webview).

use crate::config::{self, AppConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const BASE: &str = "https://sitnn.dog";
/// Status / short calls.
const TIMEOUT_SECS: u64 = 20;
/// Redeem may wait on sitnn.dog's provider poll (server maxDuration ≈ 60s).
const REDEEM_TIMEOUT_SECS: u64 = 65;
/// Replacement: resolve + warranty check/claim + status poll.
const REPLACEMENT_TIMEOUT_SECS: u64 = 90;

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

fn client_with_timeout(secs: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(secs))
        .build()
        .map_err(|e| e.to_string())
}

fn client() -> Result<reqwest::Client, String> {
    client_with_timeout(TIMEOUT_SECS)
}

fn site_down_outcome() -> ApiOutcome {
    ApiOutcome {
        body: None,
        http_status: None,
        site_down: true,
        error: Some("Site not ready".into()),
    }
}

fn json_err(status: u16, error: &str) -> ApiOutcome {
    ApiOutcome {
        body: Some(serde_json::json!({
            "ok": false,
            "error": error
        })),
        http_status: Some(status),
        site_down: false,
        error: None,
    }
}

fn json_ok(body: Value) -> ApiOutcome {
    ApiOutcome {
        body: Some(body),
        http_status: Some(200),
        site_down: false,
        error: None,
    }
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
    site_down_outcome()
}

fn body_error(body: &Value) -> Option<&str> {
    body.get("error")
        .and_then(|v| v.as_str())
        .or_else(|| body.get("message").and_then(|v| v.as_str()))
}

fn body_ok(body: &Value) -> bool {
    body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
}

fn body_success(body: &Value) -> bool {
    body.get("success").and_then(|v| v.as_bool()).unwrap_or(false)
}

fn body_account(body: &Value) -> Option<&str> {
    body.get("account")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn body_str<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn friendly_api_error(body: &Value) -> String {
    let code = body_str(body, "error_code")
        .unwrap_or("")
        .to_ascii_uppercase();
    let fallback = body_error(body).unwrap_or("Request failed");
    match code.as_str() {
        "NOT_CONFIGURED" | "UNAUTHORIZED" => {
            "Service is temporarily unavailable. Please try again later.".into()
        }
        "NOT_REDEEMED" => {
            "This license key hasn't been redeemed yet. Redeem it first, then try again.".into()
        }
        "INSUFFICIENT_BALANCE" => {
            "Stock delivery is temporarily unavailable. Please try again shortly.".into()
        }
        "OUT_OF_STOCK" => "This product is currently out of stock. Please try again later.".into(),
        "EXPIRED" => "The warranty window for this account has expired.".into(),
        "NOT_ELIGIBLE" => "This account is not eligible for a warranty replacement.".into(),
        "NOT_COVERED" => {
            "This account type is not covered by warranty (e.g. valued inventory).".into()
        }
        "INVALID_FORMAT" => "Invalid format. Please double-check and try again.".into(),
        "INVALID_KEY" => {
            "We could not locate this key or account. Please double-check the credentials.".into()
        }
        "INVALID_LICENSE" => "Invalid license key. Please double-check and try again.".into(),
        "NOT_FOUND" => {
            "No claim was found for this license. Redeem it first, then try again.".into()
        }
        "NO_CLAIM" => "No claim was found for this request.".into(),
        "RATE_LIMITED" => "Too many requests. Please wait a moment and try again.".into(),
        _ => fallback.to_string(),
    }
}

async fn post_json(
    client: &reqwest::Client,
    cfg: &AppConfig,
    path: &str,
    body: Value,
) -> ApiOutcome {
    let req = apply_app_key(client.post(format!("{BASE}{path}")), cfg)
        .header("Content-Type", "application/json")
        .json(&body);
    match req.send().await {
        Ok(res) => parse_response(res).await,
        Err(e) => network_fail(e),
    }
}

/// Map sitnn.dog `/api/resolve-license` ({ success, accountToken }) into app `{ ok, account }`.
async fn resolve_delivered_account(
    client: &reqwest::Client,
    cfg: &AppConfig,
    key: &str,
) -> Option<ApiOutcome> {
    let outcome = post_json(
        client,
        cfg,
        "/api/resolve-license",
        serde_json::json!({ "license": key }),
    )
    .await;

    if outcome.site_down {
        return None;
    }

    let body = outcome.body.as_ref()?;
    if !body_success(body) {
        return None;
    }
    let account = body_str(body, "accountToken")?;

    let message = body_str(body, "message").unwrap_or("Account loaded");
    let redeemed_at = body.get("redeemed_at").cloned().unwrap_or(Value::Null);

    Some(json_ok(serde_json::json!({
        "ok": true,
        "status": "redeemed",
        "product": "Account",
        "message": message,
        "account": account,
        "redeemed_at": redeemed_at,
    })))
}

fn wants_resolve_fallback(outcome: &ApiOutcome) -> bool {
    let Some(body) = outcome.body.as_ref() else {
        return false;
    };
    if body_account(body).is_some() {
        return false;
    }
    let err = body_error(body).unwrap_or("");
    if err.eq_ignore_ascii_case("Already redeemed") {
        return true;
    }
    // Redeem reported success but delivery payload missing (poll timed out server-side).
    body_ok(body)
}

fn poll_ms(body: &Value) -> u64 {
    body.get("poll_ms")
        .and_then(|v| v.as_u64())
        .filter(|&n| n > 0)
        .unwrap_or(2000)
        .min(15_000)
}

async fn poll_warranty_status(
    client: &reqwest::Client,
    cfg: &AppConfig,
    license_key: &str,
) -> ApiOutcome {
    // Bound attempts so we stay under REPLACEMENT_TIMEOUT_SECS.
    for _ in 0..8 {
        let outcome = post_json(
            client,
            cfg,
            "/api/warranty-status",
            serde_json::json!({ "license_key": license_key }),
        )
        .await;
        if outcome.site_down {
            return outcome;
        }
        let Some(body) = outcome.body.as_ref() else {
            return json_err(502, "Warranty status failed");
        };
        if !body_success(body) {
            return json_err(outcome.http_status.unwrap_or(400), &friendly_api_error(body));
        }

        let status = body_str(body, "status").unwrap_or("");
        if status.eq_ignore_ascii_case("approved") || status.eq_ignore_ascii_case("rejected") {
            let new_account = body_str(body, "new_account");
            let message = body_str(body, "message").unwrap_or(if status.eq_ignore_ascii_case("approved")
            {
                "Replacement delivered"
            } else {
                "Replacement rejected"
            });
            if status.eq_ignore_ascii_case("approved") {
                return json_ok(serde_json::json!({
                    "ok": true,
                    "status": "approved",
                    "message": message,
                    "account": new_account,
                }));
            }
            return json_err(400, message);
        }

        let wait = poll_ms(body);
        tokio::time::sleep(Duration::from_millis(wait)).await;
    }

    json_err(
        202,
        "Replacement is still processing. Check again shortly or use sitnn.dog.",
    )
}

pub async fn get_key_status(key: String) -> ApiOutcome {
    let key = key.trim().to_string();
    if key.is_empty() {
        return json_err(404, "Invalid key");
    }

    let cfg = config::load();
    let mut next = cfg.clone();
    next.last_key = key.clone();
    let _ = config::save(&next);
    let _ = config::mask_key(&key); // ensure helper is used (no full-key logs)

    let client = match client() {
        Ok(c) => c,
        Err(_) => return site_down_outcome(),
    };

    let url = format!("{BASE}/api/status?key={}", urlencoding_encode(&key));

    let req = apply_app_key(client.get(&url), &cfg);
    match req.send().await {
        Ok(res) => parse_response(res).await,
        Err(e) => network_fail(e),
    }
}

pub async fn redeem_key(key: String) -> ApiOutcome {
    let key = key.trim().to_string();
    if key.is_empty() {
        return json_err(404, "Invalid key");
    }

    let cfg = config::load();
    let mut next = cfg.clone();
    next.last_key = key.clone();
    let _ = config::save(&next);

    let client = match client_with_timeout(REDEEM_TIMEOUT_SECS) {
        Ok(c) => c,
        Err(_) => return site_down_outcome(),
    };

    // Match sitnn.dog site UX: already-delivered licenses load the account instead of 409.
    if let Some(resolved) = resolve_delivered_account(&client, &cfg, &key).await {
        return resolved;
    }

    // App middleman contract: { "key" } — server queues + polls Resync (can take ~60s).
    let outcome = post_json(
        &client,
        &cfg,
        "/api/redeem",
        serde_json::json!({ "key": key }),
    )
    .await;

    if wants_resolve_fallback(&outcome) {
        if let Some(resolved) = resolve_delivered_account(&client, &cfg, &key).await {
            return resolved;
        }
    }

    outcome
}

pub async fn request_replacement(key: String, reason: String) -> ApiOutcome {
    let key = key.trim().to_string();
    let reason = reason.trim().to_string();

    if key.is_empty() {
        return json_err(404, "Invalid key");
    }
    if reason.is_empty() {
        return json_err(400, "Reason is required");
    }
    if reason.chars().count() > 500 {
        return json_err(400, "Reason must be 500 characters or fewer");
    }

    let cfg = config::load();
    let mut next = cfg.clone();
    next.last_key = key.clone();
    let _ = config::save(&next);

    let client = match client_with_timeout(REPLACEMENT_TIMEOUT_SECS) {
        Ok(c) => c,
        Err(_) => return site_down_outcome(),
    };

    // 1) Resolve delivered account (same as sitnn.dog Replacement tab).
    let Some(resolved) = resolve_delivered_account(&client, &cfg, &key).await else {
        return json_err(
            404,
            "This license key hasn't been redeemed yet. Redeem it first, then try again.",
        );
    };
    let token = resolved
        .body
        .as_ref()
        .and_then(body_account)
        .unwrap_or("")
        .to_string();
    if token.is_empty() {
        return json_err(
            404,
            "This license key hasn't been redeemed yet. Redeem it first, then try again.",
        );
    }

    // 2) Warranty eligibility check.
    let check = post_json(
        &client,
        &cfg,
        "/api/warranty-check",
        serde_json::json!({ "license_key": token }),
    )
    .await;
    if check.site_down {
        return check;
    }
    let Some(check_body) = check.body.as_ref() else {
        return json_err(502, "Warranty check failed");
    };
    if !body_success(check_body) {
        return json_err(
            check.http_status.unwrap_or(400),
            &friendly_api_error(check_body),
        );
    }

    let verdict = check_body.get("verdict");
    let action = verdict
        .and_then(|v| body_str(v, "action"))
        .unwrap_or("")
        .to_ascii_uppercase();
    let verdict_reason = verdict
        .and_then(|v| body_str(v, "reason"))
        .unwrap_or("")
        .to_string();
    let eligible = verdict
        .and_then(|v| v.get("eligible_for_warranty"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if action == "IN_QUEUE" {
        return poll_warranty_status(&client, &cfg, &token).await;
    }
    if action == "APPROVED" {
        return json_err(
            400,
            if verdict_reason.is_empty() {
                "A replacement was already issued for this account."
            } else {
                verdict_reason.as_str()
            },
        );
    }
    if action == "EXPIRED" || action == "REJECTED" || !eligible {
        let msg = if !verdict_reason.is_empty() {
            verdict_reason
        } else if action == "EXPIRED" {
            "The warranty window for this account has expired.".into()
        } else {
            "This account is not eligible for a warranty replacement.".into()
        };
        return json_err(400, &msg);
    }

    // 3) File warranty claim (site path — includes Discord notify on success).
    let claim = post_json(
        &client,
        &cfg,
        "/api/warranty-claim",
        serde_json::json!({
            "license_key": token,
            "license": key,
            "reason": reason,
        }),
    )
    .await;
    if claim.site_down {
        return claim;
    }
    let Some(claim_body) = claim.body.as_ref() else {
        return json_err(502, "Replacement request failed");
    };
    if !body_success(claim_body) {
        return json_err(
            claim.http_status.unwrap_or(400),
            &friendly_api_error(claim_body),
        );
    }

    let ticket = claim_body
        .get("claim_id")
        .map(|v| match v {
            Value::Number(n) => format!("R-{n}"),
            Value::String(s) => format!("R-{s}"),
            _ => "R-OPEN".into(),
        })
        .unwrap_or_else(|| "R-OPEN".into());
    let queued_msg = body_str(claim_body, "message").unwrap_or("Replacement queued");

    // 4) Poll until delivered / rejected.
    let polled = poll_warranty_status(&client, &cfg, &token).await;
    if let Some(body) = polled.body.as_ref() {
        if body_ok(body) {
            let mut out = body.clone();
            if let Some(obj) = out.as_object_mut() {
                obj.insert("ticket".into(), Value::String(ticket));
                if !obj.contains_key("message") {
                    obj.insert("message".into(), Value::String(queued_msg.into()));
                }
            }
            return json_ok(out);
        }
        if polled.http_status == Some(202) {
            return json_ok(serde_json::json!({
                "ok": true,
                "ticket": ticket,
                "message": queued_msg,
            }));
        }
    }

    if polled.site_down {
        // Claim already accepted — don't report total failure.
        return json_ok(serde_json::json!({
            "ok": true,
            "ticket": ticket,
            "message": queued_msg,
        }));
    }

    polled
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
