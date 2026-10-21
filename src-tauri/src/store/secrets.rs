//! Database passwords and the Claude API key, kept only in the OS keychain.

use crate::error::AppResult;

const SERVICE: &str = "com.sayql.app";
const API_KEY_ACCOUNT: &str = "anthropic-api-key";

fn entry(account: &str) -> AppResult<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, account)?)
}

fn get(account: &str) -> Option<String> {
    entry(account).ok()?.get_password().ok().filter(|s| !s.is_empty())
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

/// The keychain entry wins; `ANTHROPIC_API_KEY` is a fallback for development.
pub fn api_key() -> Option<String> {
    get(API_KEY_ACCOUNT).or_else(|| std::env::var("ANTHROPIC_API_KEY").ok().filter(|s| !s.is_empty()))
}

pub fn set_api_key(key: &str) -> AppResult<()> {
    set(API_KEY_ACCOUNT, key.trim())
}
