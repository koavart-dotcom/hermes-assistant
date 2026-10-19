//! macOS (launchd daemon) and Linux (systemd unit) service plumbing: the
//! `service` entry point and `install` / `uninstall`.
//!
//! `sudo hermesd install [--user NAME]` copies hermesd and the bundled
//! resources to [`server::unix_program_dir`], prepares the data root
//! ([`server::server_root`], mode 700, owned by the service user — the control
//! token is there) and registers a daemon that starts at boot, runs as that
//! user (never root) and is restarted when it fails. The service user also owns
//! the program folder so the self-update can swap the files.

use std::path::{Path, PathBuf};
use std::process::Command;

use hermes_core::server::{self, SERVICE_DISPLAY_NAME, UNIX_SERVICE_ID};

/// Exit code after a self-update: launchd (`KeepAlive.SuccessfulExit=false`)
/// and systemd (`Restart=on-failure`) start the new binary.
const EXIT_RESTART_FOR_UPDATE: i32 = 1;

pub struct InstallOptions {
    /// Account the daemon runs as; `None` = the user who ran `sudo`.
    pub user: Option<String>,
}

fn sh(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd:?}: {} {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn is_root() -> bool {
    sh(Command::new("id").arg("-u")).is_ok_and(|u| u == "0")
}

/// A plain account name: goes into the plist / unit and into `chown`.
pub fn valid_user(u: &str) -> bool {
    let mut chars = u.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && u.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c))
}

/// HOME of the service: inside the data root, so Hermes' installer, uv and
/// git never touch the real home of the account (on the Mac it has its own
/// Hermes in ~/.hermes and a `hermes` link in ~/.local/bin).
pub fn service_home(root: &Path) -> PathBuf {
    root.join("home")
}

const PATH: &str = "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin";

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn env_pairs(root: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("HOME", service_home(root).display().to_string()),
        ("PATH", PATH.into()),
        ("LANG", "en_US.UTF-8".into()),
        (hermes_core::secrets::FILE_ONLY_ENV, "1".into()),
    ]
}

/// `/Library/LaunchDaemons/ua.koavart.hermesd.plist`.
pub fn launchd_plist(exe: &Path, user: &str, root: &Path, log_dir: &Path) -> String {
    let env: String = env_pairs(root)
        .iter()
        .map(|(k, v)| format!("    <key>{k}</key><string>{}</string>\n", xml(v)))
        .collect();
    let out = xml(&log_dir.join("hermesd.stdout.log").display().to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{UNIX_SERVICE_ID}</string>
  <key>ProgramArguments</key>
  <array><string>{exe}</string><string>service</string></array>
  <key>UserName</key><string>{user}</string>
  <key>WorkingDirectory</key><string>{root}</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>ThrottleInterval</key><integer>30</integer>
  <key>ExitTimeOut</key><integer>40</integer>
  <key>AbandonProcessGroup</key><true/>
  <key>EnvironmentVariables</key>
  <dict>
{env}  </dict>
  <key>StandardOutPath</key><string>{out}</string>
  <key>StandardErrorPath</key><string>{out}</string>
</dict>
</plist>
"#,
        exe = xml(&exe.display().to_string()),
        user = xml(user),
        root = xml(&root.display().to_string()),
    )
}

/// `/etc/systemd/system/ua.koavart.hermesd.service`.
pub fn systemd_unit(exe: &Path, user: &str, root: &Path) -> String {
    let env: String = env_pairs(root)
        .iter()
        .map(|(k, v)| format!("Environment=\"{k}={v}\"\n"))
        .collect();
    format!(
        "[Unit]
Description={SERVICE_DISPLAY_NAME}
After=network-online.target
Wants=network-online.target

[Service]
User={user}
ExecStart={exe} service
WorkingDirectory={root}
{env}Restart=on-failure
RestartSec=30
TimeoutStopSec=40
# The Hermes gateway is our child in its own process group: an update restart
# of hermesd leaves it running.
KillMode=process

[Install]
WantedBy=multi-user.target
",
        exe = exe.display(),
        root = root.display(),
    )
}

