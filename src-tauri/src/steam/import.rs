use base64::Engine;
use clipboard::ClipboardProvider;

use super::vdf::validate_vdf_value;

pub fn read_clipboard() -> Result<String, String> {
    let mut clipboard = clipboard::ClipboardContext::new()
        .map_err(|_| "Clipboard is not available. Copy the account payload and try again.")?;
    clipboard
        .get_contents()
        .map_err(|_| "Failed to read clipboard. Copy the payload again and retry.".to_string())
}

pub fn write_clipboard(content: &str) -> Result<(), String> {
    let mut clipboard = clipboard::ClipboardContext::new()
        .map_err(|_| "Clipboard is not available.".to_string())?;
    clipboard
        .set_contents(content.to_string())
        .map_err(|_| "Failed to write clipboard.".to_string())
}

pub(crate) fn split_batch_payloads(content: &str) -> Vec<String> {
    let cleaned = sanitize_clipboard_input(content);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return vec![];
    }

    let by_paragraph: Vec<String> = trimmed
        .split("\n\n")
        .map(str::trim)
        .filter(|chunk| !chunk.is_empty())
        .map(str::to_string)
        .collect();
    if by_paragraph.len() > 1 {
        return by_paragraph;
    }

    let line_entries: Vec<String> = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            // Any username/steamid----token(----meta) line is a batch entry.
            line.contains("----") && line.split("----").count() >= 2
        })
        .map(str::to_string)
        .collect();
    if line_entries.len() > 1 {
        return line_entries;
    }

    vec![trimmed.to_string()]
}

/// Parsed import line: username + JWT only (metadata stripped) + optional key:value map.
#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedTokenLine {
    pub username: String,
    pub token: String,
    /// Trailing `key:value` segments from export lines (accepted, unused).
    #[allow(dead_code)]
    pub meta: serde_json::Map<String, serde_json::Value>,
}

#[allow(dead_code)]
pub(crate) fn parse_clipboard(input: &str) -> Result<(String, String), String> {
    let parsed = parse_token_line(input)?;
    Ok((parsed.username, parsed.token))
}

/// Parse `username----token` or `username----token----key:value…` (and token-only).
pub(crate) fn parse_token_line(input: &str) -> Result<ParsedTokenLine, String> {
    let cleaned = sanitize_clipboard_input(input);
    let trimmed = cleaned.trim();
    let collapsed = collapse_import_payload(trimmed);
    if collapsed.is_empty() {
        return Err("Clipboard is empty.".to_string());
    }

    // Preferred path: first ---- segment = user, second = JWT, rest = metadata.
    if let Some(parsed) = parse_segmented_line(&collapsed) {
        validate_vdf_value("Username", &parsed.username)?;
        return Ok(parsed);
    }

    if let Some((username_raw, token_raw)) = split_username_and_token(&collapsed) {
        let username = clean_clipboard_field(username_raw);
        let token = extract_jwt_token(token_raw).ok_or_else(|| {
            "Token does not look like a valid JWT. Copy the full code again.".to_string()
        })?;
        validate_vdf_value("Username", &username)?;
        return Ok(ParsedTokenLine {
            username,
            token,
            meta: serde_json::Map::new(),
        });
    }

    if let Some(token) = extract_jwt_token(&collapsed) {
        let steamid = extract_steamid_from_jwt(&token)?;
        let username = username_for_jwt(&token, &steamid)?;
        return Ok(ParsedTokenLine {
            username,
            token,
            meta: serde_json::Map::new(),
        });
    }

    Err(
        "Unrecognized format. Use steamid----token, username----token, or paste the JWT on its own."
            .to_string(),
    )
}

