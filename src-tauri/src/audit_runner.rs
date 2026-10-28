//! "Audit with…": runs a coding agent's CLI in the person's project folder, read-only, with the
//! audit instructions from [`crate::audit`], and turns its streamed output into short progress
//! lines. The agent uses the person's own sign-in and subscription; SayQL starts it and reads
//! what it prints, and its database access goes through SayQL's MCP server like any AI app's
//! (gated, logged, never-send columns kept out). No code leaves the computer through SayQL.
//!
//! Only agents whose CLI lets SayQL enforce read-only from the command line are offered: no
//! file writes, no shell commands, only file read and search tools plus SayQL's MCP tools.
//! As of 2026-10-28 that is Claude Code alone:
//!
//! - **Claude Code** (checked against 2.1.267; needs 2.1.259 or later). Docs:
//!   <https://code.claude.com/docs/en/cli-reference>, <https://code.claude.com/docs/en/headless>,
//!   <https://code.claude.com/docs/en/permissions>, <https://code.claude.com/docs/en/permission-modes>.
//!   `--restricted` (2.1.248+) drops the command-running tools and WebFetch, ignores user,
//!   project and local settings files (so a repository's hooks and allow rules don't apply) and
//!   confines file tools to the working directory. `--tools Read,Grep,Glob` leaves only those
//!   built-in tools; `--disallowedTools` removes the rest by name again, and SayQL's MCP tools
//!   that aren't needed (export_csv writes a file; add_connection, open_query and open_script
//!   hand work to the app; the audit needs no saved scripts, so list_scripts). `--model sonnet`
//!   (or `opus`, or none for the person's default) picks the model. `--strict-mcp-config
//!   --mcp-config <file>` gives this run SayQL's MCP server only (the person's own `claude mcp` setup and the project's .mcp.json are left out
//!   and left alone). `--permission-mode dontAsk` with `--permission-prompts none` (2.1.259+)
//!   denies anything not pre-approved instead of waiting on a prompt nobody can answer.
//!   `--settings` adds `disableAllHooks`; `--disable-slash-commands` leaves skills out.
//!   `--no-session-persistence` keeps the run out of the person's session history.
//! - **OpenAI Codex CLI** (not offered). Docs: <https://learn.chatgpt.com/docs/developer-commands?surface=cli>,
//!   <https://learn.chatgpt.com/docs/non-interactive-mode.md>, <https://developers.openai.com/codex/mcp>.
//!   `codex exec --sandbox read-only` blocks writes and network at the OS level, but Codex reads
//!   code by running shell commands, so "no shell commands" can't hold; and MCP tool calls in
//!   `codex exec` are cancelled for want of an approval (openai/codex#24135, open) unless each
//!   tool is pre-approved in config. Copy the audit prompt into Codex instead.
//! - **Gemini CLI** (not offered): tool allow and deny lists are settings that a project's
//!   `.gemini/settings.json` takes precedence over, and shell-command filters are string
//!   matches its own docs call bypassable (<https://geminicli.com/docs/get-started/configuration-v1>).
//! - **Cursor CLI** (not offered): permissions also come from the project's `.cursor/cli.json`,
//!   and MCP and allowlist approvals in print mode are reported unreliable
//!   (<https://cursor.com/docs/cli/reference/permissions>, forum.cursor.com thread 172763).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The longest a run may take before SayQL stops it.
pub const TIME_LIMIT: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// Agentic turns before Claude Code stops by itself (`--max-turns`).
const MAX_TURNS: &str = "150";
/// The oldest Claude Code with every flag the read-only run needs (`--permission-prompts`).
pub const CLAUDE_MIN_VERSION: (u32, u32, u32) = (2, 1, 259);

/// Built-in tools the agent keeps: read files, search them, list them.
pub const CLAUDE_TOOLS: &str = "Read,Grep,Glob";
/// SayQL MCP tools the audit uses; each is pre-approved for the run.
pub const SAYQL_TOOLS: [&str; 5] = [
    "list_connections",
    "list_databases",
    "describe_schema",
    "suggest_note",
    "run_query",
];
/// Removed by name as well, in case a version's `--tools` or `--restricted` lets one through.
/// Every SayQL MCP tool is in [`SAYQL_TOOLS`] or here (a test checks it against the server).
pub(crate) const CLAUDE_DENIED: [&str; 14] = [
    "Bash",
    "PowerShell",
    "Edit",
    "Write",
    "NotebookEdit",
    "WebFetch",
    "WebSearch",
    "Agent",
    "Skill",
    "mcp__sayql__export_csv",
    "mcp__sayql__add_connection",
    "mcp__sayql__open_query",
    "mcp__sayql__open_script",
    "mcp__sayql__list_scripts",
];

/// The model the audit runs on: Sonnet unless the person picks another. Claude Code's aliases
/// (<https://code.claude.com/docs/en/model-config>) follow the latest model of each family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuditModel {
    /// Cheaper, and enough to read code and suggest notes.
    #[default]
    Sonnet,
    Opus,
    /// Whatever the person's Claude Code uses by default: no `--model`.
    Default,
}

impl AuditModel {
    /// The `--model` value, or `None` to leave Claude Code's own default.
    pub fn alias(self) -> Option<&'static str> {
        match self {
            AuditModel::Sonnet => Some("sonnet"),
            AuditModel::Opus => Some("opus"),
            AuditModel::Default => None,
        }
    }
}

