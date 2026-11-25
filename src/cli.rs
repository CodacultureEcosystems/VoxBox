use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "voxbox",
    version,
    about = "Voice-first shell for Linux: speaks, listens, and runs commands on the whole PC."
)]
pub struct Args {
    /// Path to config.toml (default: ~/.config/voxbox/config.toml)
    #[arg(long)]
    pub config: Option<String>,

    /// Whisper model size: tiny|tiny.en|base|small|medium|large-v3
    #[arg(long)]
    pub model: Option<String>,

    /// Text-only mode: no microphone, type commands
    #[arg(long)]
    pub text: bool,

    /// Disable spoken replies
    #[arg(long)]
    pub no_tts: bool,

    /// Override the TTS engine for this session
    #[arg(long, value_enum)]
    pub tts_engine: Option<TtsEngineArg>,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsEngineArg {
    Auto,
    Piper,
    SpdSay,
    Espeak,
    None,
}

impl TtsEngineArg {
    pub fn as_str(&self) -> &'static str {
        match self {
            TtsEngineArg::Auto => "auto",
            TtsEngineArg::Piper => "piper",
            TtsEngineArg::SpdSay => "spd-say",
            TtsEngineArg::Espeak => "espeak",
            TtsEngineArg::None => "none",
        }
    }
}
