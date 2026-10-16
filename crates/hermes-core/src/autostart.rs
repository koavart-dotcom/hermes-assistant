//! Start the shell at Windows login so Ollama and the control API are up
//! after a reboot (the Hermes gateway already autostarts through its own
//! Startup-folder item). Implemented with the per-user `Run` registry key —
//! no admin rights, no COM shortcut code.

use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

use crate::Result;
use crate::cli::hide_console;

pub const RUN_KEY: &str = r"HKCU:\Software\Microsoft\Windows\CurrentVersion\Run";
pub const VALUE_NAME: &str = "Hermes Assistant";

/// PowerShell that registers (or refreshes) the login item for `exe`.
pub fn register_script(exe: &Path) -> String {
    let exe = exe.display().to_string().replace('\'', "''");
    format!(
        "New-Item -Path '{RUN_KEY}' -Force | Out-Null; \
         Set-ItemProperty -Path '{RUN_KEY}' -Name '{VALUE_NAME}' -Value '\"{exe}\"'"
    )
}

/// PowerShell that removes the login item.
pub fn unregister_script() -> String {
    format!(
        "Remove-ItemProperty -Path '{RUN_KEY}' -Name '{VALUE_NAME}' -ErrorAction SilentlyContinue"
    )
}

async fn run_powershell(script: &str) -> Result<()> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console(&mut cmd);
    let status = cmd.status().await?;
    if !status.success() {
        return Err(crate::CoreError::ProcessFailed(
            "powershell (autostart)".into(),
            status.to_string(),
        ));
    }
    Ok(())
}

/// Make `exe` start at login (Windows only; no-op elsewhere). Idempotent.
pub async fn enable(exe: &Path) -> Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }
    run_powershell(&register_script(exe)).await
}

pub async fn disable() -> Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }
    run_powershell(&unregister_script()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_script_quotes_the_path() {
        let s = register_script(Path::new(
            r"C:\Users\Koava\AppData\Local\Hermes Assistant\hermes-assistant.exe",
        ));
        assert!(s.contains(
            r#"-Value '"C:\Users\Koava\AppData\Local\Hermes Assistant\hermes-assistant.exe"'"#
        ));
        assert!(s.contains("Set-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name 'Hermes Assistant'"));
        let odd = register_script(Path::new(r"C:\it's\app.exe"));
        assert!(odd.contains(r"C:\it''s\app.exe"));
    }
}
