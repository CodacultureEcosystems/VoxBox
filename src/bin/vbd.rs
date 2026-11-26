//! `vbd` — the VoxBox Station's resident dictation Being.
//!
//! Signature: Bsew@vbd (Seer / Witnessing / Worker at the VoxBox Station).
//! Wave in a hotkey, speak, and the text lands in whatever app is focused.
use std::sync::{atomic::AtomicBool, Arc};

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

    /// STT model: name (tiny|base|small|medium|large-v3) or local path
    #[arg(long)]
    model: Option<String>,

    /// STT engine: whisper (default) or parakeet
    #[arg(long)]
    engine: Option<String>,

    /// Disable desktop notifications
    #[arg(long)]
    no_notify: bool,

    /// Open the VoxBox Studio window (voice, modes, settings UI)
    #[arg(long)]
    window: bool,
}

impl VxdArgs {
    fn apply(self, mut config: Config) -> Config {
        if let Some(key) = self.key {
            config.dictation.key = key;
        }
        if let Some(model) = self.model {
            config.stt.model = model;
        }
        if let Some(engine) = self.engine {
            config.stt.engine = engine;
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
    if args.window {
        voxbox::studio::launch(voxbox::studio::StudioCtx {
            trigger: None,
            controls: None,
            open_flag: Arc::new(AtomicBool::new(true)),
            tile_open: Arc::new(AtomicBool::new(false)),
            key: config.dictation.key.clone(),
        });
        return std::process::ExitCode::SUCCESS;
    }
    let path = args.config.clone();
    let result = dictate::run(args.apply(config), path.as_deref());
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vbd: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
