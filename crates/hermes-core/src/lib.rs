//! Core logic of the Hermes shell. No Tauri dependency: everything here is
//! testable with plain `cargo test` and reused by the Tauri commands.

pub mod autostart;
pub mod bootstrap;
pub mod chat;
pub mod cli;
pub mod control;
pub mod error;
pub mod events;
pub mod gateway;
pub mod hermes_config;
pub mod ollama;
pub mod paths;
pub mod secrets;
pub mod setup;
pub mod status;
pub mod supervisor;
pub mod supervisor_chat;

pub use error::{CoreError, Result};
pub use paths::Layout;
