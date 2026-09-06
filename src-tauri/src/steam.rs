mod account;
mod config;
mod crypto;
mod import;
mod paths;
mod process;
mod tokens;
mod vdf;

pub use account::{load_steam_accounts, SteamAccount};
pub use import::{read_clipboard, write_clipboard};

use std::path::Path;
use std::time::Duration;

/// Format `username----jwt` for Import paste.
fn export_line(account: &SteamAccount, token: &str) -> Option<String> {
    let user = if !account.account_name.trim().is_empty() {
        account.account_name.trim()
    } else {
        account.steamid.trim()
    };
    if user.is_empty() || token.trim().is_empty() {
        return None;
    }
    Some(format!("{}----{}", user, token.trim()))
}

/// Copy one History account's token line to the clipboard (saved JWT or ConnectCache).
pub fn export_account_token(steamid: &str) -> Result<String, String> {
    let accounts = load_steam_accounts()?;
    let account = accounts
        .into_iter()
        .find(|a| a.steamid == steamid)
        .ok_or_else(|| "Account not found.".to_string())?;

    let Some(token) = resolve_login_token(&account) else {
        return Err("No token available for this account.".into());
    };
    let line = export_line(&account, &token)
        .ok_or_else(|| "No token available for this account.".to_string())?;

    write_clipboard(&line)?;
    Ok("Copied 1 account".to_string())
}

/// Copy all exportable History accounts (`username----jwt`, one per line).
pub fn export_all_account_tokens() -> Result<String, String> {
    let accounts = load_steam_accounts()?;
    let mut lines = Vec::new();

    for account in &accounts {
        if let Some(token) = resolve_login_token(account) {
            if let Some(line) = export_line(account, &token) {
                lines.push(line);
            }
        }
    }

    if lines.is_empty() {
        return Ok("Nothing to export".to_string());
    }

    let n = lines.len();
    write_clipboard(&lines.join("\n"))?;
    Ok(format!(
        "Copied {n} account{}",
        if n == 1 { "" } else { "s" }
    ))
}

/// Resolve a login JWT: saved store first, else Steam ConnectCache.
pub fn resolve_login_token(account: &SteamAccount) -> Option<String> {
    if let Some(token) = account
        .token
        .as_ref()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
    {
        return Some(token.to_string());
    }
    if account.account_name.trim().is_empty() {
        return None;
    }
    config::read_connect_cache_token(&account.account_name)
}

/// Close Steam without relaunching.
pub fn stop_steam_only() -> Result<(), String> {
    process::stop_steam()
}

/// Wipe History + Steam login cache: stop Steam, clear ConnectCache, empty
/// loginusers, clear AutoLoginUser, and delete our saved tokens.
pub fn handle_clear_all() -> Result<String, String> {
    process::stop_steam()?;

    if let Ok(base_path) = paths::local_steam_cache_path() {
        let _ = config::clear_login_cache(&base_path);
    }

    if let Ok(steam_path_string) = paths::get_steam_path() {
        let steam_path = Path::new(&steam_path_string);
        let loginusers = steam_path.join("config").join("loginusers.vdf");
        let _ = std::fs::write(&loginusers, "\"users\"\n{\n}\n");
        let _ = process::write_autologin_user("");
    }

    tokens::clear_all_records();
    crate::config::clear_last_token();

    Ok("Cleared all accounts and Steam login cache.".to_string())
}

pub fn import_from_clipboard() -> Result<String, String> {
    let content = read_clipboard()?;
    handle_batch_import(&content)
}

pub fn handle_batch_import(content: &str) -> Result<String, String> {
    let entries = import::split_batch_payloads(content);
    if entries.is_empty() {
        return Err("Clipboard is empty.".to_string());
    }
    if entries.len() == 1 {
        return import_single_account(&entries[0]);
    }

    let steam_path_string = paths::get_steam_path()?;
    let steam_path = Path::new(&steam_path_string);
    config::check_steam_config_files(&steam_path.join("config"))?;
    process::stop_steam()?;

    let mut imported = 0usize;
    let mut errors = Vec::new();
    let mut last: Option<(String, String)> = None;

    for (idx, entry) in entries.iter().enumerate() {
        match import_account_files(entry, steam_path) {
            Ok((username, steamid)) => {
                imported += 1;
                last = Some((username, steamid));
            }
            Err(e) => errors.push(format!("#{}: {}", idx + 1, e)),
        }
    }

    if imported == 0 {
        return Err(errors.join(" | "));
    }

    if let Some((username, steamid)) = last {
        apply_active_account(&username, &steamid, steam_path)?;
        relaunch_steam(&steam_path_string)?;
    }

    let mut msg = format!("Imported {imported} accounts. Starting Steam.");
    if !errors.is_empty() {
        msg.push_str(&format!(" {} failed.", errors.len()));
    }
    Ok(msg)
}

