//! Read-only diagnostics on a machine from the inventory (server mode, stage 3
//! of ADR 18.10.2026). The service — not the model — runs a fixed catalogue of
//! probes: Linux over OpenSSH (one connection, the service key, a read-only
//! user), Windows over PowerShell remoting into a JEA endpoint. Which probes run
//! is decided by a playbook from the alert; the outputs go into the analysis
//! prompt as untrusted data. Every run is written to `diag.jsonl`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Serialize;
use serde_json::json;
use tokio::process::Command;

use crate::cli::hide_console;
use crate::inventory::{Access, Host, Os};
use crate::wazuh::Alert;
use crate::{CoreError, Layout, Result};

/// One probe's output is clipped to this many characters.
pub const OUTPUT_MAX: usize = 3000;
/// The whole SSH session (all probes) / one WinRM probe.
const SSH_TIMEOUT: Duration = Duration::from_secs(90);
const WINRM_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Probe {
    pub id: &'static str,
    pub os: Os,
    pub title: &'static str,
    /// Linux: a shell command (no single quotes — it is wrapped in `sh -c '…'`);
    /// Windows: the body of `Invoke-Command -ScriptBlock { … }`.
    pub command: &'static str,
}

const fn p(id: &'static str, os: Os, title: &'static str, command: &'static str) -> Probe {
    Probe {
        id,
        os,
        title,
        command,
    }
}

/// The whole catalogue: nothing outside it is ever run.
pub const PROBES: &[Probe] = &[
    p("uptime", Os::Linux, "Нагрузка и память", "uptime; free -m"),
    p(
        "disk",
        Os::Linux,
        "Место и inode",
        "df -hP -x tmpfs -x devtmpfs -x squashfs -x overlay; echo; df -iP -x tmpfs -x devtmpfs -x squashfs -x overlay",
    ),
    p(
        "disk_top",
        Os::Linux,
        "Крупные папки",
        "du -xhd1 /var /var/log /home /tmp /opt 2>/dev/null | sort -h | tail -15",
    ),
    p(
        "procs",
        Os::Linux,
        "Процессы по CPU",
        "ps -eo pid,user,%cpu,%mem,etime,comm --sort=-%cpu | head -15",
    ),
    p(
        "failed_units",
        Os::Linux,
        "Упавшие службы",
        "systemctl --failed --no-legend --plain --no-pager",
    ),
    p(
        "journal_errors",
        Os::Linux,
        "Ошибки журнала за час",
        "journalctl -p err --since=-1h -n 60 --no-pager -q",
    ),
    p(
        "auth_log",
        Os::Linux,
        "Входы и SSH за час",
        "journalctl -u ssh -u sshd --since=-1h -n 60 --no-pager -q; last -n 10 -w 2>/dev/null",
    ),
    p(
        "oom",
        Os::Linux,
        "OOM за 6 часов",
        "journalctl -k --since=-6h --no-pager -q | grep -iE \"out of memory|killed process\" | tail -20",
    ),
    p(
        "net",
        Os::Linux,
        "Сеть и порты",
        "ss -s; echo; ss -ltn | head -30",
    ),
    p(
        "uptime",
        Os::Windows,
        "Загрузка и память",
        "Get-CimInstance -ClassName Win32_OperatingSystem | Select-Object CSName,LastBootUpTime,FreePhysicalMemory,TotalVisibleMemorySize",
    ),
    p(
        "disk",
        Os::Windows,
        "Место на дисках",
        "Get-CimInstance -ClassName Win32_LogicalDisk -Filter 'DriveType=3' | Select-Object DeviceID,Size,FreeSpace",
    ),
    p(
        "services",
        Os::Windows,
        "Автоматические службы не в работе",
        "Get-CimInstance -ClassName Win32_Service -Filter \"StartMode='Auto' AND State<>'Running'\" | Select-Object Name,DisplayName,State,ExitCode",
    ),
    p(
        "events",
        Os::Windows,
        "Ошибки System/Application за час",
        "Get-WinEvent -LogName System,Application -MaxEvents 40 -FilterXPath '*[System[(Level=1 or Level=2) and TimeCreated[timediff(@SystemTime) <= 3600000]]]' -ErrorAction SilentlyContinue | Select-Object TimeCreated,LogName,ProviderName,Id,Message",
    ),
    p(
        "procs",
        Os::Windows,
        "Процессы по CPU",
        "Get-Process | Sort-Object CPU -Descending | Select-Object -First 15 Name,Id,CPU,WorkingSet64",
    ),
    p(
        "auth",
        Os::Windows,
        "Неудачные входы и блокировки за час",
        "Get-WinEvent -LogName Security -MaxEvents 30 -FilterXPath '*[System[(EventID=4625 or EventID=4740) and TimeCreated[timediff(@SystemTime) <= 3600000]]]' -ErrorAction SilentlyContinue | Select-Object TimeCreated,Id,Message",
    ),
];

