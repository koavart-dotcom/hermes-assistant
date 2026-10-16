//! Install our Hermes plugins (today: `hermes_permissions`) from the app's
//! bundled resources into `$HERMES_HOME/plugins/<name>/` and enable them.
//! Idempotent: files are compared byte-for-byte, so an app update that ships
//! a new plugin version is rolled out on the next start; `changed == true`
//! tells the caller the gateway must be restarted to load it.

use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::cli::run_hermes;
use crate::events::{EventSender, SetupEvent, emit};
use crate::{CoreError, Layout, Result};

pub const PERMISSIONS_PLUGIN: &str = "hermes_permissions";
pub const LOG_FILE: &str = "hermes_permissions.log";
pub const POLICY_FILE: &str = "hermes_permissions.json";

/// Copy `src_dir` (a plugin directory with `plugin.yaml` + `__init__.py`) to
/// `$HERMES_HOME/plugins/<name>/`. Returns whether anything was written.
pub fn sync_plugin_files(src_dir: &Path, hermes_home: &Path, name: &str) -> Result<bool> {
    let manifest = src_dir.join("plugin.yaml");
    if !manifest.exists() {
        return Err(CoreError::Other(format!(
            "plugin source {} has no plugin.yaml",
            src_dir.display()
        )));
    }
    let dest = hermes_home.join("plugins").join(name);
    std::fs::create_dir_all(&dest)?;
    let mut changed = false;
    for entry in std::fs::read_dir(src_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        // Ship only what the plugin needs at runtime.
        if !(name_str.ends_with(".py")
            || name_str.ends_with(".yaml")
            || name_str.ends_with(".json"))
            || name_str.starts_with("test_")
        {
            continue;
        }
        let target = dest.join(&file_name);
        let src_bytes = std::fs::read(&path)?;
        let same = std::fs::read(&target)
            .map(|b| b == src_bytes)
            .unwrap_or(false);
        if !same {
            std::fs::write(&target, &src_bytes)?;
            changed = true;
        }
    }
    Ok(changed)
}

pub fn log_path(layout: &Layout) -> PathBuf {
    layout.hermes_home.join(LOG_FILE)
}

pub fn policy_path(layout: &Layout) -> PathBuf {
    layout.hermes_home.join(POLICY_FILE)
}

/// Install (or refresh) the permissions plugin and enable it. Returns whether
/// files changed (caller restarts the gateway if it is running).
pub async fn install_permissions_plugin(
    layout: &Layout,
    src_dir: &Path,
    events: Option<&EventSender>,
    cancel: &CancellationToken,
) -> Result<bool> {
    let changed = sync_plugin_files(src_dir, &layout.hermes_home, PERMISSIONS_PLUGIN)?;
    if let Some(tx) = events {
        emit(
            tx,
            SetupEvent::Log {
                line: if changed {
                    format!("plugin {PERMISSIONS_PLUGIN}: files installed")
                } else {
                    format!("plugin {PERMISSIONS_PLUGIN}: up to date")
                },
            },
        );
    }
    // `hermes plugins enable` is idempotent; it only edits config.yaml.
    run_hermes(
        layout,
        &["plugins", "enable", PERMISSIONS_PLUGIN],
        events,
        cancel,
    )
    .await?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syncs_runtime_files_only_and_reports_changes() {
        let root = std::env::temp_dir().join(format!("hermes-plug-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("plugin.yaml"), "name: x\n").unwrap();
        std::fs::write(src.join("__init__.py"), "def register(ctx): pass\n").unwrap();
        std::fs::write(src.join("test_x.py"), "# not shipped\n").unwrap();
        std::fs::write(src.join("README.md"), "# not shipped\n").unwrap();
        let home = root.join("home");
        assert!(sync_plugin_files(&src, &home, "x").unwrap());
        let dest = home.join("plugins").join("x");
        assert!(dest.join("plugin.yaml").exists());
        assert!(dest.join("__init__.py").exists());
        assert!(!dest.join("test_x.py").exists());
        assert!(!dest.join("README.md").exists());
        assert!(!sync_plugin_files(&src, &home, "x").unwrap());
        std::fs::write(src.join("__init__.py"), "def register(ctx): return 1\n").unwrap();
        assert!(sync_plugin_files(&src, &home, "x").unwrap());
        assert!(sync_plugin_files(&root.join("missing"), &home, "x").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn repo_plugin_sources_are_shipped_as_is() {
        let src =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../hermes-plugins/hermes_permissions");
        assert!(src.join("plugin.yaml").exists(), "{}", src.display());
        assert!(src.join("__init__.py").exists());
    }
}
