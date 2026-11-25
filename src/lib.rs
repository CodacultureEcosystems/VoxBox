//! VoxBox Station library: local STT, system-wide dictation (the `vbd`
//! resident Being), safe command execution, and spoken replies.
//! Rust-only, no Python anywhere.

pub mod brain;
pub mod capture;
pub mod cli;
pub mod config;
pub mod dictate;
pub mod executor;
pub mod inject;
pub mod keys;
pub mod shell;
pub mod stt;
#[cfg(test)]
pub mod tests;
pub mod trace;
pub mod tray;
pub mod tts;
pub mod vad;