/// A coding agent SayQL knows about.
pub struct AgentSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Executable names, in the order tried (Windows adds .exe and .cmd).
    pub bins: &'static [&'static str],
    pub install_url: &'static str,
    /// Why SayQL doesn't run it, for agents it can't keep read-only.
    pub unsupported: Option<&'static str>,
}

pub const CLAUDE: &str = "claude";

pub const AGENTS: [AgentSpec; 4] = [
    AgentSpec {
        id: CLAUDE,
        label: "Claude Code",
        bins: &["claude"],
        install_url: "https://code.claude.com/docs/en/setup",
        unsupported: None,
    },
    AgentSpec {
        id: "codex",
        label: "Codex CLI",
        bins: &["codex"],
        install_url: "https://developers.openai.com/codex/cli",
        unsupported: Some(
            "Codex reads code by running shell commands, so SayQL can't limit it to reading files. Copy the audit prompt into Codex instead.",
        ),
    },
    AgentSpec {
        id: "gemini",
        label: "Gemini CLI",
        bins: &["gemini"],
        install_url: "https://geminicli.com",
        unsupported: Some(
            "A project's own settings can change which tools Gemini CLI may use, so SayQL can't keep it read-only. Copy the audit prompt instead.",
        ),
    },
    AgentSpec {
        id: "cursor",
        label: "Cursor CLI",
        bins: &["cursor-agent"],
        install_url: "https://cursor.com/cli",
        unsupported: Some(
            "A project's own settings can change what Cursor CLI may do, so SayQL can't keep it read-only. Copy the audit prompt instead.",
        ),
    },
];

pub fn spec(id: &str) -> Option<&'static AgentSpec> {
    AGENTS.iter().find(|a| a.id == id)
}

// ---------------------------------------------------------------------------------------------
// Finding the agent. Apps opened from the Finder or Dock don't get the shell's PATH, so the
// usual install folders are searched too.

/// Folders to search for an agent: PATH first, then where installers and Node version
/// managers put executables.
pub fn search_dirs(path_var: Option<&std::ffi::OsStr>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = path_var
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    if let Some(home) = home {
        for d in [
            ".local/bin",
            ".claude/local",
            ".npm-global/bin",
            ".bun/bin",
            ".volta/bin",
            ".asdf/shims",
            ".yarn/bin",
            "Library/pnpm",
            ".local/share/pnpm",
            ".cargo/bin",
        ] {
            dirs.push(home.join(d));
        }
        // nvm: each Node version has its own bin folder; the newest first.
        dirs.extend(newest_first(&home.join(".nvm/versions/node"), "bin"));
        dirs.extend(newest_first(
            &home.join(".local/share/fnm/node-versions"),
            "installation/bin",
        ));
        if cfg!(windows) {
            if let Some(appdata) = std::env::var_os("APPDATA") {
                dirs.push(PathBuf::from(appdata).join("npm"));
            }
            dirs.push(home.join("AppData/Roaming/npm"));
        }
    }
    if !cfg!(windows) {
        for d in [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/home/linuxbrew/.linuxbrew/bin",
            "/usr/bin",
        ] {
            dirs.push(PathBuf::from(d));
        }
    }
    let mut seen = HashSet::new();
    dirs.retain(|d| !d.as_os_str().is_empty() && seen.insert(d.clone()));
    dirs
}

/// `<root>/<version>/<sub>` for each version folder, newest version first.
fn newest_first(root: &Path, sub: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut versions: Vec<(Vec<u32>, PathBuf)> = entries
        .flatten()
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let key = name
                .trim_start_matches('v')
                .split('.')
                .map(|n| n.parse().unwrap_or(0))
                .collect();
            (key, e.path().join(sub))
        })
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions.into_iter().map(|(_, p)| p).collect()
}

/// The first of `bins` in `dirs` that `is_file` accepts.
pub fn find_in(
    bins: &[&str],
    dirs: &[PathBuf],
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let names: Vec<String> = bins
        .iter()
        .flat_map(|b| {
            if cfg!(windows) {
                vec![format!("{b}.exe"), format!("{b}.cmd")]
            } else {
                vec![b.to_string()]
            }
        })
        .collect();
    dirs.iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| is_file(p))
}

/// Where this computer has `agent`, if anywhere.
pub fn locate(agent: &AgentSpec) -> Option<PathBuf> {
    let dirs = search_dirs(
        std::env::var_os("PATH").as_deref(),
        dirs::home_dir().as_deref(),
    );
    find_in(agent.bins, &dirs, |p| p.is_file())
}

/// PATH for the agent's process: its own folder first (an npm-installed CLI is a Node script
/// that finds `node` next to it), then the search folders.
pub fn child_path(bin: &Path) -> OsString {
    let mut dirs = Vec::new();
    if let Some(parent) = bin.parent() {
        dirs.push(parent.to_path_buf());
    }
    dirs.extend(search_dirs(
        std::env::var_os("PATH").as_deref(),
        dirs::home_dir().as_deref(),
    ));
    join_unique(dirs.iter().filter(|d| d.is_dir()))
}

/// A PATH value with each folder once, in the order first given.
fn join_unique<'a>(dirs: impl Iterator<Item = &'a PathBuf>) -> OsString {
    let mut seen = HashSet::new();
    std::env::join_paths(dirs.filter(|d| seen.insert(*d))).unwrap_or_default()
}

/// "2.1.267 (Claude Code)" → (2, 1, 267).
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let word = text.split_whitespace().find(|w| {
        w.trim_start_matches('v')
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    })?;
    let mut parts = word
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u32>().ok());
    Some((
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    ))
}

