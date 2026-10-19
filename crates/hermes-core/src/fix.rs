//! Fixes from the web panel (server mode, stage 4 of ADR 18.10.2026): a short
//! catalogue of safe actions in code, run by the service on an inventory host
//! only when a person presses the button and confirms the exact command.
//!
//! Three locks must all be open: the action is in this catalogue, the host has
//! `fix: true` (and, for services, the name is in its `fix_services`), and the
//! host itself allows it — `sudoers` rules for exactly these commands on Linux
//! (`tools/diag/linux-setup.sh --fix`), the `HermesFix` JEA endpoint on Windows
//! (`tools/diag/windows-setup.ps1 -FixServices`). Security incidents are never
//! fixed. Every run is written to `fix.jsonl`.

use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;
use ts_rs::TS;

use crate::diag::{self, DiagReport};
use crate::inventory::{Access, Host, Os};
use crate::{CoreError, Layout, Result};

/// JEA endpoint with the fix role (separate from the read-only `HermesDiag`).
pub const FIX_JEA: &str = "HermesFix";
const FIX_TIMEOUT: Duration = Duration::from_secs(120);
/// Pause before the check after the fix (a restarted service needs a moment).
const VERIFY_DELAY: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct FixAction {
    pub id: &'static str,
    pub os: Os,
    pub title: &'static str,
    /// What it does and what it may affect — shown before confirmation.
    pub description: &'static str,
    /// Takes a service name from the host's `fix_services`.
    pub needs_service: bool,
    /// Linux: shell command, `{service}` replaced; Windows: ScriptBlock body.
    pub command: &'static str,
    /// Diagnostic probe run afterwards to show the result.
    pub verify: &'static str,
}

const fn a(
    id: &'static str,
    os: Os,
    title: &'static str,
    description: &'static str,
    needs_service: bool,
    command: &'static str,
    verify: &'static str,
) -> FixAction {
    FixAction {
        id,
        os,
        title,
        description,
        needs_service,
        command,
        verify,
    }
}

pub const ACTIONS: &[FixAction] = &[
    a(
        "restart_service",
        Os::Linux,
        "Перезапустить службу",
        "systemctl restart: служба остановится и запустится снова, текущие соединения оборвутся.",
        true,
        "sudo -n /usr/bin/systemctl restart {service} && /usr/bin/systemctl --no-pager -n 15 status {service}",
        "failed_units",
    ),
    a(
        "vacuum_journal",
        Os::Linux,
        "Сжать системный журнал до 500 МБ",
        "journalctl --vacuum-size=500M: удаляет самые старые архивные журналы systemd.",
        false,
        "sudo -n /usr/bin/journalctl --vacuum-size=500M; df -hP /var",
        "disk",
    ),
    a(
        "rotate_logs",
        Os::Linux,
        "Принудительно ротировать логи",
        "logrotate -f: текущие логи переименуются и сожмутся по правилам /etc/logrotate.d; службы переоткроют файлы.",
        false,
        "sudo -n /usr/sbin/logrotate -f /etc/logrotate.conf; df -hP /var/log",
        "disk",
    ),
    a(
        "restart_service",
        Os::Windows,
        "Перезапустить службу",
        "Restart-Service: служба остановится и запустится снова; зависимые службы не трогаются.",
        true,
        "Restart-Service -Name '{service}' -PassThru | Select-Object Name,Status",
        "services",
    ),
    a(
        "start_service",
        Os::Windows,
        "Запустить остановленную службу",
        "Start-Service: запускает службу, если она остановлена.",
        true,
        "Start-Service -Name '{service}' -PassThru | Select-Object Name,Status",
        "services",
    ),
];

pub fn action(os: Os, id: &str) -> Option<&'static FixAction> {
    ACTIONS.iter().find(|a| a.os == os && a.id == id)
}

/// What exactly will run where — shown in the confirmation dialog and used to run.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct FixPlan {
    pub host: String,
    pub target: String,
    pub action: String,
    pub title: String,
    pub description: String,
    pub service: Option<String>,
    /// The command on the host (Linux) or the ScriptBlock (Windows).
    pub command: String,
    /// `ssh hermes-diag@web-01` / `WinRM dc-01 → HermesFix`.
    pub via: String,
}

