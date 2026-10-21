//! The one error type used across the backend. Commands return it to the UI, where it
//! arrives as its display string.

use serde::{Serialize, Serializer};
use std::fmt;

/// Sent instead of a message when a statement needs the user's confirmation.
/// Must match `WRITE_CONFIRM` in src/api/types.ts.
pub const WRITE_CONFIRM: &str = "WRITE_CONFIRM";

#[derive(Debug)]
pub enum AppError {
    /// The statement changes data and the caller hasn't confirmed it yet.
    NeedsWriteConfirmation,
    /// A message written for the person using the app.
    User(String),
    Database(sqlx::Error),
    Io(std::io::Error),
    Keychain(keyring::Error),
    Json(serde_json::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NeedsWriteConfirmation => f.write_str(WRITE_CONFIRM),
            AppError::User(msg) => f.write_str(msg),
            AppError::Database(e) => write!(f, "{e}"),
            AppError::Io(e) => write!(f, "File error: {e}"),
            AppError::Keychain(e) => write!(f, "Keychain error: {e}"),
            AppError::Json(e) => write!(f, "Could not read saved data: {e}"),
        }
    }
}

impl std::error::Error for AppError {}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl From<String> for AppError {
    fn from(msg: String) -> Self {
        AppError::User(msg)
    }
}

impl From<&str> for AppError {
    fn from(msg: &str) -> Self {
        AppError::User(msg.to_string())
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Database(e)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e)
    }
}

impl From<keyring::Error> for AppError {
    fn from(e: keyring::Error) -> Self {
        AppError::Keychain(e)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Json(e)
    }
}
