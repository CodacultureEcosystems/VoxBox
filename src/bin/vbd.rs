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

    /// Toggle one utterance on the running resident — or wake it if it is
    /// asleep. This is what the desktop icon and menu entry run.
    #[arg(long)]
    toggle: bool,
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

fn spawn_detached_resident() {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let exe = std::env::current_exe()
        .ok()
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("vbd"));
    let mut cmd = std::process::Command::new(exe);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid only detaches the child from the controlling terminal
    // so the resident survives its launcher closing. No other signals or
    // descriptors are touched.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = cmd.spawn();
}

fn main() -> std::process::ExitCode {
    let args = VxdArgs::parse();
    let config = Config::load(args.config.as_deref());
    if args.toggle {
        return match voxbox::ipc::send("toggle") {
            Ok(state) => {
                println!("VoxBox: {state}");
                std::process::ExitCode::SUCCESS
            }
            Err(_) => {
                println!("VoxBox resident is asleep — waking it");
                spawn_detached_resident();
                std::process::ExitCode::SUCCESS
            }
        };
    }
    if args.window {
        voxbox::studio::launch(voxbox::studio::StudioCtx {
            open_flag: Arc::new(AtomicBool::new(true)),
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