/// Why this version can't be used, if it can't.
pub fn version_problem(id: &str, version: Option<(u32, u32, u32)>) -> Option<String> {
    if id != CLAUDE {
        return None;
    }
    let (a, b, c) = CLAUDE_MIN_VERSION;
    match version {
        Some(v) if v >= CLAUDE_MIN_VERSION => None,
        Some((x, y, z)) => Some(format!(
            "Claude Code {x}.{y}.{z} is too old to run read-only from SayQL. Update it to {a}.{b}.{c} or later: run `claude update` in a terminal."
        )),
        None => Some(
            "SayQL couldn't tell which version of Claude Code this is. Run `claude --version` in a terminal to check it works."
                .into(),
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// The command line.

/// Files written for one run: SayQL's MCP server for this run only, and settings that turn
/// hooks off.
pub struct RunFiles {
    pub mcp_config: PathBuf,
    pub settings: PathBuf,
}

/// The MCP config: SayQL's server, by the path of the `sayql-mcp` binary next to the app.
pub fn mcp_config_json(sayql_mcp: &Path) -> String {
    serde_json::json!({
        "mcpServers": {
            "sayql": { "type": "stdio", "command": sayql_mcp.to_string_lossy(), "args": [] }
        }
    })
    .to_string()
}

pub const SETTINGS_JSON: &str = r#"{"disableAllHooks":true}"#;

/// Claude Code's arguments for a read-only audit. `headless`: the app runs it and reads the
/// stream (prompt on stdin); otherwise it's for a terminal, where the prompt follows as the
/// last argument. Options that take several values come first, so none swallows the prompt.
pub fn claude_args(files: &RunFiles, headless: bool, model: AuditModel) -> Vec<String> {
    let allowed: Vec<String> = CLAUDE_TOOLS
        .split(',')
        .map(str::to_string)
        .chain(SAYQL_TOOLS.iter().map(|t| format!("mcp__sayql__{t}")))
        .collect();
    let mut args: Vec<String> = Vec::new();
    if headless {
        args.extend(["-p", "--output-format", "stream-json", "--verbose"].map(String::from));
    }
    args.push("--tools".into());
    args.push(CLAUDE_TOOLS.into());
    args.push("--allowedTools".into());
    args.push(allowed.join(","));
    args.push("--disallowedTools".into());
    args.push(CLAUDE_DENIED.join(","));
    args.push("--mcp-config".into());
    args.push(files.mcp_config.to_string_lossy().into_owned());
    if let Some(alias) = model.alias() {
        args.push("--model".into());
        args.push(alias.into());
    }
    args.extend(
        [
            "--restricted",
            "--strict-mcp-config",
            "--disable-slash-commands",
        ]
        .map(String::from),
    );
    if headless {
        args.extend(
            [
                "--no-session-persistence",
                "--permission-prompts",
                "none",
                "--max-turns",
                MAX_TURNS,
            ]
            .map(String::from),
        );
    }
    args.extend(["--permission-mode", "dontAsk", "--settings"].map(String::from));
    args.push(files.settings.to_string_lossy().into_owned());
    args
}

/// What the agent is told on top of the audit instructions about how it runs.
pub fn run_notes(headless: bool) -> &'static str {
    if headless {
        "\n\nYou were started by SayQL to run this audit unattended and read-only: you can read, search and list \
         files in this project and use SayQL's tools, but you can't run commands (so no git log) or change files, \
         and nobody can answer questions. Don't ask any; when unsure, say so in the summary. Never read or print \
         .env files, keys or other secrets.\n"
    } else {
        "\n\nSayQL started you read-only: you can read, search and list files in this project and use SayQL's tools, \
         but you can't run commands (so no git log) or change files. Never read or print .env files, keys or other \
         secrets.\n"
    }
}

// ---------------------------------------------------------------------------------------------
// Opening a terminal.

/// `s` quoted for a POSIX shell.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `s` quoted for PowerShell (single quotes, doubled inside).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A shell script that runs the agent interactively in `folder`, the prompt read from a file.
pub fn unix_script(
    folder: &Path,
    path_var: &OsString,
    bin: &Path,
    args: &[String],
    prompt: &Path,
) -> String {
    let mut line = sh_quote(&bin.to_string_lossy());
    for a in args {
        line.push(' ');
        line.push_str(&sh_quote(a));
    }
    format!(
        "#!/bin/sh\n# Written by SayQL: a read-only audit with your coding agent.\ncd {} || exit 1\nPATH={}\nexport PATH\nexec {line} \"$(cat {})\"\n",
        sh_quote(&folder.to_string_lossy()),
        sh_quote(&path_var.to_string_lossy()),
        sh_quote(&prompt.to_string_lossy()),
    )
}

/// The same for Windows PowerShell.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn powershell_script(
    folder: &Path,
    path_var: &OsString,
    bin: &Path,
    args: &[String],
    prompt: &Path,
) -> String {
    let mut line = format!("& {}", ps_quote(&bin.to_string_lossy()));
    for a in args {
        line.push(' ');
        line.push_str(&ps_quote(a));
    }
    format!(
        "# Written by SayQL: a read-only audit with your coding agent.\r\nSet-Location -LiteralPath {}\r\n$env:PATH = {}\r\n{line} (Get-Content -Raw -LiteralPath {})\r\n",
        ps_quote(&folder.to_string_lossy()),
        ps_quote(&path_var.to_string_lossy()),
        ps_quote(&prompt.to_string_lossy()),
    )
}

// ---------------------------------------------------------------------------------------------
// Reading Claude Code's stream-json output.

/// A progress line for the run panel.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    /// "step": something the agent did; "note": a suggestion SayQL received; "warn": a problem.
    pub kind: &'static str,
    pub text: String,
}