fn import_account_files(entry: &str, steam_path: &Path) -> Result<(String, String), String> {
    let parsed = import::parse_token_line(entry)?;
    let steamid = import::extract_steamid_from_jwt(&parsed.token)?;
    config::write_account_files(&parsed.username, &parsed.token, &steamid, steam_path)?;
    tokens::save_record(&steamid, &parsed.username, &parsed.username, &parsed.token);
    Ok((parsed.username, steamid))
}

fn import_single_account(content: &str) -> Result<String, String> {
    let parsed = import::parse_token_line(content)?;
    let steamid = import::extract_steamid_from_jwt(&parsed.token)?;

    let steam_path_string = paths::get_steam_path()?;
    let steam_path = Path::new(&steam_path_string);
    config::check_steam_config_files(&steam_path.join("config"))?;
    process::stop_steam()?;

    config::write_account_files(&parsed.username, &parsed.token, &steamid, steam_path)?;
    tokens::save_record(&steamid, &parsed.username, &parsed.username, &parsed.token);
    apply_active_account(&parsed.username, &steamid, steam_path)?;
    relaunch_steam(&steam_path_string)?;

    Ok(format!("Imported {}. Starting Steam.", parsed.username))
}

pub fn handle_login_account(account: &SteamAccount) -> Result<String, String> {
    let steam_path_string = paths::get_steam_path()?;
    let steam_path = Path::new(&steam_path_string);

    process::stop_steam()?;

    // Re-provision the ConnectCache token + config.vdf from our stored copy on
    // every sign-in, so switching works even if Steam's cache was cleared since
    // import. Accounts imported before token persistence fall back to the flip.
    if let Some(jwt) = &account.token {
        config::check_steam_config_files(&steam_path.join("config"))?;
        config::write_account_files(&account.account_name, jwt, &account.steamid, steam_path)?;
    }

    apply_active_account(&account.account_name, &account.steamid, steam_path)?;
    relaunch_steam(&steam_path_string)?;

    Ok(format!(
        "Signed in as {}. Starting Steam.",
        account.display_name()
    ))
}

pub fn handle_delete_account(account: &SteamAccount) -> Result<String, String> {
    let steam_path_string = paths::get_steam_path()?;
    let steam_path = Path::new(&steam_path_string);
    process::stop_steam()?;

    config::remove_loginuser(
        &steam_path.join("config").join("loginusers.vdf"),
        &account.steamid,
    )?;

    let config_vdf = steam_path.join("config").join("config.vdf");
    if config_vdf.exists() {
        config::remove_config_account(&config_vdf, &account.steamid)?;
    }

    if let Ok(local_dir) = paths::local_steam_cache_path() {
        let local_vdf = local_dir.join("local.vdf");
        if local_vdf.exists() {
            let crc = crypto::compute_crc32(&account.account_name);
            if let Ok(content) = std::fs::read_to_string(&local_vdf) {
                let updated = config::remove_connect_cache_entry(&content, &crc);
                let _ = std::fs::write(&local_vdf, updated);
            }
        }
    }

    process::clear_autologin_if_matches(&account.account_name);
    tokens::remove_record(&account.steamid);

    Ok(format!("Removed {}.", account.display_name()))
}

fn apply_active_account(username: &str, steamid: &str, steam_path: &Path) -> Result<(), String> {
    let loginusers_vdf = steam_path.join("config").join("loginusers.vdf");
    config::update_loginusers_vdf(&loginusers_vdf, username, steamid)?;
    // Silent default: Invisible persona in localconfig before Steam starts.
    config::apply_localconfig_invisible(steamid, steam_path)?;
    process::write_autologin_user(username)
}

fn relaunch_steam(steam_path: &str) -> Result<(), String> {
    std::thread::sleep(Duration::from_millis(400));
    process::launch_steam(steam_path)?;
    Ok(())
}