/// `user----jwt----csgoRank:40----…` → username, JWT only, meta map.
fn parse_segmented_line(input: &str) -> Option<ParsedTokenLine> {
    if !input.contains("----") {
        return None;
    }
    let segments: Vec<&str> = input
        .split("----")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if segments.len() < 2 {
        return None;
    }

    // Find JWT segment (usually index 1)
    let mut token_idx: Option<usize> = None;
    let mut token: Option<String> = None;
    for (i, seg) in segments.iter().enumerate() {
        if let Some(jwt) = extract_jwt_token(seg) {
            token_idx = Some(i);
            token = Some(jwt);
            break;
        }
    }
    let token = token?;
    let token_idx = token_idx?;

    let username = if token_idx > 0 {
        let left = segments[0];
        clean_clipboard_field(left)
    } else {
        let steamid = extract_steamid_from_jwt(&token).ok()?;
        username_for_jwt(&token, &steamid).ok()?
    };

    let mut meta = serde_json::Map::new();
    for seg in segments.iter().skip(token_idx + 1) {
        if let Some((k, v)) = seg.split_once(':') {
            let key = k.trim();
            if !key.is_empty() {
                meta.insert(
                    key.to_string(),
                    serde_json::Value::String(v.trim().to_string()),
                );
            }
        }
    }

    Some(ParsedTokenLine {
        username,
        token,
        meta,
    })
}

pub(crate) fn extract_steamid_from_jwt(jwt: &str) -> Result<String, String> {
    let payload = decode_jwt_payload(jwt)?;
    let json: serde_json::Value =
        serde_json::from_slice(&payload).map_err(|_| "Token payload is not valid JSON.")?;
    let steamid = json
        .get("sub")
        .and_then(|v| v.as_str())
        .ok_or("Token is missing the SteamID field.")?;
    if steamid.parse::<u64>().is_err() {
        return Err("Token SteamID is not numeric.".to_string());
    }
    Ok(steamid.to_string())
}

fn sanitize_clipboard_input(input: &str) -> String {
    input
        .replace(['\u{feff}', '\u{200b}'], "")
        .replace('\u{00a0}', " ")
}

fn collapse_import_payload(input: &str) -> String {
    input
        .chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .collect()
}

fn split_username_and_token(input: &str) -> Option<(&str, &str)> {
    if input.contains("----") || input.contains("eyJ") || input.contains("eyA") || input.contains("ey ")
    {
        return None;
    }
    for sep in ["---", "::", ":", "|"] {
        if let Some(pos) = input.find(sep) {
            let (left, right) = input.split_at(pos);
            let right = &right[sep.len()..];
            if !left.trim().is_empty() && !right.trim().is_empty() {
                return Some((left.trim(), right.trim()));
            }
        }
    }

    let mut lines = input.lines().map(str::trim).filter(|line| !line.is_empty());
    let first = lines.next()?;
    let second = lines.next()?;
    if lines.next().is_some() {
        return None;
    }
    if extract_jwt_token(first).is_some() {
        return None;
    }
    Some((first, second))
}

fn is_steamid(value: &str) -> bool {
    let value = value.trim();
    (15..=20).contains(&value.len()) && value.chars().all(|c| c.is_ascii_digit())
}

fn extract_jwt_token(input: &str) -> Option<String> {
    let cleaned = sanitize_clipboard_input(input);
    let collapsed = collapse_import_payload(cleaned.trim());

    if let Some(pos) = collapsed.find("----") {
        let left = collapsed[..pos].trim();
        let right = collapsed[pos + 4..].trim();
        if is_steamid(left) {
            if let Some(jwt) = extract_jwt_token(right) {
                return Some(jwt);
            }
        }
    }

    let whole = normalize_clipboard_token(&collapsed);
    if looks_like_jwt(&whole) {
        return Some(whole);
    }

    if let Some(pos) = collapsed.find("----") {
        let head = normalize_clipboard_token(collapsed.get(..pos)?);
        if looks_like_jwt(&head) {
            return Some(head);
        }
    }

    let compact = normalize_clipboard_token(&collapsed);
    for (start, _) in compact.match_indices("ey") {
        if let Some(jwt) = slice_three_jwt_parts(&compact[start..]) {
            if looks_like_jwt(&jwt) {
                return Some(jwt);
            }
        }
    }
    None
}

fn slice_three_jwt_parts(s: &str) -> Option<String> {
    let first_dot = s.find('.')?;
    let rest = &s[first_dot + 1..];
    let second_dot = rest.find('.')? + first_dot + 1;
    let third = &s[second_dot + 1..];
    let sig_len = third.find(|c: char| !is_jwt_char(c)).unwrap_or(third.len());
    let jwt = format!(
        "{}.{}.{}",
        &s[..first_dot],
        &s[first_dot + 1..second_dot],
        &third[..sig_len]
    );
    if looks_like_jwt(&jwt) {
        Some(jwt)
    } else {
        None
    }
}

