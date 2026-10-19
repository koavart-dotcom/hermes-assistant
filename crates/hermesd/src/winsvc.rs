//! Windows service plumbing: the SCM entry point and `install` / `uninstall`.
//!
//! `install` (run as administrator) copies hermesd and the bundled resources to
//! `%ProgramFiles%\Hermes Assistant Server` — the desktop install lives in a
//! user profile the service account cannot read — prepares the data root with
//! an ACL for SYSTEM, Administrators and the service account only (the control
//! token is there), and registers an auto-start service with restart-on-failure.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hermes_core::server::{self, SERVICE_DISPLAY_NAME, SERVICE_NAME};
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

/// Virtual service account: no password, gets «Log on as a service» by itself,
/// reaches the network as the computer account (DOMAIN\SERVER$).
pub const VIRTUAL_ACCOUNT: &str = r"NT SERVICE\HermesAssistant";

/// Resource folders the service needs (the Vosk DLLs and the app itself are not copied).
const RESOURCE_DIRS: &[&str] = &[
    "hermes",
    "hermes-plugins",
    "hermes-skills",
    "hermes-persona",
    "hermes-quick-commands",
];

define_windows_service!(ffi_service_main, service_main);

/// Called by `main` for `hermesd service`: hands the thread to the SCM.
pub fn dispatch() -> Result<(), String> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main).map_err(|e| e.to_string())
}

fn service_main(_args: Vec<OsString>) {
    if let Err(e) = run_service() {
        log::error!("service failed: {e}");
    }
}

fn status(state: ServiceState, accept: ServiceControlAccept, exit: u32) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accept,
        exit_code: ServiceExitCode::Win32(exit),
        checkpoint: 0,
        wait_hint: if state == ServiceState::StopPending {
            Duration::from_secs(30)
        } else {
            Duration::default()
        },
        process_id: None,
    }
}