fn plist_path() -> PathBuf {
    PathBuf::from(format!("/Library/LaunchDaemons/{UNIX_SERVICE_ID}.plist"))
}

fn unit_path() -> PathBuf {
    PathBuf::from(format!("/etc/systemd/system/{UNIX_SERVICE_ID}.service"))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

fn stop_existing() {
    if cfg!(target_os = "macos") {
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("system/{UNIX_SERVICE_ID}")])
            .output();
    } else {
        let _ = Command::new("systemctl")
            .args(["stop", UNIX_SERVICE_ID])
            .output();
    }
}

pub fn install(opts: InstallOptions) -> Result<(), String> {
    if !is_root() {
        return Err("run it with sudo: sudo ./hermesd install [--user NAME]".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let res =
        crate::daemon::resource_root(&exe).ok_or("bundled resources not found next to hermesd")?;
    let user = opts
        .user
        .or_else(|| std::env::var("SUDO_USER").ok())
        .filter(|u| !u.is_empty())
        .ok_or("which account should the service run as? --user NAME")?;
    if !valid_user(&user) || user == "root" {
        return Err(format!("account `{user}`: a regular user, not root"));
    }
    sh(Command::new("id").arg("-u").arg(&user)).map_err(|_| format!("no account `{user}`"))?;

    let root = server::server_root();
    let target = server::unix_program_dir();
    let layout = server::layout(&root);
    let log_dir = crate::log_dir(&layout);
    println!("Hermes Assistant Server — установка службы ({UNIX_SERVICE_ID}).");
    println!("  программа:   {}", target.display());
    println!("  данные:      {}", root.display());
    println!("  учётка:      {user}");
    println!(
        "  Control API: http://127.0.0.1:{}/control/v1",
        crate::daemon::control_port()
    );
    println!("При первом запуске служба скачает и поставит Hermes Agent (Python через uv,");
    println!("исходники Hermes, ~1–2 ГБ) в папку данных. Ollama и локальные модели не ставятся.");
    println!();

    stop_existing();

    // Files: hermesd + resources (skip when running from the target already).
    let target_exe = target.join(hermes_core::server_update::PACKAGE_EXE);
    if exe != target_exe {
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&target_exe);
        std::fs::copy(&exe, &target_exe).map_err(|e| format!("copy hermesd: {e}"))?;
        for d in server::RESOURCE_DIRS {
            let from = res.join("resources").join(d);
            let to = target.join("resources").join(d);
            if from.is_dir() {
                let _ = std::fs::remove_dir_all(&to);
                copy_dir(&from, &to).map_err(|e| format!("copy resources/{d}: {e}"))?;
            }
        }
    }
    sh(Command::new("chmod").arg("755").arg(&target_exe))?;
    std::fs::create_dir_all(service_home(&root)).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    sh(Command::new("chown").arg("-R").arg(&user).arg(&root))?;
    sh(Command::new("chmod").arg("700").arg(&root))?;
    // Self-update replaces the files as the service user.
    sh(Command::new("chown").arg("-R").arg(&user).arg(&target))?;

    if cfg!(target_os = "macos") {
        let plist = plist_path();
        std::fs::write(&plist, launchd_plist(&target_exe, &user, &root, &log_dir))
            .map_err(|e| format!("{}: {e}", plist.display()))?;
        sh(Command::new("chown").arg("root:wheel").arg(&plist))?;
        sh(Command::new("chmod").arg("644").arg(&plist))?;
        let _ = Command::new("launchctl")
            .args(["enable", &format!("system/{UNIX_SERVICE_ID}")])
            .output();
        sh(Command::new("launchctl")
            .arg("bootstrap")
            .arg("system")
            .arg(&plist))?;
    } else {
        let unit = unit_path();
        std::fs::write(&unit, systemd_unit(&target_exe, &user, &root))
            .map_err(|e| format!("{}: {e}", unit.display()))?;
        sh(Command::new("systemctl").arg("daemon-reload"))?;
        sh(Command::new("systemctl").args(["enable", "--now", UNIX_SERVICE_ID]))?;
    }
    println!("Готово: служба запущена от {user}.");
    println!(
        "Ключ Control API: {}",
        hermes_core::control::token_path(&layout).display()
    );
    println!(
        "Журнал: {}",
        log_dir.join(crate::logger::FILE_NAME).display()
    );
    println!(
        "Панель: http://127.0.0.1:{}/ui/",
        crate::daemon::control_port()
    );
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    if !is_root() {
        return Err("run it with sudo".into());
    }
    stop_existing();
    if cfg!(target_os = "macos") {
        let _ = std::fs::remove_file(plist_path());
    } else {
        let _ = Command::new("systemctl")
            .args(["disable", UNIX_SERVICE_ID])
            .output();
        let _ = std::fs::remove_file(unit_path());
        let _ = Command::new("systemctl").arg("daemon-reload").output();
    }
    println!(
        "Служба удалена. Программа осталась в {}, данные — в {}",
        server::unix_program_dir().display(),
        server::server_root().display()
    );
    Ok(())
}

pub fn query() -> String {
    if cfg!(target_os = "macos") {
        if !plist_path().exists() {
            return "not installed".into();
        }
        match Command::new("launchctl")
            .args(["print", &format!("system/{UNIX_SERVICE_ID}")])
            .output()
        {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .find_map(|l| l.trim().strip_prefix("state = ").map(str::to_string))
                .unwrap_or_else(|| "loaded".into()),
            _ => "installed, not loaded".into(),
        }
    } else if !unit_path().exists() {
        "not installed".into()
    } else {
        Command::new("systemctl")
            .args(["is-active", UNIX_SERVICE_ID])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| e.to_string())
    }
}