fn is_jwt_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '=')
}

fn looks_like_jwt(token: &str) -> bool {
    let token = normalize_clipboard_token(token);
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    if !parts[0].starts_with("ey") {
        return false;
    }
    parts
        .iter()
        .all(|part| !part.is_empty() && part.chars().all(is_jwt_char))
}

fn username_for_jwt(jwt: &str, steamid: &str) -> Result<String, String> {
    let payload = decode_jwt_payload(jwt)?;
    let json: serde_json::Value =
        serde_json::from_slice(&payload).map_err(|_| "Token payload is not valid JSON.")?;

    for key in ["preferred_username", "name", "unique_name", "sub"] {
        if let Some(value) = json.get(key).and_then(|v| v.as_str()) {
            let candidate = clean_clipboard_field(value);
            if key == "sub" && candidate == steamid {
                continue;
            }
            if validate_vdf_value("Username", &candidate).is_ok() {
                return Ok(candidate);
            }
        }
    }

    Ok(format!("user{}", &steamid[steamid.len().saturating_sub(6)..]))
}

fn decode_jwt_payload(jwt: &str) -> Result<Vec<u8>, String> {
    let jwt = normalize_clipboard_token(jwt);
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err("Invalid token format.".into());
    }
    use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD
        .decode(parts[1])
        .or_else(|_| URL_SAFE.decode(parts[1]))
        .or_else(|_| STANDARD.decode(parts[1]))
        .map_err(|_| "Token payload could not be decoded.".to_string())
}

fn clean_clipboard_field(value: &str) -> String {
    value
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .trim()
        .to_string()
}