fn run_service() -> Result<(), String> {
    let cancel = tokio_util::sync::CancellationToken::new();
    let stop = cancel.clone();
    let handle = service_control_handler::register(SERVICE_NAME, move |ev| match ev {
        ServiceControl::Stop | ServiceControl::Shutdown | ServiceControl::Preshutdown => {
            stop.cancel();
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })
    .map_err(|e| e.to_string())?;

    let report = |state, accept, exit| {
        if let Err(e) = handle.set_service_status(status(state, accept, exit)) {
            log::warn!("set_service_status: {e}");
        }
    };
    report(ServiceState::StartPending, ServiceControlAccept::empty(), 0);
    let prepared = crate::prepare(false);
    let (sup, log_dir) = match prepared {
        Ok(p) => p,
        Err(e) => {
            log::error!("{e}");
            report(ServiceState::Stopped, ServiceControlAccept::empty(), 1);
            return Err(e);
        }
    };
    report(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        0,
    );
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(crate::daemon::run(sup, log_dir, cancel));
    report(ServiceState::StopPending, ServiceControlAccept::empty(), 0);
    drop(rt);
    report(ServiceState::Stopped, ServiceControlAccept::empty(), 0);
    Ok(())
}

// ------------------------------------------------------------- install --

pub struct InstallOptions {
    /// `None` = [`VIRTUAL_ACCOUNT`]; `DOMAIN\user` asks for its password.
    pub account: Option<String>,
    /// Ask the account in the console (the installer starts us like this).
    pub interactive: bool,
}

fn program_dir() -> PathBuf {
    std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
        .join(SERVICE_DISPLAY_NAME)
}

fn prompt(question: &str) -> String {
    print!("{question}");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().lock().read_line(&mut s);
    s.trim().to_string()
}

pub fn is_builtin_account(account: &str) -> bool {
    let a = account.to_ascii_uppercase();
    a.starts_with(r"NT SERVICE\") || a.starts_with(r"NT AUTHORITY\") || a == "LOCALSYSTEM"
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

fn icacls(dir: &Path, account: &str) -> Result<(), String> {
    let out = Command::new("icacls")
        .arg(dir)
        .args(["/inheritance:r", "/grant:r"])
        .arg("*S-1-5-18:(OI)(CI)F") // SYSTEM
        .arg("*S-1-5-32-544:(OI)(CI)F") // Administrators
        .arg(format!("{account}:(OI)(CI)M"))
        .arg("/T")
        .arg("/Q")
        .output()
        .map_err(|e| format!("icacls: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "icacls {}: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stdout).trim()
        ));
    }
    Ok(())
}

fn wait_stopped(service: &windows_service::service::Service) {
    for _ in 0..60 {
        match service.query_status() {
            Ok(s) if s.current_state == ServiceState::Stopped => return,
            Err(_) => return,
            _ => std::thread::sleep(Duration::from_millis(500)),
        }
    }
}

pub fn install(opts: InstallOptions) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let res = crate::daemon::resource_root(&exe)
        .ok_or("bundled resources not found next to hermesd.exe")?;
    let root = server::server_root();
    let target = program_dir();

    println!("Hermes Assistant Server — установка службы Windows «{SERVICE_DISPLAY_NAME}».");
    println!("  программа:  {}", target.display());
    println!("  данные:     {}", root.display());
    println!(
        "  Control API: http://127.0.0.1:{}/control/v1",
        crate::daemon::control_port()
    );
    println!("При первом запуске служба скачает и поставит Hermes Agent (Python через uv, Git,");
    println!("исходники Hermes, ~1–2 ГБ) в папку данных. Ollama и локальные модели не ставятся.");
    println!();

    let mut account = opts.account.clone();
    if account.is_none() && opts.interactive {
        let a = prompt(&format!(
            "Учётная запись службы (Enter — {VIRTUAL_ACCOUNT}, или DOMAIN\\user): "
        ));
        if !a.is_empty() {
            account = Some(a);
        }
    }
    let account = account.unwrap_or_else(|| VIRTUAL_ACCOUNT.to_string());
    let password = if is_builtin_account(&account) {
        None
    } else {
        let p =
            rpassword::prompt_password(format!("Пароль {account}: ")).map_err(|e| e.to_string())?;
        Some(OsString::from(p))
    };

    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| format!("service manager (run as administrator): {e}"))?;
    let access = ServiceAccess::QUERY_STATUS
        | ServiceAccess::START
        | ServiceAccess::STOP
        | ServiceAccess::CHANGE_CONFIG;
    let existing = manager.open_service(SERVICE_NAME, access).ok();
    if let Some(s) = &existing {
        println!("Служба уже есть — останавливаю для обновления файлов…");
        let _ = s.stop();
        wait_stopped(s);
    }

    // Files: hermesd + resources into Program Files (skip when running from there).
    let target_exe = target.join("hermesd.exe");
    if exe != target_exe {
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        std::fs::copy(&exe, &target_exe).map_err(|e| format!("copy hermesd.exe: {e}"))?;
        for d in RESOURCE_DIRS {
            let from = res.join("resources").join(d);
            if from.is_dir() {
                copy_dir(&from, &target.join("resources").join(d))
                    .map_err(|e| format!("copy resources/{d}: {e}"))?;
            }
        }
    }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;

    let info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: target_exe.clone(),
        launch_arguments: vec![OsString::from("service")],
        dependencies: vec![],
        account_name: Some(OsString::from(&account)),
        account_password: password,
    };
    let service = match existing {
        Some(s) => {
            s.change_config(&info)
                .map_err(|e| format!("change service: {e}"))?;
            s
        }
        None => manager
            .create_service(&info, access)
            .map_err(|e| format!("create service: {e}"))?,
    };
    let _ = service.set_description(
        "Hermes Agent на сервере: разбор алертов, диагностика машин, Control API на 127.0.0.1",
    );
    let _ = service.set_delayed_auto_start(true);
    let restart = ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: Duration::from_secs(30),
    };
    let _ = service.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 3600)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![restart.clone(), restart.clone(), restart]),
    });
    let _ = service.set_failure_actions_on_non_crash_failures(true);

    // The virtual account exists only once the service does, so the ACL comes last.
    icacls(&root, &account)?;

    service
        .start(&[] as &[&OsStr])
        .map_err(|e| format!("start service: {e}"))?;
    println!();
    println!("Готово: служба «{SERVICE_DISPLAY_NAME}» запущена ({account}).");
    println!(
        "Ключ Control API: {}",
        hermes_core::control::token_path(&server::layout(&root)).display()
    );
    println!(
        "Журнал: {}",
        crate::log_dir(&server::layout(&root))
            .join(crate::logger::FILE_NAME)
            .display()
    );
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| format!("service manager (run as administrator): {e}"))?;
    let service = manager
        .open_service(
            SERVICE_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        )
        .map_err(|e| format!("open service: {e}"))?;
    let _ = service.stop();
    wait_stopped(&service);
    service
        .delete()
        .map_err(|e| format!("delete service: {e}"))?;
    println!(
        "Служба удалена. Данные остались в {}",
        server::server_root().display()
    );
    Ok(())
}

pub fn query() -> Result<String, String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| e.to_string())?;
    match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
        Ok(s) => s
            .query_status()
            .map(|st| format!("{:?}", st.current_state))
            .map_err(|e| e.to_string()),
        Err(_) => Ok("not installed".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_accounts_need_no_password() {
        assert!(is_builtin_account(VIRTUAL_ACCOUNT));
        assert!(is_builtin_account(r"nt authority\NetworkService"));
        assert!(!is_builtin_account(r"DEKA\svc-hermes"));
    }
}
