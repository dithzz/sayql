//! "Run audit": finds the person's coding agents, runs one read-only in a project folder
//! with the audit instructions, streams its progress to the window as `audit://event`, and
//! opens the same run in a terminal instead. See src/audit_runner.rs for what's enforced.

use crate::agent_process::{TerminalRun, kill_tree, launch_terminal, no_window, version_of};
use crate::ai::{SAYQL_CREDITS, endpoint};
use crate::audit::{self, Target};
use crate::audit_runner::{
    self as runner, AgentSpec, AuditModel, AuditPayment, ClaudeStream, RunFiles,
};
use crate::commands::settings::mcp_binary;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::apps::AppGate;
use crate::store::secrets;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

/// Must match AUDIT_EVENT in src/api/events.ts.
const EVENT: &str = "audit://event";
/// The stderr kept for an error message.
const STDERR_MAX: usize = 16 * 1024;

/// Running audits the window can cancel, by the id it gave each run.
#[derive(Default)]
pub struct AuditRuns(Mutex<HashMap<String, CancellationToken>>);

/// A coding agent SayQL knows about, as found on this computer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    id: &'static str,
    label: &'static str,
    /// Where it's installed; `None` when it isn't found.
    path: Option<String>,
    version: Option<String>,
    /// SayQL can keep it read-only (only these can be run).
    supported: bool,
    /// Found, supported, and recent enough: it can run an audit now.
    ready: bool,
    /// Why it can't run, when found but not ready.
    problem: Option<String>,
    install_url: &'static str,
}

#[derive(Serialize, Clone)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum AuditEvent {
    Line {
        run_id: String,
        kind: &'static str,
        text: String,
    },
    Done {
        run_id: String,
        /// "ok", "error", "cancelled" or "timeout".
        outcome: &'static str,
        /// Suggestions SayQL received for review.
        suggested: u32,
        /// The agent's closing summary.
        summary: Option<String>,
        error: Option<String>,
    },
}

async fn view(spec: &'static AgentSpec) -> AgentView {
    let path = runner::locate(spec);
    let (version, problem) = match (&path, spec.unsupported) {
        (Some(p), None) => {
            let v = version_of(p).await;
            let problem =
                runner::version_problem(spec.id, v.as_deref().and_then(runner::parse_version));
            (v, problem)
        }
        (Some(_), Some(why)) => (None, Some(why.to_string())),
        (None, _) => (None, None),
    };
    AgentView {
        id: spec.id,
        label: spec.label,
        ready: path.is_some() && spec.unsupported.is_none() && problem.is_none(),
        path: path.map(|p| p.to_string_lossy().into_owned()),
        version,
        supported: spec.unsupported.is_none(),
        problem,
        install_url: spec.install_url,
    }
}

/// The coding agents SayQL knows, whether each is installed, and whether it can run an audit.
#[tauri::command]
pub async fn audit_agents() -> AppResult<Vec<AgentView>> {
    let mut out = Vec::new();
    for spec in &runner::AGENTS {
        out.push(view(spec).await);
    }
    Ok(out)
}

/// The project folder last used to audit this connection.
#[tauri::command]
pub fn audit_folder(state: State<'_, AppState>, id: String) -> AppResult<Option<String>> {
    state.store.audit_folder(&id)
}

/// Asks for the project folder and remembers it for this connection; `None` when cancelled.
#[tauri::command]
pub async fn choose_audit_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Choose the project folder with this database's code");
    if let Some(current) = state.store.audit_folder(&id)? {
        dialog = dialog.set_directory(current);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| AppError::User(format!("Could not use that folder: {e}")))?;
    let path = path.to_string_lossy().into_owned();
    state.store.set_audit_folder(&id, &path)?;
    Ok(Some(path))
}

/// The model audits run on.
#[tauri::command]
pub fn audit_model(state: State<'_, AppState>) -> AppResult<AuditModel> {
    state.store.audit_model()
}

/// Remembers the model audits run on.
#[tauri::command]
pub fn set_audit_model(state: State<'_, AppState>, model: AuditModel) -> AppResult<()> {
    state.store.set_audit_model(model)
}

/// Who pays for audits, and whether a SayQL credit key is saved (so credits can be chosen).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditPaymentView {
    payment: AuditPayment,
    has_credit_key: bool,
}

