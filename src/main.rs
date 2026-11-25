//! The VoxBox Station workbench: interactive voice shell.
use clap::Parser;

use voxbox::cli::{Args, ModeArg};
use voxbox::config::Config;

fn main() -> std::process::ExitCode {
    let args = Args::parse();
    let mut config = Config::load(args.config.as_deref());
    if let Some(model) = &args.model {
        config.stt.model = model.clone();
    }
    if let Some(engine) = &args.tts_engine {
        config.tts.engine = engine.as_str().to_string();
        if *engine == voxbox::cli::TtsEngineArg::None {
            config.tts.enabled = false;
        }
    }
    let mode = args.mode.unwrap_or(ModeArg::Shell);
    let result = match mode {
        ModeArg::Shell => voxbox::shell::run(config, args.text, args.no_tts),
        ModeArg::Dictation | ModeArg::Resident => voxbox::dictate::run(config, args.no_tts),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("voxbox: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
