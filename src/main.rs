mod brain;
mod capture;
mod cli;
mod config;
mod executor;
mod shell;
mod stt;
#[cfg(test)]
mod tests;
mod tts;
mod vad;

use anyhow::Result;
use clap::Parser;

fn main() -> std::process::ExitCode {
    let args = cli::Args::parse();
    let mut config = config::Config::load(args.config.as_deref());
    if let Some(model) = &args.model {
        config.stt.model = model.clone();
    }
    if let Some(engine) = &args.tts_engine {
        config.tts.engine = engine.as_str().to_string();
        if *engine == cli::TtsEngineArg::None {
            config.tts.enabled = false;
        }
    }
    match run(config, args.text, args.no_tts) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("voxbox: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(config: config::Config, text_only: bool, no_tts: bool) -> Result<()> {
    let mut vs = shell::VoxBox::new(config, text_only, no_tts);
    vs.run()
}
