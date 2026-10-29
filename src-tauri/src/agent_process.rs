//! Running a coding agent's process for "Run audit": asking its version, hiding its console
//! window on Windows, stopping it with everything it started, and opening it in the system's
//! terminal instead.

use crate::error::AppResult;
use std::path::Path;
use std::process::Stdio;

/// `<bin> --version`, run with the agent's PATH; `None` when it doesn't answer in 10 seconds.
pub async fn version_of(bin: &Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("--version")
        .env("PATH", crate::audit_runner::child_path(bin))
        .env_remove("CLAUDECODE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    no_window(&mut cmd);
    let out = tokio::time::timeout(std::time::Duration::from_secs(10), cmd.output())
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

#[cfg(windows)]
pub fn no_window(cmd: &mut tokio::process::Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn no_window(_: &mut tokio::process::Command) {}

/// Stops the agent and everything it started (SayQL's MCP server among them).
pub async fn kill_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(unix)]
        {
            // The child leads its own process group: signal the whole group.
            let _ = tokio::process::Command::new("kill")
                .args(["-TERM", "--", &format!("-{pid}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            if tokio::time::timeout(std::time::Duration::from_secs(3), child.wait())
                .await
                .is_ok()
            {
                return;
            }
            let _ = tokio::process::Command::new("kill")
                .args(["-KILL", "--", &format!("-{pid}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        #[cfg(windows)]
        {
            let _ = tokio::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
}

/// What the terminal runs: the agent at `bin` in `folder`, with `args` and the prompt file, and
/// a script written to `dir`.
pub struct TerminalRun<'a> {
    pub dir: &'a Path,
    pub folder: &'a Path,
    pub bin: &'a Path,
    pub path_var: &'a std::ffi::OsString,
    pub args: &'a [String],
    pub prompt: &'a Path,
}

#[cfg(target_os = "macos")]
pub fn launch_terminal(run: &TerminalRun) -> AppResult<()> {
    let TerminalRun {
        dir,
        folder,
        bin,
        path_var,
        args,
        prompt,
    } = *run;
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("SayQL audit.command");
    std::fs::write(
        &script,
        crate::audit_runner::unix_script(folder, path_var, bin, args, prompt),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
    let status = std::process::Command::new("open")
        .args(["-a", "Terminal"])
        .arg(&script)
        .status()?;
    if !status.success() {
        return Err("Could not open Terminal.".into());
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn launch_terminal(run: &TerminalRun) -> AppResult<()> {
    let TerminalRun {
        dir,
        folder,
        bin,
        path_var,
        args,
        prompt,
    } = *run;
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("sayql-audit.sh");
    std::fs::write(
        &script,
        crate::audit_runner::unix_script(folder, path_var, bin, args, prompt),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
    let terminals: [(&str, &[&str]); 5] = [
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-x"]),
        ("xterm", &["-e"]),
    ];
    for (term, pre) in terminals {
        let spawned = std::process::Command::new(term)
            .args(pre)
            .arg(&script)
            .current_dir(folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if spawned.is_ok() {
            return Ok(());
        }
    }
    Err(format!(
        "SayQL couldn't find a terminal to open. Run this in a terminal instead: {}",
        script.display()
    )
    .into())
}

#[cfg(windows)]
pub fn launch_terminal(run: &TerminalRun) -> AppResult<()> {
    let TerminalRun {
        dir,
        folder,
        bin,
        path_var,
        args,
        prompt,
    } = *run;
    let script = dir.join("sayql-audit.ps1");
    std::fs::write(
        &script,
        crate::audit_runner::powershell_script(folder, path_var, bin, args, prompt),
    )?;
    std::process::Command::new("cmd")
        .args([
            "/c",
            "start",
            "SayQL audit",
            "powershell",
            "-NoExit",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .current_dir(folder)
        .spawn()?;
    Ok(())
}
