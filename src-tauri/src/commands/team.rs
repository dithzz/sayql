//! Settings → Team: signing in to a company's SayQL server, and its shared connections.
//! Signed out, none of these make a network request.

use crate::db::ConnConfig;
use crate::error::AppResult;
use crate::state::AppState;
use crate::store::secrets;
use crate::team::ai::AiStatus;
use crate::team::{self, TeamAccount, TeamConnection, TeamStatus};
use serde::Serialize;
use tauri::State;

/// A team connection as the navigator shows it: no host, user or password, ever.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamConnectionView {
    #[serde(flatten)]
    config: ConnConfig,
    team: TeamInfo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TeamInfo {
    /// The database's name on the server, for display.
    database: String,
    notes: String,
}

fn view(c: &TeamConnection) -> Option<TeamConnectionView> {
    let kind = c.kind()?;
    Some(TeamConnectionView {
        config: ConnConfig {
            id: c.local_id(),
            name: c.name.clone(),
            kind,
            host: String::new(),
            port: None,
            // Empty: a team connection has one database, chosen by the server.
            database: String::new(),
            user: String::new(),
            file: String::new(),
            ssl: false,
            trust_cert: false,
        },
        team: TeamInfo { database: c.database.clone(), notes: c.notes.clone() },
    })
}

/// Redeems an invite code, keeps the token in the keychain and the server in settings.
#[tauri::command]
pub async fn team_sign_in(state: State<'_, AppState>, server: String, code: String) -> AppResult<TeamAccount> {
    let server = team::clean_server(&server)?;
    let code = team::clean_code(&code)?;
    let (token, account) = team::redeem(&state.http, &server, &code, &team::device_name()).await?;
    secrets::set_team_token(&token)?;
    let mut settings = state.store.settings()?;
    settings.team = Some(account.clone());
    state.store.save_settings(&settings)?;
    state.forget_team();
    Ok(account)
}

/// Who is signed in, with the server's licence; `None` when signed out (without asking anyone).
#[tauri::command]
pub async fn team_status(state: State<'_, AppState>) -> AppResult<Option<TeamStatus>> {
    let Some(client) = state.team_client()? else { return Ok(None) };
    let (user, server) = client.status().await?;
    let mut settings = state.store.settings()?;
    let Some(saved) = settings.team.clone() else { return Ok(None) };
    let account = TeamAccount { email: user.email, name: user.name, role: user.role, ..saved.clone() };
    if account != saved {
        settings.team = Some(account.clone());
        state.store.save_settings(&settings)?;
    }
    // With /me: team AI, so Settings shows it and AI requests on team connections use it.
    let ai = state.refresh_team_ai().await.ok().flatten();
    Ok(Some(TeamStatus { account, server, ai }))
}

/// Signs out: the server revokes the token (when it can be reached), and this computer forgets it.
#[tauri::command]
pub async fn team_sign_out(state: State<'_, AppState>) -> AppResult<()> {
    let client = state.team_client().ok().flatten();
    if let Some(client) = client
        && let Err(e) = client.sign_out().await
    {
        // Signing out here still works offline; the admin can revoke the token on the server.
        eprintln!("sayql: the team server didn't confirm the sign-out: {e}");
    }
    team::forget_sign_in(&state.store);
    state.forget_team();
    Ok(())
}

/// The team's shared connections; empty when signed out, without asking anyone.
#[tauri::command]
pub async fn team_connections(state: State<'_, AppState>) -> AppResult<Vec<TeamConnectionView>> {
    let list = match state.team_client()? {
        None => Vec::new(),
        Some(client) => client.connections().await?,
    };
    let views = list.iter().filter_map(view).collect();
    state.set_team_connections(list);
    // Team AI applies to these connections; an older server without it simply has none.
    if state.refresh_team_ai().await.is_err() {
        state.set_team_ai(None);
    }
    Ok(views)
}

/// Team AI's state, asked again (Settings opening); `None` when signed out, without asking anyone.
#[tauri::command]
pub async fn team_ai(state: State<'_, AppState>) -> AppResult<Option<AiStatus>> {
    state.refresh_team_ai().await
}