fn normalize_clipboard_token(value: &str) -> String {
    let cleaned = clean_clipboard_field(value);
    let mut token = String::new();
    for part in cleaned.split_whitespace() {
        token.push_str(part);
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACED_JWT: &str = "ey AidHlwIjogIkpXVCIsICJhbGciOiAiRWREU0EiIH0.eyAiaXNzIjogInN0ZWFtIiwgInN1YiI6ICI3NjU2MTE5OTg0MzA4MTgyNSIsICJhdWQiOiBbICJjbGllbnQiLCAid2ViIiwgInJlbmV3IiwgImRlcml2ZSIgXSwgImV4cCI6IDE3ODk1MjI5MzEsICJuYmYiOiAxNzQ5NzY1OTAxLCAiaWF0IjogMTc1ODQwNTkwMSwgImp0aSI6ICIwMDBEXzI2RjM4QThFXzIyMjJEIiwgIm9hdCI6IDE3NTg0MDU5MDEsICJnZW4iOiAxLCAicGVyIjogMSwgImlwX3N1YmplY3QiOiAiMTg5LjIwMy44NS4xMTAiLCAiaXBfY29uZmlybWVyIjogIjE4OS4yMDMuODUuMTEwIiB9.WNDsKm4FmiWFdRXeSNYFvsiU5a3khkLojs4x76lTZmYm7abrXKPAOfGCZnQ2LISKPIm6CD6SNMdsJL0yo_A9BQ";

    #[test]
    fn split_batch_by_lines() {
        let a = format!("76561199863008391----{SPACED_JWT}");
        let b = format!("76561199194545602----{SPACED_JWT}");
        let input = format!("{a}\n{b}");
        let parts = split_batch_payloads(&input);
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn split_single_stays_one() {
        let parts = split_batch_payloads(SPACED_JWT);
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn jwt_only_with_spaces() {
        let token = extract_jwt_token(SPACED_JWT).expect("jwt");
        assert!(looks_like_jwt(&token));
        assert_eq!(extract_steamid_from_jwt(&token).unwrap(), "76561199843081825");
        let (user, _) = parse_clipboard(SPACED_JWT).unwrap();
        assert!(!user.is_empty());
    }

    #[test]
    fn steamid_then_jwt() {
        let input = format!("76561199843081825----{SPACED_JWT}");
        let (user, token) = parse_clipboard(&input).unwrap();
        assert_eq!(user, "76561199843081825");
        assert!(looks_like_jwt(&token));
    }

    #[test]
    fn user_steamid_spaced_jwt_with_line_break() {
        let input = "76561199194545602----eyAidHlwIjog\nIkpXVCIsICJhbGciOiAiRWREU0EiIH0.eyAiaXNzIjogInN0ZWFtIiwgInN1YiI6ICI3NjU2MTE5OTE5NDU0NTYwMiIsICJhdWQiOiBbICJjbGllbnQiLCAid2ViIiwgInJlbmV3IiwgImRlcml2ZSIgXSwgImV4cCI6IDE3ODcxNjA0MTgsICJuYmYiOiAxNzYwMzYxNTIwLCAiaWF0IjogMTc2OTAwMTUyMCwgImp0aSI6ICIwMDAyXzI3OTlCMTAyXzZENUY3IiwgIm9hdCI6IDE3NjkwMDE1MjAsICJwZXIiOiAxLCAiaXBfc3ViamVjdCI6ICI4MC4xMTUuMjQ2LjY5IiwgImlwX2NvbmZpcm1lciI6ICI4MC4xMTUuMjQ2LjY5IiB9.ZL0-Z6NmDNC7LqHUE5YUNTph-Ee9Fo1MON3Alw2OLLVA_H_s8yOtugnWDgnHrApc8hTyVYGSn8fIBxh8yMZOCQ";
        parse_clipboard(input).expect("parse with newline");
    }

    #[test]
    fn user_steamid_spaced_jwt() {
        let input = "76561199194545602----eyAidHlwIjogIkpXVCIsICJhbGciOiAiRWREU0EiIH0.eyAiaXNzIjogInN0ZWFtIiwgInN1YiI6ICI3NjU2MTE5OTE5NDU0NTYwMiIsICJhdWQiOiBbICJjbGllbnQiLCAid2ViIiwgInJlbmV3IiwgImRlcml2ZSIgXSwgImV4cCI6IDE3ODcxNjA0MTgsICJuYmYiOiAxNzYwMzYxNTIwLCAiaWF0IjogMTc2OTAwMTUyMCwgImp0aSI6ICIwMDAyXzI3OTlCMTAyXzZENUY3IiwgIm9hdCI6IDE3NjkwMDE1MjAsICJwZXIiOiAxLCAiaXBfc3ViamVjdCI6ICI4MC4xMTUuMjQ2LjY5IiwgImlwX2NvbmZpcm1lciI6ICI4MC4xMTUuMjQ2LjY5IiB9.ZL0-Z6NmDNC7LqHUE5YUNTph-Ee9Fo1MON3Alw2OLLVA_H_s8yOtugnWDgnHrApc8hTyVYGSn8fIBxh8yMZOCQ";
        let (user, token) = parse_clipboard(input).expect("parse");
        assert_eq!(user, "76561199194545602");
        assert_eq!(extract_steamid_from_jwt(&token).unwrap(), "76561199194545602");
    }

    #[test]
    fn steamid_jwt_then_metadata() {
        let input = format!(
            "76561199863008391----{SPACED_JWT}----csgoRank:13----earnedServiceMedal:No----vacStatus:Clean"
        );
        let (user, token) = parse_clipboard(&input).unwrap();
        assert_eq!(user, "76561199863008391");
        assert!(looks_like_jwt(&token));
        assert!(!token.contains("csgoRank"));
        assert_eq!(extract_steamid_from_jwt(&token).unwrap(), "76561199843081825");
        let parsed = parse_token_line(&input).unwrap();
        assert_eq!(
            parsed.meta.get("csgoRank").and_then(|v| v.as_str()),
            Some("13")
        );
    }

    #[test]
    fn username_jwt_then_metadata() {
        let input = format!(
            "jake----{SPACED_JWT}----csgoRank:40----earnedServiceMedal:No----vacStatus:Clean----primeStatus:Yes----inventoryValue:0----cooldown:false----rating:0----medals:0----hasRareItem:0----lastchecked:2026-05-22T02:36:20.656Z"
        );
        let parsed = parse_token_line(&input).unwrap();
        assert_eq!(parsed.username, "jake");
        assert!(looks_like_jwt(&parsed.token));
        assert!(!parsed.token.contains("csgoRank"));
        assert_eq!(
            parsed.meta.get("primeStatus").and_then(|v| v.as_str()),
            Some("Yes")
        );
        assert_eq!(
            parsed
                .meta
                .get("lastchecked")
                .and_then(|v| v.as_str()),
            Some("2026-05-22T02:36:20.656Z")
        );
    }
}