/// Credits count only while paying with them is on ([`runner::CREDITS_RUNS`], off for now) and
/// a credit key is saved; otherwise the run uses the person's own sign-in.
fn effective(stored: AuditPayment, has_credit_key: bool) -> AuditPayment {
    if runner::CREDITS_RUNS && has_credit_key {
        stored
    } else {
        AuditPayment::Subscription
    }
}

#[tauri::command]
pub fn audit_payment(state: State<'_, AppState>) -> AppResult<AuditPaymentView> {
    let has_credit_key = secrets::api_key(SAYQL_CREDITS).is_some();
    Ok(AuditPaymentView {
        payment: effective(
            state.store.audit_payment().unwrap_or_default(),
            has_credit_key,
        ),
        has_credit_key,
    })
}

/// Remembers who pays for audits.
#[tauri::command]
pub fn set_audit_payment(state: State<'_, AppState>, payment: AuditPayment) -> AppResult<()> {
    state.store.set_audit_payment(payment)
}

/// The SayQL credits service a run paid with credits talks to: its address (the one set in
/// Settings → AI provider, else sayql.dev) and the person's credit key from the keychain.
struct Credits {
    base_url: String,
    key: String,
}

/// The credits a run uses: `None` when it's paid with the person's own sign-in.
fn credits(state: &AppState) -> AppResult<Option<Credits>> {
    // Off for now (see `runner::CREDITS_RUNS`): a saved credits preference counts as the
    // person's own sign-in, and nothing points Claude Code at the credits service.
    if !runner::CREDITS_RUNS
        || state.store.audit_payment().unwrap_or_default() != AuditPayment::Credits
    {
        return Ok(None);
    }
    // No key saved any more: the person's own sign-in, as the dialog shows (see `effective`).
    let Some(key) = secrets::api_key(SAYQL_CREDITS) else {
        return Ok(None);
    };
    let provider = endpoint::provider(SAYQL_CREDITS)?;
    let settings = state.store.settings().unwrap_or_default();
    let base_url = endpoint::address(provider, &settings.provider_settings(SAYQL_CREDITS));
    Ok(Some(Credits { base_url, key }))
}

/// Everything a run needs, checked before anything starts.
struct Prepared {
    spec: &'static AgentSpec,
    bin: PathBuf,
    folder: PathBuf,
    connection_id: String,
    database: Option<String>,
    prompt: String,
    files: RunFiles,
    dir: PathBuf,
    model: AuditModel,
    payment: AuditPayment,
    /// Set when the run is paid with SayQL credits.
    credits: Option<Credits>,
}

async fn prepare(
    state: &AppState,
    agent: &str,
    id: &str,
    database: Option<&str>,
    folder: &str,
    headless: bool,
) -> AppResult<Prepared> {
    let spec = runner::spec(agent).ok_or("SayQL doesn't know this coding agent.")?;
    if let Some(why) = spec.unsupported {
        return Err(why.into());
    }
    let folder = PathBuf::from(folder.trim());
    if folder.as_os_str().is_empty() || !folder.is_dir() {
        return Err("Choose the project folder: the one with this database's code.".into());
    }
    let folder = folder.canonicalize()?;
    let credits = credits(state)?;
    let payment = if credits.is_some() {
        AuditPayment::Credits
    } else {
        AuditPayment::Subscription
    };
    // A terminal would need the key in a script file or the shell: the app's own run keeps it
    // in the agent's environment only.
    if credits.is_some() && !headless {
        return Err(
            "Paying with SayQL credits works for runs inside SayQL. Start the audit here, or pay with your Claude Code sign-in to open it in Terminal."
                .into(),
        );
    }
    let bin = runner::locate(spec).ok_or_else(|| {
        format!(
            "{} isn't installed on this computer, or SayQL can't find it.",
            spec.label
        )
    })?;
    let version = version_of(&bin).await;
    if let Some(problem) =
        runner::version_problem(spec.id, version.as_deref().and_then(runner::parse_version))
    {
        return Err(problem.into());
    }
    // The agent reaches the database through SayQL's MCP server, which lets it in only as the
    // person allows: say so now rather than after a run that couldn't read anything.
    match state.store.app_gate(spec.label)? {
        AppGate::Allowed => {}
        AppGate::Blocked => {
            return Err(format!(
                "{} is blocked from using SayQL. Allow it in Settings → AI apps → Activity first.",
                spec.label
            )
            .into());
        }
        AppGate::NotApproved => {
            return Err(format!(
                "Only approved apps may use SayQL, and {} isn't approved. Approve it in Settings → AI apps first.",
                spec.label
            )
            .into());
        }
    }
    let conn = state.store.connection(id)?;
    let database = audit::target_database(&state.store, &conn, database);
    let mut prompt = audit::instructions(
        Some(&Target {
            connection_id: &conn.id,
            name: &conn.name,
            database: database.as_deref(),
        }),
        false,
    );
    prompt.push_str(runner::run_notes(headless));
    let sayql_mcp = mcp_binary()?;
    if !sayql_mcp.exists() {
        return Err(format!(
            "SayQL's MCP server isn't next to the app ({}). Reinstall SayQL; in a dev build, run `cargo build --bin sayql-mcp`.",
            sayql_mcp.display()
        )
        .into());
    }
    let dir = std::env::temp_dir()
        .join("sayql-audit")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir)?;
    let files = RunFiles {
        mcp_config: dir.join("mcp.json"),
        settings: dir.join("settings.json"),
    };
    std::fs::write(&files.mcp_config, runner::mcp_config_json(&sayql_mcp))?;
    std::fs::write(&files.settings, runner::SETTINGS_JSON)?;
    let _ = state
        .store
        .set_audit_folder(&conn.id, &folder.to_string_lossy());
    Ok(Prepared {
        spec,
        bin,
        folder,
        connection_id: conn.id,
        database,
        prompt,
        files,
        dir,
        model: state.store.audit_model().unwrap_or_default(),
        payment,
        credits,
    })
}

