mod api;
mod config;
mod elevation;
mod mutex;
mod steam;
mod tray;

use api::ApiOutcome;
use serde::Serialize;
use tauri::Manager;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    pub steamid: String,
    pub account_name: String,
    pub persona_name: String,
    pub display_name: String,
    pub most_recent: bool,
    pub has_token: bool,
}

fn account_to_dto(a: &steam::SteamAccount) -> AccountDto {
    AccountDto {
        steamid: a.steamid.clone(),
        account_name: a.account_name.clone(),
        persona_name: a.persona_name.clone(),
        display_name: a.display_name().to_string(),
        most_recent: a.most_recent,
        has_token: a.token.as_ref().map(|t| !t.trim().is_empty()).unwrap_or(false)
            || steam::resolve_login_token(a).is_some(),
    }
}

fn find_account(steamid: &str) -> Result<steam::SteamAccount, String> {
    let accounts = steam::load_steam_accounts()?;
    accounts
        .into_iter()
        .find(|a| a.steamid == steamid)
        .ok_or_else(|| "Account not found.".to_string())
}

#[tauri::command]
async fn get_key_status(key: String) -> ApiOutcome {
    api::get_key_status(key).await
}

#[tauri::command]
async fn redeem_key(key: String) -> ApiOutcome {
    api::redeem_key(key).await
}

#[tauri::command]
async fn request_replacement(key: String, reason: String) -> ApiOutcome {
    api::request_replacement(key, reason).await
}

#[tauri::command]
fn get_last_key() -> String {
    api::get_last_key()
}

#[tauri::command]
fn save_last_key(key: String) -> Result<(), String> {
    api::save_last_key(key)
}

#[tauri::command]
fn get_last_token() -> String {
    config::get_last_token()
}

#[tauri::command]
fn save_last_token(token: String) -> Result<(), String> {
    config::save_last_token(token)
}

#[tauri::command]
async fn import_account(line: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let line = line.trim().to_string();
        if line.is_empty() {
            return Err("Paste username----token first.".into());
        }
        steam::handle_batch_import(&line)
    })
    .await
    .unwrap_or_else(|_| Err("Import task failed".into()))
}

#[tauri::command]
async fn import_from_clipboard() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(steam::import_from_clipboard)
        .await
        .unwrap_or_else(|_| Err("Import task failed".into()))
}

/// Read the OS clipboard from Rust (no WebView clipboard permission prompt).
#[tauri::command]
fn read_clipboard_text() -> Result<String, String> {
    steam::read_clipboard()
}

#[tauri::command]
fn export_account_token(steamid: String) -> Result<String, String> {
    steam::export_account_token(steamid.trim())
}

#[tauri::command]
fn export_all_account_tokens() -> Result<String, String> {
    steam::export_all_account_tokens()
}

#[tauri::command]
fn list_accounts() -> Result<Vec<AccountDto>, String> {
    let accounts = steam::load_steam_accounts()?;
    Ok(accounts.iter().map(account_to_dto).collect())
}

#[tauri::command]
async fn sign_in(steamid: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let account = find_account(steamid.trim())?;
        steam::handle_login_account(&account)
    })
    .await
    .unwrap_or_else(|_| Err("Sign-in task failed".into()))
}

#[tauri::command]
async fn stop_steam() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(steam::stop_steam_only)
        .await
        .unwrap_or_else(|_| Err("Stop Steam task failed".into()))
}

#[tauri::command]
fn clear_all_history() -> Result<String, String> {
    steam::handle_clear_all()
}

#[tauri::command]
fn remove_account(steamid: String) -> Result<String, String> {
    let sid = steamid.trim().to_string();
    let account = find_account(&sid)?;
    steam::handle_delete_account(&account)
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    open::that(url).map_err(|e| e.to_string())
}

#[tauri::command]
fn minimize_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        w.minimize().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn is_running_as_admin() -> bool {
    elevation::is_elevated()
}

#[tauri::command]
fn relaunch_as_admin(app: tauri::AppHandle) -> Result<(), String> {
    elevation::relaunch_as_admin(app)
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

pub fn run() {
    if !mutex::ensure_single_instance() {
        std::process::exit(0);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            tray::setup(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_key_status,
            redeem_key,
            request_replacement,
            get_last_key,
            save_last_key,
            get_last_token,
            save_last_token,
            import_account,
            import_from_clipboard,
            read_clipboard_text,
            export_account_token,
            export_all_account_tokens,
            list_accounts,
            sign_in,
            stop_steam,
            clear_all_history,
            remove_account,
            open_url,
            minimize_window,
            quit_app,
            is_running_as_admin,
            relaunch_as_admin,
        ])
        .run(tauri::generate_context!())
        .expect("error while running nocheater");
}