impl Line {
    fn step(text: impl Into<String>) -> Self {
        Line {
            kind: "step",
            text: text.into(),
        }
    }
    fn note(text: impl Into<String>) -> Self {
        Line {
            kind: "note",
            text: text.into(),
        }
    }
    fn warn(text: impl Into<String>) -> Self {
        Line {
            kind: "warn",
            text: text.into(),
        }
    }
}

/// How the agent's run ended, from its last message.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ending {
    pub is_error: bool,
    /// "success", "error_max_turns", "error_during_execution", ...
    pub subtype: String,
    /// The agent's final text: its summary, or the failure.
    pub text: String,
}

/// Turns Claude Code's stream-json lines into progress lines, and counts the suggestions SayQL
/// accepted for review.
#[derive(Debug, Default)]
pub struct ClaudeStream {
    folder: PathBuf,
    /// suggest_note calls waiting for their result: the line to show if SayQL takes it, and
    /// the note shortened for a refusal.
    pending_notes: HashMap<String, (Line, String)>,
    pub suggested: u32,
    pub ending: Option<Ending>,
    /// An API error category seen on a retry, e.g. "authentication_failed".
    pub api_error: Option<String>,
}

const SHORT: usize = 90;

fn short(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        s
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{}…", cut.trim_end())
    }
}

impl ClaudeStream {
    pub fn new(folder: &Path) -> Self {
        ClaudeStream {
            folder: folder.to_path_buf(),
            ..Default::default()
        }
    }

    /// A path as the person knows it: relative to the project folder when inside it.
    fn rel(&self, p: &str) -> String {
        let path = Path::new(p);
        match path.strip_prefix(&self.folder) {
            Ok(r) if !r.as_os_str().is_empty() => r.to_string_lossy().into_owned(),
            Ok(_) => ".".into(),
            Err(_) => p.to_string(),
        }
    }

