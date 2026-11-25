//! `vbd` — the VoxBox Station's resident dictation Being.
//!
//! Signature: Bsew@vbd (Seer / Witnessing / Worker at the VoxBox Station).
//! Wave in a hotkey, speak, and the text lands in whatever app is focused.
use clap::Parser;

use voxbox::config::Config;
use voxbox::dictate;

#[derive(Parser, Debug)]
#[command(
    name = "vbd",
    version,
    about = "VoxBox resident (Bsew@vbd): global-hotkey speech-to-text for every application on the desktop."
)]
struct VxdArgs {
    /// Path to config.toml
    #[arg(long)]
    config: Option<String>,

    /// Override the global hotkey (e.g. scrolllock, pause, f13)
    #[arg(long)]
    key: Option<String>,

    /// Whisper model size: tiny|base|small|medium|large-v3
    #[arg(long)]
    model: Option<String>,

    /// Disable desktop notifications
    #[arg(long)]
    no_notify: bool,
}

impl VxdArgs {
    fn apply(self, mut config: Config) -> Config {
        if let Some(key) = self.key {
            config.dictation.key = key;
        }
        if let Some(model) = self.model {
            config.stt.model = model;
        }
        if self.no_notify {
            config.dictation.notifications = false;
        }
        config
    }
}

fn main() -> std::process::ExitCode {
    let args = VxdArgs::parse();
    let config = Config::load(args.config.as_deref());
    let result = dictate::run(args.apply(config), false);
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vbd: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