/// `hermesd service`, started by launchd / systemd. Stops on SIGTERM.
pub fn service() -> Result<(), String> {
    let (sup, log_dir) = crate::prepare(false)?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let restart = rt.block_on(async move {
        let cancel = tokio_util::sync::CancellationToken::new();
        let stop = cancel.clone();
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| e.to_string())?;
        tokio::spawn(async move {
            tokio::select! {
                _ = term.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
            log::info!("stop signal received");
            stop.cancel();
        });
        Ok::<bool, String>(crate::daemon::run(sup, log_dir, cancel).await)
    })?;
    drop(rt);
    if restart {
        std::process::exit(EXIT_RESTART_FOR_UPDATE);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_account_names_only() {
        assert!(valid_user("deka"));
        assert!(valid_user("_hermes"));
        assert!(valid_user("svc-hermes.1"));
        assert!(!valid_user(""));
        assert!(!valid_user("Deka"));
        assert!(!valid_user("a b"));
        assert!(!valid_user("x</string>"));
        assert!(!valid_user("-rf"));
    }

    #[test]
    fn plist_runs_as_the_user_and_restarts_after_failures() {
        let root = Path::new("/usr/local/var/hermes-server");
        let p = launchd_plist(
            Path::new("/usr/local/hermes-server/hermesd"),
            "deka",
            root,
            &root.join("ua.koavart.hermes/logs"),
        );
        assert!(p.contains("<key>UserName</key><string>deka</string>"));
        assert!(
            p.contains("<string>/usr/local/hermes-server/hermesd</string><string>service</string>")
        );
        assert!(p.contains("<key>SuccessfulExit</key><false/>"));
        assert!(p.contains("<key>HOME</key><string>/usr/local/var/hermes-server/home</string>"));
        assert!(p.contains("<key>HERMES_SECRETS_FILE_ONLY</key><string>1</string>"));
        assert!(p.contains("<key>AbandonProcessGroup</key><true/>"));
    }

    #[test]
    fn unit_runs_as_the_user_and_restarts_on_failure() {
        let u = systemd_unit(
            Path::new("/opt/hermes-server/hermesd"),
            "hermes",
            Path::new("/var/lib/hermes-server"),
        );
        assert!(u.contains("User=hermes\n"));
        assert!(u.contains("ExecStart=/opt/hermes-server/hermesd service\n"));
        assert!(u.contains("Restart=on-failure\n"));
        assert!(u.contains("KillMode=process\n"));
        assert!(u.contains("Environment=\"HOME=/var/lib/hermes-server/home\"\n"));
    }
}
