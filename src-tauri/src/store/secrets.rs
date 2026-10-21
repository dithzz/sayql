//! Database passwords and the Claude API key, kept only in the OS keychain.

use crate::error::AppResult;

const SERVICE: &str = "com.sayql.app";
const API_KEY_ACCOUNT: &str = "anthropic-api-key";

fn entry(account: &str) -> AppResult<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, account)?)
}

/// The keychain service used before the app was renamed from AskDB.
const LEGACY_SERVICE: &str = "com.askdb.app";

fn get(account: &str) -> Option<String> {
    let read = |service: &str| keyring::Entry::new(service, account).ok()?.get_password().ok().filter(|s| !s.is_empty());
    if let Some(value) = read(SERVICE) {
        return Some(value);
    }
    // Saved before the rename: copy it forward so nothing has to be entered again.
    let legacy = read(LEGACY_SERVICE)?;
    let _ = set(account, &legacy);
    Some(legacy)
}

/// An empty value deletes the entry.
fn set(account: &str, value: &str) -> AppResult<()> {
    let e = entry(account)?;
    if value.is_empty() {
        match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.into()),
        }
    } else {
        Ok(e.set_password(value)?)
    }
}

fn connection_account(conn_id: &str) -> String {
    format!("conn:{conn_id}")
}

pub fn password(conn_id: &str) -> Option<String> {
    get(&connection_account(conn_id))
}

pub fn set_password(conn_id: &str, password: &str) -> AppResult<()> {
    set(&connection_account(conn_id), password)
}

/// Claude's key keeps the account name it had before other providers were added.
fn api_key_account(provider: &str) -> String {
    if provider == crate::ai::ANTHROPIC { API_KEY_ACCOUNT.to_string() } else { format!("api-key:{provider}") }
}

/// The AI provider's API key. For Claude, `ANTHROPIC_API_KEY` is a fallback for development.
pub fn api_key(provider: &str) -> Option<String> {
    let saved = get(&api_key_account(provider));
    if provider != crate::ai::ANTHROPIC {
        return saved;
    }
    saved.or_else(|| std::env::var("ANTHROPIC_API_KEY").ok().filter(|s| !s.is_empty()))
}

pub fn set_api_key(provider: &str, key: &str) -> AppResult<()> {
    set(&api_key_account(provider), key.trim())
}