/// Check every lock and build the command; nothing runs here.
pub fn plan(host: &Host, action_id: &str, service: Option<&str>) -> Result<FixPlan> {
    host.validate()?;
    if !host.fix {
        return Err(CoreError::Other(format!(
            "fixes are not allowed on {} (inventory: fix = false)",
            host.name
        )));
    }
    let act = action(host.os, action_id)
        .ok_or_else(|| CoreError::Other(format!("unknown action `{action_id}` for this OS")))?;
    let via = match (host.access, host.os) {
        (Access::Ssh, Os::Linux) => format!("ssh {}@{}", host.ssh_user(), host.target()),
        (Access::Winrm, Os::Windows) => format!("WinRM {} → {FIX_JEA}", host.target()),
        _ => {
            return Err(CoreError::Other(format!(
                "{} has no access configured for its OS",
                host.name
            )));
        }
    };
    let service = match (
        act.needs_service,
        service.map(str::trim).filter(|s| !s.is_empty()),
    ) {
        (true, None) => return Err(CoreError::Other("choose a service".into())),
        (true, Some(s)) => {
            if !host.fix_services.iter().any(|x| x == s) {
                return Err(CoreError::Other(format!(
                    "`{s}` is not in fix_services of {}",
                    host.name
                )));
            }
            Some(s.to_string())
        }
        (false, _) => None,
    };
    let command = act
        .command
        .replace("{service}", service.as_deref().unwrap_or(""));
    Ok(FixPlan {
        host: host.name.clone(),
        target: host.target().to_string(),
        action: act.id.into(),
        title: act.title.into(),
        description: act.description.into(),
        service,
        command,
        via,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct FixReport {
    pub plan: FixPlan,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub output: String,
    #[ts(type = "number")]
    pub duration_ms: u64,
    /// The check after the fix (the action's `verify` probe).
    pub verify: Option<DiagReport>,
}

pub fn winrm_fix_script(host: &Host, command: &str) -> String {
    format!(
        "$ErrorActionPreference='Stop'; Invoke-Command -ComputerName '{target}' -ConfigurationName '{FIX_JEA}' -ScriptBlock {{ {command} }} | Format-List | Out-String -Width 160",
        target = host.target(),
    )
}

/// Run a confirmed plan on the host, then the verify probe. Logged to `fix.jsonl`.
pub async fn run(layout: &Layout, host: &Host, plan: FixPlan, by: &str) -> FixReport {
    let started = Instant::now();
    let result = match host.os {
        Os::Linux => diag::ssh_exec(layout, host, &plan.command, FIX_TIMEOUT).await,
        Os::Windows => {
            diag::powershell_exec(&winrm_fix_script(host, &plan.command), FIX_TIMEOUT).await
        }
    };
    let (ok, exit_code, output) = match result {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n");
            let err = String::from_utf8_lossy(&o.stderr);
            if !err.trim().is_empty() {
                text.push('\n');
                text.push_str(err.trim());
            }
            (
                o.status.success(),
                o.status.code(),
                diag::clip_output(text.trim(), 6000),
            )
        }
        Err(e) => (false, None, e.to_string()),
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    let verify = match action(host.os, &plan.action) {
        Some(act) => {
            tokio::time::sleep(VERIFY_DELAY).await;
            Some(diag::run(layout, host, &[act.verify]).await)
        }
        None => None,
    };
    let audit = json!({
        "ts_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "by": by,
        "host": plan.host,
        "target": plan.target,
        "action": plan.action,
        "service": plan.service,
        "command": plan.command,
        "ok": ok,
        "exit_code": exit_code,
        "duration_ms": duration_ms,
        "output": diag::clip_output(&output, 2000),
    });
    if let Err(e) = crate::wazuh::append_jsonl(layout, "fix.jsonl", &audit) {
        log::warn!("fix audit: {e}");
    }
    log::info!(
        "fix {} on {} ({}): {} in {duration_ms} ms",
        plan.action,
        plan.host,
        plan.service.as_deref().unwrap_or("-"),
        if ok { "ok" } else { "failed" }
    );
    FixReport {
        plan,
        ok,
        exit_code,
        output,
        duration_ms,
        verify,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(os: Os, access: Access) -> Host {
        Host {
            name: "web-01".into(),
            address: "10.0.0.5".into(),
            os,
            access,
            fix: true,
            fix_services: vec!["nginx".into(), "php8.2-fpm".into()],
            ..Default::default()
        }
    }

    #[test]
    fn plan_builds_the_exact_command() {
        let p = plan(
            &host(Os::Linux, Access::Ssh),
            "restart_service",
            Some("nginx"),
        )
        .unwrap();
        assert_eq!(
            p.command,
            "sudo -n /usr/bin/systemctl restart nginx && /usr/bin/systemctl --no-pager -n 15 status nginx"
        );
        assert_eq!(p.via, "ssh hermes-diag@10.0.0.5");
        let p = plan(&host(Os::Linux, Access::Ssh), "vacuum_journal", None).unwrap();
        assert!(p.service.is_none() && p.command.contains("--vacuum-size=500M"));
        let w = Host {
            fix_services: vec!["W3SVC".into()],
            ..host(Os::Windows, Access::Winrm)
        };
        let p = plan(&w, "restart_service", Some("W3SVC")).unwrap();
        assert!(winrm_fix_script(&w, &p.command).contains(
            "-ConfigurationName 'HermesFix' -ScriptBlock { Restart-Service -Name 'W3SVC'"
        ));
    }

    #[test]
    fn every_lock_is_checked() {
        let h = host(Os::Linux, Access::Ssh);
        // Service outside the host's list, or missing.
        assert!(plan(&h, "restart_service", Some("sshd")).is_err());
        assert!(plan(&h, "restart_service", Some("nginx; reboot")).is_err());
        assert!(plan(&h, "restart_service", None).is_err());
        // Unknown action / action of the other OS.
        assert!(plan(&h, "rm_rf", None).is_err());
        assert!(plan(&h, "start_service", Some("nginx")).is_err());
        // Fixes off, no access.
        let off = Host {
            fix: false,
            ..h.clone()
        };
        assert!(plan(&off, "vacuum_journal", None).is_err());
        let none = Host {
            access: Access::None,
            ..h
        };
        assert!(plan(&none, "vacuum_journal", None).is_err());
    }

    #[test]
    fn catalogue_is_consistent() {
        for (i, x) in ACTIONS.iter().enumerate() {
            assert!(!ACTIONS[..i].iter().any(|y| y.os == x.os && y.id == x.id));
            assert!(diag::probe(x.os, x.verify).is_some(), "{} verify", x.id);
            assert_eq!(x.needs_service, x.command.contains("{service}"), "{}", x.id);
            if x.os == Os::Linux {
                // Every privileged step goes through sudo -n with an absolute path.
                assert!(x.command.starts_with("sudo -n /"), "{}", x.id);
            }
        }
    }
}
