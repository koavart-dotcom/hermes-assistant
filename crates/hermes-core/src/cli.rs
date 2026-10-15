//! Running child processes (PowerShell, the Hermes CLI, Ollama) with the
//! conventions every call needs: no console window on Windows, Hermes home in
//! the environment, lines streamed to the setup log.

use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::events::{EventSender, SetupEvent, emit};
use crate::{CoreError, Layout, Result};

/// Windows `CREATE_NO_WINDOW`: child consoles must never pop up over the app.
#[cfg(windows)]
pub fn hide_console(cmd: &mut Command) {
    cmd.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
pub fn hide_console(_cmd: &mut Command) {}

/// Base environment for anything that talks to the Hermes installation.
pub fn hermes_env(cmd: &mut Command, layout: &Layout) {
    cmd.env("HERMES_HOME", &layout.hermes_home);
    cmd.env("HERMES_NONINTERACTIVE", "1");
    cmd.env("UV_NO_CONFIG", "1");
}

/// Output of a finished child process.
#[derive(Debug, Default)]
pub struct Output {
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub success: bool,
    pub status: String,
}

/// Run a command to completion, streaming every output line to `events`
/// (when given) and collecting it. Cancellation kills the child.
pub async fn run_streaming(
    mut cmd: Command,
    label: &str,
    events: Option<&EventSender>,
    cancel: &CancellationToken,
) -> Result<Output> {
    hide_console(&mut cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| CoreError::Other("no stdout".into()))?;
    let stderr = child.stderr.take().ok_or_else(|| CoreError::Other("no stderr".into()))?;

    let mut out_lines = BufReader::new(stdout).lines();
    let mut err_lines = BufReader::new(stderr).lines();
    let mut output = Output::default();
    let mut out_done = false;
    let mut err_done = false;

    while !(out_done && err_done) {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                return Err(CoreError::Cancelled);
            }
            line = out_lines.next_line(), if !out_done => match line? {
                Some(l) => {
                    if let Some(tx) = events { emit(tx, SetupEvent::Log { line: l.clone() }); }
                    output.stdout.push(l);
                }
                None => out_done = true,
            },
            line = err_lines.next_line(), if !err_done => match line? {
                Some(l) => {
                    if let Some(tx) = events { emit(tx, SetupEvent::Log { line: format!("[{label}] {l}") }); }
                    output.stderr.push(l);
                }
                None => err_done = true,
            },
        }
    }

    let status = tokio::select! {
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            return Err(CoreError::Cancelled);
        }
        s = child.wait() => s?,
    };
    output.success = status.success();
    output.status = status.to_string();
    Ok(output)
}

/// Run the installed Hermes CLI (`%LOCALAPPDATA%\hermes\bin\hermes`).
pub async fn run_hermes(
    layout: &Layout,
    args: &[&str],
    events: Option<&EventSender>,
    cancel: &CancellationToken,
) -> Result<Output> {
    let cli = layout.hermes_cli();
    let mut cmd = command_for(&cli);
    cmd.args(args);
    hermes_env(&mut cmd, layout);
    let out = run_streaming(cmd, "hermes", events, cancel).await?;
    if !out.success {
        return Err(CoreError::ProcessFailed(
            format!("hermes {}", args.join(" ")),
            format!("{} — {}", out.status, out.stderr.join(" | ")),
        ));
    }
    Ok(out)
}

/// `.cmd` launchers on Windows must go through `cmd.exe /c`.
pub fn command_for(program: &Path) -> Command {
    if cfg!(windows) && program.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd")) {
        let mut c = Command::new("cmd.exe");
        c.arg("/c").arg(program);
        c
    } else {
        Command::new(program)
    }
}

/// PowerShell invocation of a script file with the flags the Hermes installer
/// expects; the caller appends script arguments.
pub fn powershell_script(script: &Path) -> Command {
    let mut c = Command::new("powershell.exe");
    c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]);
    c.arg(script);
    c
}
