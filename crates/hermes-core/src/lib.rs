//! Core logic of the Hermes shell. No Tauri dependency: everything here is
//! testable with plain `cargo test` and reused by the Tauri commands.

pub mod autostart;
pub mod bootstrap;
pub mod calendar;
pub mod calls;
pub mod chat;
pub mod cli;
pub mod control;
pub mod digest;
pub mod envkeys;
pub mod error;
pub mod events;
pub mod freescout;
pub mod gateway;
pub mod hermes_config;
pub mod jira;
pub mod mail;
pub mod models;
pub mod ollama;
pub mod paths;
pub mod plugins;
pub mod prefs;
pub mod quick;
pub mod secrets;
pub mod server;
pub mod setup;
pub mod status;
pub mod supervisor;
pub mod supervisor_chat;
pub mod supervisor_quick;
pub mod supervisor_settings;
pub mod supervisor_voice;
pub mod supervisor_wazuh;
pub mod voice;
pub mod vosk;
pub mod wazuh;
pub mod web;

pub use error::{CoreError, Result};
pub use paths::Layout;