/// Starts an audit with `agent` in `folder`; progress arrives as `audit://event` with `runId`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_audit(
    app: AppHandle,
    state: State<'_, AppState>,
    runs: State<'_, AuditRuns>,
    run_id: String,
    agent: String,
    id: String,
    database: Option<String>,
    folder: String,
) -> AppResult<()> {
    let p = prepare(&state, &agent, &id, database.as_deref(), &folder, true).await?;
    let mut cmd = tokio::process::Command::new(&p.bin);
    cmd.args(runner::claude_args(&p.files, true, p.model, p.payment))
        .current_dir(&p.folder)
        .env("PATH", runner::child_path(&p.bin))
        // Started from inside a Claude Code session (a dev build), it would refuse to nest.
        .env_remove("CLAUDECODE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(c) = &p.credits {
        // The credit key goes to the agent's environment only: not its arguments, not a file.
        let env = runner::credits_env(
            &c.base_url,
            &c.key,
            p.model,
            std::env::vars_os().map(|(k, _)| k),
        );
        for name in &env.remove {
            cmd.env_remove(name);
        }
        for (name, value) in &env.set {
            cmd.env(name, value);
        }
    }
    #[cfg(unix)]
    cmd.process_group(0);
    no_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        let _ = std::fs::remove_dir_all(&p.dir);
        AppError::User(format!("Could not start {}: {e}", p.spec.label))
    })?;
    // The prompt goes on stdin: nothing about the run is on the command line but file paths.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(p.prompt.as_bytes()).await?;
        drop(stdin);
    }
    let stop = CancellationToken::new();
    runs.0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(run_id.clone(), stop.clone());
    tauri::async_runtime::spawn(drive(app, run_id, child, p, stop));
    Ok(())
}

