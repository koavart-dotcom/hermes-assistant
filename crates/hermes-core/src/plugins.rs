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

/// Copy a bundled skill directory (`SKILL.md` + helpers) to
/// `$HERMES_HOME/skills/<name>/`. Returns whether anything was written.
pub fn sync_skill_files(src_dir: &Path, hermes_home: &Path, name: &str) -> Result<bool> {
    if !src_dir.join("SKILL.md").exists() {
        return Err(CoreError::Other(format!(
            "skill source {} has no SKILL.md",
            src_dir.display()
        )));
    }
    let dest = hermes_home.join("skills").join(name);
    std::fs::create_dir_all(&dest)?;
    let mut changed = false;
    for entry in std::fs::read_dir(src_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let target = dest.join(entry.file_name());
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

/// Install every skill directory found under `skills_dir`.
pub fn sync_all_skills(skills_dir: &Path, hermes_home: &Path) -> Result<Vec<String>> {
    let mut updated = Vec::new();
    let Ok(rd) = std::fs::read_dir(skills_dir) else {
        return Ok(updated);
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() && p.join("SKILL.md").exists() {
            let name = entry.file_name().to_string_lossy().to_string();
            if sync_skill_files(&p, hermes_home, &name)? {
                updated.push(name);
            }
        }
    }
    Ok(updated)
}

/// Marker of the SOUL.md we ship; files without it were written by the user (or are the Hermes default).
pub const SOUL_MARKER: &str = "<!-- hermes-assistant:soul ";

/// Install our persona as `$HERMES_HOME/SOUL.md` when the current file is the Hermes default
/// ("You are Hermes Agent, built by Nous Research…"), missing, or an older unedited copy of ours.
/// A file the user changed (no marker, not the default) is left alone. Returns whether it wrote.
pub fn sync_soul(src: &Path, hermes_home: &Path) -> Result<bool> {
    let ours = std::fs::read_to_string(src)?;
    let dest = hermes_home.join("SOUL.md");
    let current = std::fs::read_to_string(&dest).unwrap_or_default();
    if current == ours {
        return Ok(false);
    }
    let is_default = current.trim().is_empty()
        || current.starts_with("You are Hermes Agent, built by Nous Research");
    // Ours with an OLDER version number: safe to replace (never downgrade a newer one).
    let version = |s: &str| -> u32 {
        s.strip_prefix(SOUL_MARKER)
            .and_then(|r| r.strip_prefix('v'))
            .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    };
    let is_ours_unedited = current.starts_with(SOUL_MARKER) && version(&current) < version(&ours);
    if !(is_default || is_ours_unedited) {
        return Ok(false);
    }
    std::fs::create_dir_all(hermes_home)?;
    std::fs::write(&dest, ours)?;
    Ok(true)
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

#[cfg(test)]
mod skill_tests {
    use super::*;

    #[test]
    fn soul_replaces_default_but_not_user_edits() {
        let root = std::env::temp_dir().join(format!("hermes-soul-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("SOUL.src.md");
        std::fs::write(&src, "<!-- hermes-assistant:soul v1 -->\nТы — Джарвис.").unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("SOUL.md"),
            "You are Hermes Agent, built by Nous Research. Be direct.",
        )
        .unwrap();
        assert!(sync_soul(&src, &home).unwrap());
        assert!(!sync_soul(&src, &home).unwrap());
        // A newer copy of ours is not downgraded.
        std::fs::write(
            home.join("SOUL.md"),
            "<!-- hermes-assistant:soul v2 -->\nновее",
        )
        .unwrap();
        assert!(!sync_soul(&src, &home).unwrap());
        std::fs::write(
            home.join("SOUL.md"),
            "<!-- hermes-assistant:soul v0 -->\nстарее",
        )
        .unwrap();
        assert!(sync_soul(&src, &home).unwrap());
        std::fs::write(home.join("SOUL.md"), "Мой собственный характер").unwrap();
        assert!(!sync_soul(&src, &home).unwrap());
        assert_eq!(
            std::fs::read_to_string(home.join("SOUL.md")).unwrap(),
            "Мой собственный характер"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn skills_are_copied_and_reported_once() {
        let root = std::env::temp_dir().join(format!("hermes-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("src").join("helpdesk-triage");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SKILL.md"), "# skill").unwrap();
        std::fs::create_dir_all(root.join("src").join("not-a-skill")).unwrap();
        let home = root.join("home");
        assert_eq!(
            sync_all_skills(&root.join("src"), &home).unwrap(),
            vec!["helpdesk-triage"]
        );
        assert!(home.join("skills/helpdesk-triage/SKILL.md").exists());
        assert!(
            sync_all_skills(&root.join("src"), &home)
                .unwrap()
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
