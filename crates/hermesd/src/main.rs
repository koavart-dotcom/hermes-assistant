//! hermesd — Hermes Assistant without a window: the hermes-core supervisor in
//! server mode (Hermes Agent install, gateway watchdog, control API on
//! 127.0.0.1:8645). On Windows it runs as the service «Hermes Assistant Server».
//!
//! ```text
//! hermesd run                     foreground, logs to the console too
//! hermesd install [--account A]   (admin) copy to Program Files, register and start the service
//! hermesd uninstall               (admin) stop and remove the service, data stays
//! hermesd status                  service state, paths, control API health
//! hermesd service                 entry point for the Service Control Manager
//! ```

mod daemon;
mod logger;
#[cfg(windows)]
mod winsvc;

use std::path::PathBuf;
use std::sync::Arc;

use hermes_core::Layout;
use hermes_core::server;
use hermes_core::supervisor::Supervisor;

pub fn log_dir(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("logs")
}

/// Logger + supervisor for `run` and `service`.
pub fn prepare(console: bool) -> Result<(Arc<Supervisor>, PathBuf), String> {
    let root = server::server_root();
    let layout = server::layout(&root);
    let dir = log_dir(&layout);
    logger::init(&dir, console).map_err(|e| format!("log dir {}: {e}", dir.display()))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let res = daemon::resource_root(&exe)
        .ok_or_else(|| format!("bundled resources not found near {}", exe.display()))?;
    let sup = daemon::build_supervisor(&res, &root)?;
    Ok((sup, dir))
}

fn usage() -> ! {
    eprintln!(
        "usage: hermesd run | install [--account DOMAIN\\user] [--interactive] [--pause] | uninstall | status"
    );
    std::process::exit(2)
}

fn run_foreground() -> Result<(), String> {
    let (sup, dir) = prepare(true)?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let cancel = tokio_util::sync::CancellationToken::new();
        let stop = cancel.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            stop.cancel();
        });
        daemon::run(sup, dir, cancel).await;
    });
    Ok(())
}

fn status() -> Result<(), String> {
    let root = server::server_root();
    let layout = server::layout(&root);
    #[cfg(windows)]
    println!("service:     {}", winsvc::query()?);
    println!("data root:   {}", root.display());
    println!("hermes home: {}", layout.hermes_home.display());
    println!(
        "hermes:      {}",
        if layout.bootstrap_marker().exists() {
            "installed"
        } else {
            "not installed"
        }
    );
    println!(
        "token:       {}",
        hermes_core::control::token_path(&layout).display()
    );
    println!(
        "log:         {}",
        log_dir(&layout).join(logger::FILE_NAME).display()
    );
    let url = format!(
        "http://127.0.0.1:{}{}/health",
        daemon::control_port(),
        hermes_core::control::BASE_PATH
    );
    let health = std::net::TcpStream::connect(("127.0.0.1", daemon::control_port()))
        .map(|_| "listening")
        .unwrap_or("not listening");
    println!("control api: {url} ({health})");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    #[cfg_attr(not(windows), allow(unused_variables))]
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let result = match args.first().map(String::as_str) {
        Some("run") => run_foreground(),
        Some("status") => status(),
        #[cfg(windows)]
        Some("service") => winsvc::dispatch(),
        #[cfg(windows)]
        Some("install") => winsvc::install(winsvc::InstallOptions {
            account: value("--account"),
            interactive: flag("--interactive"),
        }),
        #[cfg(windows)]
        Some("uninstall") => winsvc::uninstall(),
        #[cfg(not(windows))]
        Some("service" | "install" | "uninstall") => {
            Err("the Windows service commands work only on Windows; use `hermesd run`".into())
        }
        _ => usage(),
    };
    let code = match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("hermesd: {e}");
            1
        }
    };
    if flag("--pause") {
        eprintln!("\nНажмите Enter, чтобы закрыть окно…");
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
    }
    std::process::exit(code);
}