async fn drive(
    app: AppHandle,
    run_id: String,
    mut child: tokio::process::Child,
    p: Prepared,
    stop: CancellationToken,
) {
    // Nothing shown in the window carries the credit key.
    let key = p.credits.as_ref().map(|c| c.key.clone());
    let clean = |s: String| runner::redact(&s, key.as_deref());
    let emit = |e: AuditEvent| {
        let _ = app.emit(EVENT, e);
    };
    let stderr = child.stderr.take().map(|mut err| {
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = (&mut err)
                .take(STDERR_MAX as u64)
                .read_to_end(&mut buf)
                .await;
            // Keep reading so the agent never blocks on a full pipe.
            let _ = tokio::io::copy(&mut err, &mut tokio::io::sink()).await;
            String::from_utf8_lossy(&buf).into_owned()
        })
    });
    let mut stream = ClaudeStream::new(&p.folder);
    stream.credits = p.credits.is_some();
    let mut lines = child.stdout.take().map(|o| BufReader::new(o).lines());
    let deadline = tokio::time::sleep(runner::TIME_LIMIT);
    tokio::pin!(deadline);
    let mut ended: Option<&'static str> = None;
    if let Some(lines) = lines.as_mut() {
        loop {
            tokio::select! {
                _ = stop.cancelled() => { ended = Some("cancelled"); break; }
                _ = &mut deadline => { ended = Some("timeout"); break; }
                next = lines.next_line() => match next {
                    Ok(Some(line)) => {
                        for l in stream.feed(&line) {
                            emit(AuditEvent::Line { run_id: run_id.clone(), kind: l.kind, text: clean(l.text) });
                        }
                        // Claude Code retries a refused key for minutes; a credit key that was
                        // refused once won't be accepted on a retry.
                        if stream.credits && stream.api_error.as_deref() == Some("authentication_failed") {
                            ended = Some("refused");
                            break;
                        }
                    }
                    _ => break,
                },
            }
        }
    }
    let status = if ended.is_some() {
        kill_tree(&mut child).await;
        None
    } else {
        match tokio::time::timeout(std::time::Duration::from_secs(30), child.wait()).await {
            Ok(Ok(s)) => Some(s),
            _ => {
                kill_tree(&mut child).await;
                None
            }
        }
    };
    let stderr = match stderr {
        Some(h) => tokio::time::timeout(std::time::Duration::from_secs(5), h)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default(),
        None => String::new(),
    };
    let summary = stream
        .ending
        .as_ref()
        .filter(|e| !e.is_error && !e.text.trim().is_empty())
        .map(|e| clean(e.text.chars().take(4000).collect::<String>()));
    let ok = ended.is_none()
        && status.is_some_and(|s| s.success())
        && stream.ending.as_ref().is_some_and(|e| !e.is_error);
    let failure = || {
        clean(runner::failure(
            stream.ending.as_ref(),
            &stderr,
            stream.api_error.as_deref(),
            status.and_then(|s| s.code()),
            p.payment,
        ))
    };
    let (outcome, error) = match ended {
        Some("cancelled") => ("cancelled", None),
        Some("refused") => ("error", Some(failure())),
        Some(_) => (
            "timeout",
            Some(format!(
                "The audit ran for {} minutes and was stopped. The notes it suggested so far are waiting for you.",
                runner::TIME_LIMIT.as_secs() / 60
            )),
        ),
        None if ok => ("ok", None),
        None => ("error", Some(failure())),
    };
    if outcome == "ok" {
        // Last audit, as when an AI app fetches the audit prompt over MCP.
        let state = app.state::<AppState>();
        let _ = state
            .store
            .record_audit(&p.connection_id, p.database.as_deref(), p.spec.label);
    }
    let _ = std::fs::remove_dir_all(&p.dir);
    app.state::<AuditRuns>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&run_id);
    emit(AuditEvent::Done {
        run_id,
        outcome,
        suggested: stream.suggested,
        summary,
        error,
    });
}

/// Stops a running audit and the agent's processes.
#[tauri::command]
pub fn cancel_audit(runs: State<'_, AuditRuns>, run_id: String) {
    if let Some(stop) = runs
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&run_id)
    {
        stop.cancel();
    }
}

/// Opens the system terminal in `folder`, running the agent interactively with the same
/// read-only options and the audit instructions as its first message.
#[tauri::command]
pub async fn open_audit_in_terminal(
    state: State<'_, AppState>,
    agent: String,
    id: String,
    database: Option<String>,
    folder: String,
) -> AppResult<()> {
    let p = prepare(&state, &agent, &id, database.as_deref(), &folder, false).await?;
    let args = runner::claude_args(&p.files, false, p.model, p.payment);
    let prompt = p.dir.join("prompt.txt");
    std::fs::write(&prompt, &p.prompt)?;
    let path_var = runner::child_path(&p.bin);
    // The files stay: the terminal reads them after this returns (they're in the temp folder).
    launch_terminal(&TerminalRun {
        dir: &p.dir,
        folder: &p.folder,
        bin: &p.bin,
        path_var: &path_var,
        args: &args,
        prompt: &prompt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paying with credits is off: a saved credits choice, even with a credit key, runs on the
    /// person's own sign-in.
    #[test]
    fn a_saved_credits_choice_runs_on_the_persons_own_sign_in() {
        assert_eq!(
            effective(AuditPayment::Credits, true),
            AuditPayment::Subscription
        );
        assert_eq!(
            effective(AuditPayment::Credits, false),
            AuditPayment::Subscription
        );
    }
}
