//! Where everything lives on disk. Windows is the primary target; the Unix
//! branch exists so the crate builds and tests on the Linux dev box.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// `%LOCALAPPDATA%\hermes` — Hermes Agent home (config, sessions, logs).
    pub hermes_home: PathBuf,
    /// `<hermes_home>\hermes-agent` — Hermes source checkout made by install.ps1.
    pub install_dir: PathBuf,
    /// `<hermes_home>\bin` — CLI launchers published by install.ps1.
    pub hermes_bin: PathBuf,
    /// Data of our shell (settings, logs, secrets fallback).
    pub app_data_dir: PathBuf,
    /// Ollama binaries extracted from the release zip.
    pub ollama_dir: PathBuf,
    /// `OLLAMA_MODELS` — large files, preferably on the data drive (D:).
    pub models_dir: PathBuf,
    /// Downloaded archives before extraction.
    pub downloads_dir: PathBuf,
}

impl Layout {
    /// Build a layout from explicit roots (used by tests and by `detect`).
    pub fn from_roots(local_app_data: &Path, data_root: &Path) -> Layout {
        let hermes_home = local_app_data.join("hermes");
        Layout {
            install_dir: hermes_home.join("hermes-agent"),
            hermes_bin: hermes_home.join("bin"),
            hermes_home,
            app_data_dir: local_app_data.join("Hermes"),
            ollama_dir: data_root.join("ollama"),
            models_dir: data_root.join("models"),
            downloads_dir: data_root.join("downloads"),
        }
    }

    /// Detect the layout for the current machine.
    ///
    /// Windows: `%LOCALAPPDATA%` for Hermes and the shell; `D:\hermes` for
    /// Ollama and models when drive D: exists (the main PC has 3.5 TB free
    /// there and almost nothing on C:), otherwise `%LOCALAPPDATA%\Hermes\data`.
    pub fn detect() -> Layout {
        let local = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
        let data_root = preferred_data_root(&local);
        Layout::from_roots(&local, &data_root)
    }

    pub fn hermes_cli(&self) -> PathBuf {
        if cfg!(windows) {
            let exe = self.hermes_bin.join("hermes.exe");
            if exe.exists() {
                return exe;
            }
            self.hermes_bin.join("hermes.cmd")
        } else {
            self.hermes_bin.join("hermes")
        }
    }

    pub fn ollama_exe(&self) -> PathBuf {
        self.ollama_dir
            .join(if cfg!(windows) { "ollama.exe" } else { "ollama" })
    }

    /// Marker written by install.ps1's `complete` stage.
    pub fn bootstrap_marker(&self) -> PathBuf {
        self.install_dir.join(".hermes-bootstrap-complete")
    }

    pub fn env_file(&self) -> PathBuf {
        self.hermes_home.join(".env")
    }

    pub fn config_file(&self) -> PathBuf {
        self.hermes_home.join("config.yaml")
    }
}

fn preferred_data_root(local_app_data: &Path) -> PathBuf {
    if cfg!(windows) {
        let d = PathBuf::from(r"D:\");
        if d.is_dir() {
            return d.join("hermes");
        }
    }
    local_app_data.join("Hermes").join("data")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_from_roots_builds_expected_paths() {
        let l = Layout::from_roots(Path::new("/lad"), Path::new("/data"));
        assert_eq!(l.hermes_home, Path::new("/lad/hermes"));
        assert_eq!(l.install_dir, Path::new("/lad/hermes/hermes-agent"));
        assert_eq!(l.hermes_bin, Path::new("/lad/hermes/bin"));
        assert_eq!(l.models_dir, Path::new("/data/models"));
        assert_eq!(l.bootstrap_marker(), Path::new("/lad/hermes/hermes-agent/.hermes-bootstrap-complete"));
        assert_eq!(l.env_file(), Path::new("/lad/hermes/.env"));
    }
}