pub fn probe(os: Os, id: &str) -> Option<&'static Probe> {
    PROBES.iter().find(|p| p.os == os && p.id == id)
}

fn has(alert: &Alert, words: &[&str]) -> bool {
    let text = format!(
        "{} {}",
        alert.groups.join(" "),
        alert.description.to_lowercase()
    )
    .to_lowercase();
    words.iter().any(|w| text.contains(w))
}

/// Which probes to run for an alert: a base set per OS plus the ones its topic needs.
pub fn playbook(os: Os, alert: &Alert) -> Vec<&'static str> {
    let disk = has(
        alert,
        &["diskspace", "disk", "partition", "inode", "filesystem"],
    );
    let auth = has(
        alert,
        &[
            "authentication",
            "sshd",
            "invalid_login",
            "login",
            "logon",
            "brute",
        ],
    );
    let memory = has(alert, &["memory", "oom", "swap"]);
    let net = has(alert, &["port", "connection", "network", "firewall"]);
    let mut ids: Vec<&str> = match os {
        Os::Linux => vec!["uptime", "disk", "failed_units", "journal_errors"],
        Os::Windows => vec!["uptime", "disk", "services", "events"],
    };
    match os {
        Os::Linux => {
            if disk {
                ids.push("disk_top");
            }
            if auth {
                ids.push("auth_log");
            }
            if memory {
                ids.push("oom");
            }
            if net {
                ids.push("net");
            }
        }
        Os::Windows => {
            if auth {
                ids.push("auth");
            }
        }
    }
    if memory || !(disk || auth || net) {
        ids.push("procs");
    }
    ids.dedup();
    ids
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProbeResult {
    pub id: String,
    pub title: String,
    pub ok: bool,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagReport {
    pub host: String,
    pub target: String,
    pub access: String,
    pub duration_ms: u64,
    pub results: Vec<ProbeResult>,
    /// Connection-level failure (no route, auth, timeout); probes are then empty.
    pub error: Option<String>,
}

impl DiagReport {
    /// The `<DIAG>` block of the analysis prompt.
    pub fn prompt_text(&self) -> String {
        let mut s = format!(
            "Проверки только на чтение на {} ({}, {}):\n",
            self.host, self.target, self.access
        );
        if let Some(e) = &self.error {
            s.push_str(&format!("Подключиться не удалось: {e}\n"));
        }
        for r in &self.results {
            s.push_str(&format!(
                "--- {} ({}){}\n{}\n",
                r.title,
                r.id,
                if r.ok { "" } else { " [ошибка]" },
                r.output.trim_end()
            ));
        }
        s
    }

    pub fn summary(&self) -> String {
        match &self.error {
            Some(e) => format!("диагностика {}: не подключился ({e})", self.host),
            None => format!(
                "диагностика {}: {} проверок, {} с ошибкой",
                self.host,
                self.results.len(),
                self.results.iter().filter(|r| !r.ok).count()
            ),
        }
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("\n…(обрезано)");
    out
}

// ----------------------------------------------------------------- ssh --

pub fn ssh_dir(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("ssh")
}

pub fn ssh_key(layout: &Layout) -> PathBuf {
    ssh_dir(layout).join("id_ed25519")
}

/// Create the service SSH key on first use; returns the public key line to put
/// into `~hermes-diag/.ssh/authorized_keys` on Linux hosts.
pub async fn ensure_ssh_key(layout: &Layout) -> Result<String> {
    let key = ssh_key(layout);
    let pubkey = key.with_extension("pub");
    if !key.is_file() {
        std::fs::create_dir_all(ssh_dir(layout))?;
        let comment = format!(
            "hermes-diag@{}",
            std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "hermes".into())
        );
        let mut cmd = Command::new("ssh-keygen");
        cmd.args(["-q", "-t", "ed25519", "-N", "", "-C", &comment, "-f"])
            .arg(&key)
            .stdin(Stdio::null());
        hide_console(&mut cmd);
        let out = cmd.output().await?;
        if !out.status.success() {
            return Err(CoreError::Other(format!(
                "ssh-keygen: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    }
    Ok(std::fs::read_to_string(pubkey)?.trim().to_string())
}

/// One remote script for all probes, each output between markers.
pub fn ssh_script(probes: &[&Probe]) -> String {
    probes
        .iter()
        .map(|p| {
            format!(
                "echo \"@@BEGIN {id}\"; timeout 20 sh -c '{cmd}' 2>&1 | head -c 12000; echo \"@@END {id}\"",
                id = p.id,
                cmd = p.command
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn ssh_args(host: &Host, key: &Path, known_hosts: &Path, script: &str) -> Vec<String> {
    vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
        "-o".into(),
        format!("UserKnownHostsFile={}", known_hosts.display()),
        "-o".into(),
        "IdentitiesOnly=yes".into(),
        "-i".into(),
        key.display().to_string(),
        "-p".into(),
        host.ssh_port().to_string(),
        "-l".into(),
        host.ssh_user().to_string(),
        "--".into(),
        host.target().to_string(),
        script.to_string(),
    ]
}

/// Split the marked output back into probes; a probe without its markers failed.
pub fn parse_marked(output: &str, probes: &[&Probe]) -> Vec<ProbeResult> {
    probes
        .iter()
        .map(|p| {
            let begin = format!("@@BEGIN {}\n", p.id);
            let end = format!("@@END {}", p.id);
            let body = output.find(&begin).and_then(|b| {
                let rest = &output[b + begin.len()..];
                rest.find(&end).map(|e| &rest[..e])
            });
            ProbeResult {
                id: p.id.into(),
                title: p.title.into(),
                ok: body.is_some(),
                output: clip(body.unwrap_or("(нет вывода)"), OUTPUT_MAX),
            }
        })
        .collect()
}

async fn run_ssh(layout: &Layout, host: &Host, probes: &[&Probe]) -> Result<Vec<ProbeResult>> {
    let key = ssh_key(layout);
    if !key.is_file() {
        return Err(CoreError::Other(
            "no service SSH key yet (GET /control/v1/diag/ssh-key creates it)".into(),
        ));
    }
    let known = ssh_dir(layout).join("known_hosts");
    let script = ssh_script(probes);
    let mut cmd = Command::new("ssh");
    cmd.args(ssh_args(host, &key, &known, &script))
        .stdin(Stdio::null())
        .kill_on_drop(true);
    hide_console(&mut cmd);
    let out = tokio::time::timeout(SSH_TIMEOUT, cmd.output())
        .await
        .map_err(|_| {
            CoreError::Other(format!("ssh timed out after {}s", SSH_TIMEOUT.as_secs()))
        })??;
    let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    if !stdout.contains("@@BEGIN") {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(CoreError::Other(format!(
            "ssh exit {}: {}",
            out.status.code().unwrap_or(-1),
            clip(err.trim(), 400)
        )));
    }
    Ok(parse_marked(&stdout, probes))
}

// --------------------------------------------------------------- winrm --

/// PowerShell for one probe, run locally on the Windows server.
pub fn winrm_script(host: &Host, probe: &Probe) -> String {
    format!(
        "$ErrorActionPreference='Stop'; Invoke-Command -ComputerName '{target}' -ConfigurationName '{jea}' -ScriptBlock {{ {cmd} }} | Format-List | Out-String -Width 160",
        target = host.target(),
        jea = host.jea(),
        cmd = probe.command
    )
}

/// `powershell -EncodedCommand` takes UTF-16LE base64: no quoting problems.
pub fn encode_powershell(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

async fn run_winrm(host: &Host, probes: &[&Probe]) -> Result<Vec<ProbeResult>> {
    if !cfg!(windows) {
        return Err(CoreError::Other(
            "WinRM diagnostics run only from the Windows server".into(),
        ));
    }
    let mut results = Vec::new();
    for (i, p) in probes.iter().enumerate() {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
            .arg(encode_powershell(&winrm_script(host, p)))
            .stdin(Stdio::null())
            .kill_on_drop(true);
        hide_console(&mut cmd);
        let r = tokio::time::timeout(WINRM_TIMEOUT, cmd.output()).await;
        let (ok, output) = match r {
            Err(_) => (
                false,
                format!("timed out after {}s", WINRM_TIMEOUT.as_secs()),
            ),
            Ok(Err(e)) => (false, e.to_string()),
            Ok(Ok(o)) => {
                let text = if o.status.success() {
                    String::from_utf8_lossy(&o.stdout).to_string()
                } else {
                    String::from_utf8_lossy(&o.stderr).to_string()
                };
                (o.status.success(), text)
            }
        };
        // The first probe failing on the connection itself means the rest would too.
        if i == 0 && !ok {
            return Err(CoreError::Other(clip(output.trim(), 400)));
        }
        results.push(ProbeResult {
            id: p.id.into(),
            title: p.title.into(),
            ok,
            output: clip(output.trim(), OUTPUT_MAX),
        });
    }
    Ok(results)
}

// ----------------------------------------------------------------- run --

/// Run `ids` (or the alert's playbook when empty) on `host`; never fails —
/// connection problems are in `error`. Logged to `diag.jsonl`.
pub async fn run(layout: &Layout, host: &Host, ids: &[&str]) -> DiagReport {
    let started = Instant::now();
    let probes: Vec<&Probe> = ids.iter().filter_map(|id| probe(host.os, id)).collect();
    let access = match host.access {
        Access::Ssh => "ssh",
        Access::Winrm => "winrm",
        Access::None => "none",
    };
    let result = match (host.access, host.validate()) {
        (_, Err(e)) => Err(e),
        (Access::None, _) => Err(CoreError::Other("host has access: none".into())),
        _ if probes.is_empty() => Err(CoreError::Other("no known probes requested".into())),
        (Access::Ssh, _) => run_ssh(layout, host, &probes).await,
        (Access::Winrm, _) => run_winrm(host, &probes).await,
    };
    let (results, error) = match result {
        Ok(r) => (r, None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    let report = DiagReport {
        host: host.name.clone(),
        target: host.target().to_string(),
        access: access.into(),
        duration_ms: started.elapsed().as_millis() as u64,
        results,
        error,
    };
    let audit = json!({
        "ts_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "host": report.host,
        "target": report.target,
        "access": report.access,
        "probes": probes.iter().map(|p| p.id).collect::<Vec<_>>(),
        "ok": report.results.iter().filter(|r| r.ok).count(),
        "error": report.error,
        "duration_ms": report.duration_ms,
    });
    if let Err(e) = crate::wazuh::append_jsonl(layout, "diag.jsonl", &audit) {
        log::warn!("diag audit: {e}");
    }
    log::info!("{} in {} ms", report.summary(), report.duration_ms);
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn alert(groups: &[&str], description: &str) -> Alert {
        Alert::from_value(&json!({
            "rule": { "level": 10, "id": "1", "description": description, "groups": groups },
            "agent": { "name": "h" }
        }))
        .unwrap()
    }

    #[test]
    fn linux_commands_have_no_single_quotes() {
        for p in PROBES.iter().filter(|p| p.os == Os::Linux) {
            assert!(!p.command.contains('\''), "{} breaks sh -c '…'", p.id);
        }
    }

    #[test]
    fn catalogue_ids_are_unique_per_os() {
        for (i, a) in PROBES.iter().enumerate() {
            assert!(
                !PROBES[..i].iter().any(|b| b.os == a.os && b.id == a.id),
                "{}",
                a.id
            );
        }
    }

    #[test]
    fn playbooks_follow_the_alert_topic() {
        let disk = playbook(
            Os::Linux,
            &alert(&["ossec", "low_diskspace"], "Partition usage"),
        );
        assert!(disk.contains(&"disk_top") && !disk.contains(&"auth_log"));
        let ssh = playbook(
            Os::Linux,
            &alert(&["sshd", "authentication_failures"], "brute force"),
        );
        assert!(ssh.contains(&"auth_log"));
        let other = playbook(Os::Linux, &alert(&["syslog"], "Service crashed"));
        assert_eq!(
            other,
            vec!["uptime", "disk", "failed_units", "journal_errors", "procs"]
        );
        let win = playbook(
            Os::Windows,
            &alert(&["windows", "authentication_failed"], "Logon failure"),
        );
        assert!(win.contains(&"auth") && win.contains(&"events"));
        for os in [Os::Linux, Os::Windows] {
            for id in playbook(os, &alert(&["oom"], "out of memory")) {
                assert!(probe(os, id).is_some(), "{id}");
            }
        }
    }

    #[test]
    fn ssh_script_and_parsing_round_trip() {
        let probes: Vec<&Probe> = ["uptime", "disk"]
            .iter()
            .map(|i| probe(Os::Linux, i).unwrap())
            .collect();
        let script = ssh_script(&probes);
        assert!(script.contains("echo \"@@BEGIN uptime\"; timeout 20 sh -c 'uptime; free -m'"));
        let out = "@@BEGIN uptime\n 10:00 up 3 days\n@@END uptime\n@@BEGIN disk\n";
        let r = parse_marked(out, &probes);
        assert!(r[0].ok && r[0].output.contains("up 3 days"));
        assert!(!r[1].ok);
    }

    #[test]
    fn ssh_args_end_options_before_the_host() {
        let h = Host {
            name: "web-01".into(),
            address: "10.0.0.5".into(),
            access: Access::Ssh,
            ..Default::default()
        };
        let a = ssh_args(&h, Path::new("/k"), Path::new("/kh"), "uptime");
        let n = a.len();
        assert_eq!(&a[n - 3..], &["--", "10.0.0.5", "uptime"]);
        assert!(a.contains(&"BatchMode=yes".to_string()));
        assert!(a.contains(&"hermes-diag".to_string()));
    }

    #[test]
    fn winrm_script_targets_the_jea_endpoint() {
        let h = Host {
            name: "dc-01".into(),
            os: Os::Windows,
            access: Access::Winrm,
            ..Default::default()
        };
        let s = winrm_script(&h, probe(Os::Windows, "disk").unwrap());
        assert!(s.contains("Invoke-Command -ComputerName 'dc-01' -ConfigurationName 'HermesDiag'"));
        assert!(s.contains("Win32_LogicalDisk"));
        let enc = encode_powershell("a");
        assert_eq!(enc, "YQA=");
    }

    #[tokio::test]
    async fn unreachable_or_unconfigured_hosts_report_errors() {
        let tmp = std::env::temp_dir().join(format!("diag-{}", std::process::id()));
        let layout = Layout::from_roots(&tmp, &tmp.join("data"));
        let h = Host {
            name: "x".into(),
            ..Default::default()
        };
        let r = run(&layout, &h, &["uptime"]).await;
        assert!(r.error.unwrap().contains("access: none"));
        let h = Host {
            access: Access::Ssh,
            ..h
        };
        let r = run(&layout, &h, &["uptime"]).await;
        assert!(r.error.unwrap().contains("no service SSH key"));
        assert!(crate::wazuh::dir(&layout).join("diag.jsonl").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
