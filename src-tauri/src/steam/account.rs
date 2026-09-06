use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::paths::get_steam_path;
use super::vdf::quoted_fields;

#[derive(Clone, Default, PartialEq)]
pub struct SteamAccount {
    pub steamid: String,
    pub account_name: String,
    pub persona_name: String,
    pub most_recent: bool,
    pub timestamp: Option<String>,
    // Raw JWT from our own store, when available. Never sent to the frontend;
    // used to re-provision Steam on sign-in. None for accounts we only see in
    // Steam's loginusers.vdf (e.g. imported before token persistence).
    pub token: Option<String>,
}

impl SteamAccount {
    pub fn display_name(&self) -> &str {
        if !self.persona_name.is_empty() {
            &self.persona_name
        } else if !self.account_name.is_empty() {
            &self.account_name
        } else {
            &self.steamid
        }
    }
}

pub fn load_steam_accounts() -> Result<Vec<SteamAccount>, String> {
    let steam_path = get_steam_path()?;
    let steam_path = Path::new(&steam_path);
    let loginusers_path = steam_path.join("config").join("loginusers.vdf");
    // Missing/unreadable loginusers.vdf is not fatal — our own records below may
    // still list accounts (e.g. right after a login-cache reset).
    let content = fs::read_to_string(&loginusers_path).unwrap_or_default();

    let mut accounts = parse_loginusers(&content);

    // Merge the app's persistent records: attach stored tokens to known accounts,
    // and surface any saved account Steam has since forgotten so it can be signed
    // back in from its token.
    let records = super::tokens::load_records();
    let mut seen: HashSet<String> = accounts.iter().map(|a| a.steamid.clone()).collect();
    for account in &mut accounts {
        if let Some(rec) = records.get(&account.steamid) {
            account.token = Some(rec.token.clone());
            if account.account_name.is_empty() {
                account.account_name = rec.account_name.clone();
            }
            if account.persona_name.is_empty() {
                account.persona_name = rec.persona_name.clone();
            }
        }
    }
    for (steamid, rec) in &records {
        if seen.insert(steamid.clone()) {
            accounts.push(SteamAccount {
                steamid: steamid.clone(),
                account_name: rec.account_name.clone(),
                persona_name: rec.persona_name.clone(),
                token: Some(rec.token.clone()),
                ..Default::default()
            });
        }
    }

    accounts.sort_by(|a, b| {
        b.most_recent
            .cmp(&a.most_recent)
            .then(a.display_name().cmp(b.display_name()))
    });
    Ok(accounts)
}

fn parse_loginusers(content: &str) -> Vec<SteamAccount> {
    let mut accounts = Vec::new();
    let mut current: Option<SteamAccount> = None;

    for line in content.lines() {
        let fields = quoted_fields(line);
        if fields.len() == 1 && is_steamid64(&fields[0]) {
            current = Some(SteamAccount {
                steamid: fields[0].clone(),
                ..Default::default()
            });
            continue;
        }

        if line.trim() == "}" {
            if let Some(account) = current.take() {
                if !account.steamid.is_empty() {
                    accounts.push(account);
                }
            }
            continue;
        }

        if let Some(account) = current.as_mut() {
            if fields.len() >= 2 {
                match fields[0].as_str() {
                    "AccountName" => account.account_name = fields[1].clone(),
                    "PersonaName" => account.persona_name = fields[1].clone(),
                    "MostRecent" => account.most_recent = fields[1] == "1",
                    "Timestamp" => account.timestamp = Some(fields[1].clone()),
                    _ => {}
                }
            }
        }
    }

    accounts
}

fn is_steamid64(value: &str) -> bool {
    value.len() >= 16 && value.chars().all(|c| c.is_ascii_digit())
}