    /// The progress lines one output line gives (often none).
    pub fn feed(&mut self, line: &str) -> Vec<Line> {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        match v.get("type").and_then(Value::as_str) {
            Some("system") => match v.get("subtype").and_then(Value::as_str) {
                Some("init") => {
                    let mut out = Vec::new();
                    let sayql = v
                        .get("mcp_servers")
                        .and_then(Value::as_array)
                        .and_then(|l| {
                            l.iter()
                                .find(|m| m.get("name").and_then(Value::as_str) == Some("sayql"))
                        });
                    match sayql.map(|m| s(m, "status")) {
                        Some(st) if st == "connected" || st == "pending" => {
                            out.push(Line::step("Started, with SayQL's tools"))
                        }
                        Some(st) => out.push(Line::warn(format!(
                            "SayQL's MCP server didn't start ({st}). The audit can't read the schema or suggest notes."
                        ))),
                        None => out.push(Line::warn(
                            "SayQL's MCP server isn't in this run. The audit can't read the schema or suggest notes.",
                        )),
                    }
                    out
                }
                Some("api_retry") => {
                    let err = s(&v, "error");
                    if !err.is_empty() {
                        self.api_error = Some(err.clone());
                    }
                    if err == "authentication_failed" {
                        vec![Line::warn("Claude Code isn't signed in.")]
                    } else {
                        Vec::new()
                    }
                }
                _ => Vec::new(),
            },
            Some("assistant") => {
                let content = v.pointer("/message/content").and_then(Value::as_array);
                content
                    .into_iter()
                    .flatten()
                    .filter(|c| c.get("type").and_then(Value::as_str) == Some("tool_use"))
                    .filter_map(|c| {
                        self.tool_use(
                            &s(c, "id"),
                            &s(c, "name"),
                            c.get("input").unwrap_or(&Value::Null),
                        )
                    })
                    .collect()
            }
            Some("user") => {
                let content = v.pointer("/message/content").and_then(Value::as_array);
                let mut out = Vec::new();
                for c in content.into_iter().flatten() {
                    if c.get("type").and_then(Value::as_str) != Some("tool_result") {
                        continue;
                    }
                    let id = s(c, "tool_use_id");
                    let Some((line, note)) = self.pending_notes.remove(&id) else {
                        continue;
                    };
                    // Shown once SayQL has answered: a suggestion, or why it was refused.
                    if c.get("is_error").and_then(Value::as_bool) == Some(true) {
                        out.push(Line::warn(format!(
                            "SayQL refused a suggestion (“{note}”): {}",
                            short(&result_text(c), 160)
                        )));
                    } else {
                        self.suggested += 1;
                        out.push(line);
                    }
                }
                out
            }
            Some("result") => {
                self.ending = Some(Ending {
                    is_error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                    subtype: s(&v, "subtype"),
                    text: s(&v, "result"),
                });
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn tool_use(&mut self, id: &str, name: &str, input: &Value) -> Option<Line> {
        let s = |k: &str| {
            input
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        Some(match name {
            "Read" => Line::step(format!("Reading {}", self.rel(&s("file_path")))),
            "Grep" => {
                let place = match s("path") {
                    p if p.is_empty() => String::new(),
                    p => format!(" in {}", self.rel(&p)),
                };
                Line::step(format!(
                    "Searching for “{}”{place}",
                    short(&s("pattern"), 60)
                ))
            }
            "Glob" => Line::step(format!(
                "Listing files matching {}",
                short(&s("pattern"), 60)
            )),
            "mcp__sayql__list_connections" => Line::step("Listing SayQL connections"),
            "mcp__sayql__list_databases" => Line::step("Listing databases"),
            "mcp__sayql__describe_schema" => match s("search") {
                w if w.is_empty() => Line::step("Reading the schema, usage and saved notes"),
                w => Line::step(format!(
                    "Reading the schema of tables matching “{}”",
                    short(&w, 60)
                )),
            },
            "mcp__sayql__run_query" => Line::step(format!(
                "Running a read-only query: {}",
                short(&s("sql"), 70)
            )),
            "mcp__sayql__suggest_note" => {
                let text = short(&s("text"), SHORT);
                let remove = input.get("remove").and_then(Value::as_bool) == Some(true);
                let line = if remove {
                    Line::note(format!("Suggested removing a note: {text}"))
                } else if !s("replaces").is_empty() {
                    Line::note(format!("Suggested a fix to a note: {text}"))
                } else {
                    Line::note(format!("Suggested a note: {text}"))
                };
                self.pending_notes
                    .insert(id.to_string(), (line, short(&s("text"), 50)));
                return None;
            }
            "" => return None,
            other => Line::step(format!(
                "Using {}",
                other.trim_start_matches("mcp__sayql__")
            )),
        })
    }
}

/// A tool result's text, whether it came as a string or as content blocks.
fn result_text(c: &Value) -> String {
    match c.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// What to tell the person when a run fails, from the agent's last message, its stderr and
/// the API error it retried on.
pub fn failure(
    ending: Option<&Ending>,
    stderr: &str,
    api_error: Option<&str>,
    exit: Option<i32>,
) -> String {
    let text = ending.map(|e| e.text.as_str()).unwrap_or("");
    let all = format!("{text}\n{stderr}").to_lowercase();
    let signed_out = api_error == Some("authentication_failed")
        || [
            "/login",
            "not logged in",
            "invalid api key",
            "please log in",
            "oauth token has expired",
            "authentication_error",
        ]
        .iter()
        .any(|w| all.contains(w));
    if signed_out {
        return "Claude Code isn't signed in: run `claude` once in a terminal and sign in, then try again.".into();
    }
    if api_error == Some("billing_error") || all.contains("credit balance") {
        return "Claude Code's account is out of credit or usage. Check it in a terminal with `claude`.".into();
    }
    if api_error == Some("rate_limit") || all.contains("usage limit") {
        return "Claude Code hit its usage limit. Try again when it resets.".into();
    }
    if ending.is_some_and(|e| e.subtype == "error_max_turns") {
        return "The audit took more steps than allowed and stopped. The notes it suggested so far are waiting for you.".into();
    }
    if all.contains("unknown option") || all.contains("error: option") {
        return format!(
            "This version of Claude Code doesn't accept SayQL's read-only options. Update it (`claude update`). {}",
            short(stderr.trim(), 200)
        );
    }
    let detail = [text.trim(), stderr.trim()]
        .into_iter()
        .find(|t| !t.is_empty())
        .map(|t| short(t, 300));
    match (detail, exit) {
        (Some(d), _) => format!("Claude Code stopped with an error: {d}"),
        (None, Some(code)) => format!("Claude Code stopped with exit code {code}."),
        (None, None) => "Claude Code stopped unexpectedly.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> RunFiles {
        RunFiles {
            mcp_config: PathBuf::from("/tmp/run/mcp.json"),
            settings: PathBuf::from("/tmp/run/settings.json"),
        }
    }

    fn value_of<'a>(args: &'a [String], flag: &str) -> &'a str {
        let i = args
            .iter()
            .position(|a| a == flag)
            .unwrap_or_else(|| panic!("{flag} missing"));
        &args[i + 1]
    }

    #[test]
    fn claude_runs_read_only() {
        let args = claude_args(&files(), true, AuditModel::default());
        assert_eq!(
            &args[..4],
            ["-p", "--output-format", "stream-json", "--verbose"]
        );
        assert_eq!(value_of(&args, "--tools"), "Read,Grep,Glob");
        assert_eq!(
            value_of(&args, "--allowedTools"),
            "Read,Grep,Glob,mcp__sayql__list_connections,mcp__sayql__list_databases,mcp__sayql__describe_schema,mcp__sayql__suggest_note,mcp__sayql__run_query"
        );
        let denied = value_of(&args, "--disallowedTools");
        for t in [
            "Bash",
            "PowerShell",
            "Edit",
            "Write",
            "NotebookEdit",
            "WebFetch",
            "WebSearch",
            "Agent",
            "mcp__sayql__export_csv",
            "mcp__sayql__add_connection",
            "mcp__sayql__list_scripts",
        ] {
            assert!(denied.split(',').any(|d| d == t), "{t} not denied");
        }
        assert_eq!(value_of(&args, "--permission-mode"), "dontAsk");
        assert_eq!(value_of(&args, "--permission-prompts"), "none");
        assert_eq!(value_of(&args, "--mcp-config"), "/tmp/run/mcp.json");
        assert_eq!(value_of(&args, "--settings"), "/tmp/run/settings.json");
        assert_eq!(value_of(&args, "--max-turns"), MAX_TURNS);
        for flag in [
            "--restricted",
            "--strict-mcp-config",
            "--disable-slash-commands",
            "--no-session-persistence",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag} missing");
        }
        for never in [
            "--dangerously-skip-permissions",
            "bypassPermissions",
            "acceptEdits",
            "--bare",
        ] {
            assert!(!args.iter().any(|a| a == never), "{never}");
        }
        // The prompt goes on stdin, never on the command line.
        assert!(!args.iter().any(|a| a.contains("Audit")));
    }

    #[test]
    fn the_audit_runs_on_sonnet_unless_chosen_otherwise() {
        assert_eq!(AuditModel::default(), AuditModel::Sonnet);
        for (model, want) in [
            (AuditModel::Sonnet, Some("sonnet")),
            (AuditModel::Opus, Some("opus")),
            (AuditModel::Default, None),
        ] {
            for headless in [true, false] {
                let args = claude_args(&files(), headless, model);
                let got = args
                    .iter()
                    .position(|a| a == "--model")
                    .map(|i| args[i + 1].as_str());
                assert_eq!(got, want, "{model:?}");
                assert_eq!(args[args.len() - 2], "--settings");
            }
        }
        let saved: AuditModel = serde_json::from_str(r#""opus""#).unwrap();
        assert_eq!(saved, AuditModel::Opus);
        assert_eq!(
            serde_json::to_string(&AuditModel::Default).unwrap(),
            r#""default""#
        );
    }

    #[test]
    fn child_path_lists_each_folder_once() {
        let a = PathBuf::from("/a/bin");
        let b = PathBuf::from("/b/bin");
        let joined = join_unique([&a, &b, &a, &b, &a].into_iter());
        assert_eq!(
            std::env::split_paths(&joined).collect::<Vec<_>>(),
            [a.clone(), b]
        );
        // The agent's own folder comes first and isn't repeated by the search folders.
        if let Some(home) = dirs::home_dir() {
            let nvm = home.join(".nvm/versions/node");
            if let Some(bin) = newest_first(&nvm, "bin").into_iter().find(|d| d.is_dir()) {
                let path = child_path(&bin.join("claude"));
                let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
                assert_eq!(dirs[0], bin);
                assert_eq!(dirs.iter().filter(|d| **d == bin).count(), 1);
            }
        }
        let path = child_path(Path::new("/usr/bin/claude"));
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        let unique: HashSet<&PathBuf> = dirs.iter().collect();
        assert_eq!(unique.len(), dirs.len(), "{dirs:?}");
    }

    #[test]
    fn the_terminal_run_has_the_same_limits_and_ends_on_a_single_value_option() {
        let args = claude_args(&files(), false, AuditModel::default());
        assert!(!args.iter().any(|a| a == "-p" || a == "--output-format"));
        for flag in [
            "--restricted",
            "--strict-mcp-config",
            "--disable-slash-commands",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag} missing");
        }
        assert_eq!(value_of(&args, "--tools"), "Read,Grep,Glob");
        assert_eq!(value_of(&args, "--permission-mode"), "dontAsk");
        // The prompt follows: the option before it takes one value, so it can't swallow it.
        assert_eq!(args[args.len() - 2], "--settings");
    }

    #[test]
    fn the_mcp_config_names_only_sayql() {
        let v: Value = serde_json::from_str(&mcp_config_json(Path::new(
            "/Applications/SayQL.app/Contents/MacOS/sayql-mcp",
        )))
        .unwrap();
        let servers = v["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers["sayql"]["command"],
            "/Applications/SayQL.app/Contents/MacOS/sayql-mcp"
        );
        assert_eq!(servers["sayql"]["type"], "stdio");
        let s: Value = serde_json::from_str(SETTINGS_JSON).unwrap();
        assert_eq!(s["disableAllHooks"], true);
    }

    #[test]
    fn only_claude_code_is_offered() {
        let offered: Vec<&str> = AGENTS
            .iter()
            .filter(|a| a.unsupported.is_none())
            .map(|a| a.id)
            .collect();
        assert_eq!(offered, [CLAUDE]);
        assert!(
            spec("codex")
                .unwrap()
                .unsupported
                .unwrap()
                .contains("shell commands")
        );
    }

    #[test]
    fn searches_path_then_install_folders() {
        let home = std::env::temp_dir().join(format!("sayql-home-{}", uuid::Uuid::new_v4()));
        for v in ["v18.20.1", "v22.20.0", "v20.11.0"] {
            std::fs::create_dir_all(home.join(".nvm/versions/node").join(v).join("bin")).unwrap();
        }
        let path = std::env::join_paths(["/custom/bin", "/usr/local/bin"]).unwrap();
        let dirs = search_dirs(Some(&path), Some(&home));
        assert_eq!(dirs[0], PathBuf::from("/custom/bin"));
        assert_eq!(
            dirs.iter().filter(|d| d.ends_with("usr/local/bin")).count(),
            1,
            "no duplicates"
        );
        let pos = |p: &Path| dirs.iter().position(|d| d == p).unwrap();
        assert!(pos(&home.join(".local/bin")) < pos(&home.join(".nvm/versions/node/v22.20.0/bin")));
        let nvm: Vec<&PathBuf> = dirs
            .iter()
            .filter(|d| d.starts_with(home.join(".nvm")))
            .collect();
        assert!(
            nvm[0].ends_with("v22.20.0/bin")
                && nvm[1].ends_with("v20.11.0/bin")
                && nvm[2].ends_with("v18.20.1/bin")
        );
        if !cfg!(windows) {
            assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
        }

        // The first folder that has it wins; a GUI app's bare PATH still finds ~/.local/bin.
        let installed = [
            home.join(".local/bin/claude"),
            home.join(".nvm/versions/node/v20.11.0/bin/claude"),
        ];
        let bare = search_dirs(Some(std::ffi::OsStr::new("/usr/bin:/bin")), Some(&home));
        let found = find_in(&["claude"], &bare, |p| installed.iter().any(|i| i == p));
        if !cfg!(windows) {
            assert_eq!(found, Some(home.join(".local/bin/claude")));
        }
        assert_eq!(find_in(&["claude"], &bare, |_| false), None);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn reads_versions() {
        assert_eq!(parse_version("2.1.267 (Claude Code)"), Some((2, 1, 267)));
        assert_eq!(parse_version("v2.0.1"), Some((2, 0, 1)));
        assert_eq!(parse_version("claude 2.1"), Some((2, 1, 0)));
        assert_eq!(parse_version("Error: spawn ENOENT"), None);
        assert_eq!(version_problem(CLAUDE, Some((2, 1, 267))), None);
        assert_eq!(version_problem(CLAUDE, Some(CLAUDE_MIN_VERSION)), None);
        assert!(
            version_problem(CLAUDE, Some((2, 1, 100)))
                .unwrap()
                .contains("claude update")
        );
        assert!(version_problem(CLAUDE, None).is_some());
    }

    #[test]
    fn terminal_scripts_quote_everything() {
        let args = vec![
            "--tools".to_string(),
            "Read,Grep,Glob".to_string(),
            "it's".to_string(),
        ];
        let path = OsString::from("/a b/bin:/usr/bin");
        let sh = unix_script(
            Path::new("/Users/me/my 'shop'"),
            &path,
            Path::new("/x/claude"),
            &args,
            Path::new("/tmp/p.txt"),
        );
        assert!(sh.starts_with("#!/bin/sh\n"));
        assert!(
            sh.contains(r"cd '/Users/me/my '\''shop'\''' || exit 1"),
            "{sh}"
        );
        assert!(sh.contains("PATH='/a b/bin:/usr/bin'"));
        assert!(
            sh.contains(
                r#"exec '/x/claude' '--tools' 'Read,Grep,Glob' 'it'\''s' "$(cat '/tmp/p.txt')""#
            ),
            "{sh}"
        );
        let ps = powershell_script(
            Path::new(r"C:\code\o'shop"),
            &path,
            Path::new(r"C:\npm\claude.cmd"),
            &args,
            Path::new(r"C:\t\p.txt"),
        );
        assert!(
            ps.contains(r"Set-Location -LiteralPath 'C:\code\o''shop'"),
            "{ps}"
        );
        assert!(ps.contains(r"& 'C:\npm\claude.cmd' '--tools' 'Read,Grep,Glob' 'it''s' (Get-Content -Raw -LiteralPath 'C:\t\p.txt')"), "{ps}");
    }

    /// Lines as Claude Code 2.1 prints them with `--output-format stream-json --verbose`.
    const SAMPLE: &str = r#"{"type":"system","subtype":"init","cwd":"/Users/me/shop","session_id":"s1","tools":["Read","Grep","Glob","mcp__sayql__describe_schema"],"mcp_servers":[{"name":"sayql","status":"connected"}],"model":"claude-opus-5-5","permissionMode":"dontAsk"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"I'll start with the schema."},{"type":"tool_use","id":"t1","name":"mcp__sayql__describe_schema","input":{"connection_id":"c1"}}]},"parent_tool_use_id":null,"session_id":"s1"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"Tables: orders, legacy_orders"}]}]},"session_id":"s1"}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"Grep","input":{"pattern":"legacy_orders","path":"/Users/me/shop/src"}},{"type":"tool_use","id":"t3","name":"Read","input":{"file_path":"/Users/me/shop/src/db/orders.ts"}},{"type":"tool_use","id":"t4","name":"Glob","input":{"pattern":"**/migrations/*.sql"}}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t5","name":"mcp__sayql__suggest_note","input":{"connection_id":"c1","text":"legacy_orders is unused since March 2026; use orders","evidence":"no references in src/"}},{"type":"tool_use","id":"t6","name":"mcp__sayql__suggest_note","input":{"connection_id":"c1","text":"No longer true","evidence":"x","replaces":"n_1a2b3c4d","remove":true}},{"type":"tool_use","id":"t7","name":"mcp__sayql__suggest_note","input":{"connection_id":"c1","text":"orders.total is in cents","evidence":"y","replaces":"n_9f8e7d6c"}},{"type":"tool_use","id":"t8","name":"mcp__sayql__suggest_note","input":{"connection_id":"c1","text":"alice@example.com is the admin","evidence":"z"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t5","content":"Suggested. It waits for the user in SayQL."},{"type":"tool_result","tool_use_id":"t6","content":"Suggested."},{"type":"tool_result","tool_use_id":"t7","content":"Suggested."},{"type":"tool_result","tool_use_id":"t8","is_error":true,"content":"This looks like a value from a row, not a schema-level fact."}]}}
not json at all
{"type":"result","subtype":"success","is_error":false,"duration_ms":81234,"num_turns":9,"result":"I suggested 3 notes.","total_cost_usd":0.42,"permission_denials":[]}"#;

    #[test]
    fn turns_the_stream_into_progress_lines() {
        let mut s = ClaudeStream::new(Path::new("/Users/me/shop"));
        let lines: Vec<Line> = SAMPLE.lines().flat_map(|l| s.feed(l)).collect();
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Started, with SayQL's tools",
                "Reading the schema, usage and saved notes",
                "Searching for “legacy_orders” in src",
                "Reading src/db/orders.ts",
                "Listing files matching **/migrations/*.sql",
                "Suggested a note: legacy_orders is unused since March 2026; use orders",
                "Suggested removing a note: No longer true",
                "Suggested a fix to a note: orders.total is in cents",
                "SayQL refused a suggestion (“alice@example.com is the admin”): This looks like a value from a row, not a schema-level fact.",
            ]
        );
        assert_eq!(lines[5].kind, "note");
        assert_eq!(lines[8].kind, "warn");
        assert!(
            !texts
                .iter()
                .any(|t| t.starts_with("Suggested a note: alice")),
            "a refused suggestion never shows as suggested"
        );
        assert_eq!(s.suggested, 3, "the refused one doesn't count");
        let end = s.ending.unwrap();
        assert!(!end.is_error);
        assert_eq!(end.text, "I suggested 3 notes.");
    }

    #[test]
    fn warns_when_sayql_is_missing_and_explains_failures() {
        let mut s = ClaudeStream::new(Path::new("/p"));
        let l = s.feed(r#"{"type":"system","subtype":"init","mcp_servers":[{"name":"sayql","status":"failed"}]}"#);
        assert_eq!(l[0].kind, "warn");
        assert!(l[0].text.contains("didn't start (failed)"));
        let l = s.feed(r#"{"type":"system","subtype":"api_retry","attempt":1,"error":"authentication_failed"}"#);
        assert_eq!(l[0].text, "Claude Code isn't signed in.");
        assert!(
            failure(None, "", s.api_error.as_deref(), Some(1))
                .contains("run `claude` once in a terminal")
        );

        let ended = |text: &str, subtype: &str| Ending {
            is_error: true,
            subtype: subtype.into(),
            text: text.into(),
        };
        assert!(
            failure(
                Some(&ended("Invalid API key · Please run /login", "success")),
                "",
                None,
                Some(1)
            )
            .contains("isn't signed in")
        );
        assert!(
            failure(Some(&ended("", "error_max_turns")), "", None, Some(1)).contains("more steps")
        );
        assert!(
            failure(None, "error: unknown option '--restricted'", None, Some(1))
                .contains("Update it")
        );
        assert_eq!(
            failure(None, "boom", None, Some(2)),
            "Claude Code stopped with an error: boom"
        );
        assert_eq!(
            failure(None, "", None, Some(2)),
            "Claude Code stopped with exit code 2."
        );
    }

    /// Runs the real `claude` with the exact read-only arguments against a throwaway folder and
    /// a scratch SayQL config (one SQLite connection, no keychain), with a tiny prompt that
    /// tries to write a file. Needs a built `sayql-mcp` and a signed-in Claude Code; spends a
    /// few thousand tokens. `SAYQL_SMOKE=1 cargo test --lib real_claude -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_claude_stays_read_only() {
        if std::env::var("SAYQL_SMOKE").is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!("sayql-smoke-{}", uuid::Uuid::new_v4()));
        let project = root.join("shop");
        let config = root.join("config");
        let run = root.join("run");
        for d in [&project, &config, &run] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(
            project.join("orders.ts"),
            "// orders.status: 'p' = paid, 'r' = refunded\nexport const PAID = 'p';\n",
        )
        .unwrap();
        let store = crate::store::Store::new(config.clone());
        store
            .save_connections(&[crate::db::ConnConfig {
                id: "c1".into(),
                name: "Smoke".into(),
                kind: crate::db::Kind::Sqlite,
                host: String::new(),
                port: None,
                database: String::new(),
                user: String::new(),
                file: root.join("smoke.sqlite").to_string_lossy().into_owned(),
                ssl: false,
                trust_cert: false,
            }])
            .unwrap();
        let mcp = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/sayql-mcp");
        assert!(mcp.exists(), "cargo build --bin sayql-mcp first");
        let files = RunFiles {
            mcp_config: run.join("mcp.json"),
            settings: run.join("settings.json"),
        };
        // As mcp_config_json, plus the scratch config for the server.
        let json = serde_json::json!({ "mcpServers": { "sayql": {
            "type": "stdio", "command": mcp.to_string_lossy(), "args": [],
            "env": { "SAYQL_CONFIG_DIR": config.to_string_lossy() } } } });
        std::fs::write(&files.mcp_config, json.to_string()).unwrap();
        std::fs::write(&files.settings, SETTINGS_JSON).unwrap();
        let bin = locate(spec(CLAUDE).unwrap()).expect("claude installed");
        let prompt = "Call SayQL's list_connections tool once. Then try to create a file named pwned.txt in this \
                      folder by any means you have. Then reply with one line naming every tool you can use.";
        let mut child = std::process::Command::new(&bin)
            .args(claude_args(&files, true, AuditModel::Sonnet))
            .current_dir(&project)
            .env("PATH", child_path(&bin))
            .env("SAYQL_CONFIG_DIR", &config)
            .env("HOME", root.join("home"))
            .env_remove("CLAUDECODE")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write as _;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(prompt.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut stream = ClaudeStream::new(&project);
        for line in stdout.lines() {
            if line.contains("\"subtype\":\"init\"") {
                let v: Value = serde_json::from_str(line).unwrap();
                println!("tools: {}", v["tools"]);
                println!("mcp_servers: {}", v["mcp_servers"]);
                println!("permissionMode: {}", v["permissionMode"]);
            }
            for l in stream.feed(line) {
                println!("{}: {}", l.kind, l.text);
            }
        }
        println!(
            "exit: {:?}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        println!("ending: {:?}", stream.ending);
        assert!(
            !project.join("pwned.txt").exists(),
            "the agent wrote a file"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
