use anyhow::{anyhow, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};

#[cfg(feature = "whisper")]
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
#[cfg(feature = "parakeet")]
use {parakeet_rs::ParakeetTDT as ParakeetModelRt, parakeet_rs::Transcriber as _};

pub const MODEL_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// The bundled Parakeet model (NVIDIA parakeet-tdt-0.6b-v3 ONNX export),
/// run on the pure-Rust burn backend — no ONNX Runtime needed.
pub const PARAKEET_DIR_NAME: &str = "parakeet-tdt-0.6b-v3";
pub const PARAKEET_BASE_URL: &str =
    "https://huggingface.co/altunenes/parakeet-rs/resolve/main/parakeet-tdt-0.6b-v3";
pub const PARAKEET_FILES: &[&str] = &[
    "encoder-model.onnx",
    "encoder-model.onnx.data",
    "decoder_joint-model.onnx",
    "vocab.txt",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttEngine {
    Whisper,
    Parakeet,
}

impl SttEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            SttEngine::Whisper => "whisper",
            SttEngine::Parakeet => "parakeet",
        }
    }
}

/// Canned lines Whisper invents for silence, hum, or background noise
/// (normalized: lowercase, punctuation stripped). If the whole utterance
/// reduces to one of these, nothing gets typed.
const NOISE_LINES: &[&str] = &[
    "applause",
    "background music",
    "background noise",
    "bye",
    "bye bye",
    "huh",
    "hmm",
    "hmm hmm",
    "laughter",
    "mm",
    "mm hmm",
    "music",
    "music is playing",
    "music playing",
    "oh my god",
    "playing music",
    "silence",
    "soft music",
    "subscribe",
    "thank you",
    "thank you for watching",
    "thank you very much",
    "thanks for watching",
    "um",
    "upbeat music",
    "uh",
    "what music is playing",
    "you",
];

/// Remove stage-direction groups — `[Music]`, `(upbeat music)`, `{...}`,
/// fullwidth brackets, and bare music notes (♪ ♫) — that whisper emits for
/// non-speech audio.
fn strip_stage_directions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for ch in text.chars() {
        match ch {
            '[' | '(' | '{' | '\u{3010}' | '\u{FF08}' => depth += 1,
            ']' | ')' | '}' | '\u{3011}' | '\u{FF09}' => depth = depth.saturating_sub(1),
            '\u{266A}' | '\u{266B}' | '\u{266C}' | '\u{266D}' => {}
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Lowercase, keep only letters/digits/spaces, collapse whitespace.
fn normalize_words(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out.trim().to_string()
}

/// Decide whether a transcription is hallucinated non-speech.
///
/// True when stripping stage directions leaves nothing, or when what remains
/// is exactly one of Whisper's canned noise lines ("[Music]  What music is
/// playing?" in a quiet house, "(upbeat music)", "Thank you.", ...).
pub fn is_hallucination(text: &str) -> bool {
    let norm = normalize_words(&strip_stage_directions(text));
    if norm.is_empty() {
        return true;
    }
    NOISE_LINES.contains(&norm.as_str())
}

/// Final cleanup applied to EVERY transcription before it is typed or
/// spoken: pure noise becomes empty (so nothing lands), and Whisper's
/// stage-direction groups ("[Music]", "(upbeat music)", \u{266a}) are
/// stripped from otherwise real speech — "[Music]  Hello?" must type as
/// "Hello?", brackets and all gone.
pub fn clean_transcript(text: &str) -> String {
    if is_hallucination(text) {
        return String::new();
    }
    strip_stage_directions(text).trim().to_string()
}

/// Parse an engine name; unknown names fall back to whisper.
pub fn engine_from(name: &str) -> SttEngine {
    match name.trim().to_lowercase().as_str() {
        "parakeet" => SttEngine::Parakeet,
        _ => SttEngine::Whisper,
    }
}

pub fn known_model(name: &str) -> bool {
    matches!(
        name,
        "tiny" | "tiny.en" | "base" | "base.en" | "small" | "small.en"
            | "medium" | "medium.en" | "large-v2" | "large-v3"
    )
}

pub fn model_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxbox")
}

/// Where the bundled Parakeet model lives (or will be downloaded to).
pub fn parakeet_model_dir() -> PathBuf {
    model_cache_dir().join(PARAKEET_DIR_NAME)
}

/// Resolve a whisper model reference: either a known name (cache, with
/// download) or a local path to a `ggml-*.bin` file used as-is.
pub fn resolve_whisper_model(model: &str) -> (PathBuf, bool) {
    let model = if model.is_empty() { "base" } else { model };
    let is_path = model.contains('/') || model.starts_with('~');
    if is_path {
        let expanded = if let Some(rest) = model.strip_prefix("~/") {
            dirs::home_dir().unwrap_or_default().join(rest)
        } else {
            PathBuf::from(model)
        };
        (expanded, true)
    } else {
        (model_cache_dir().join(format!("ggml-{model}.bin")), false)
    }
}

/// Resolve a parakeet model reference: empty (bundled model dir, with
/// download) or a local directory holding the ONNX export.
pub fn resolve_parakeet_model(model: &str) -> PathBuf {
    let model = model.trim();
    if model.is_empty() {
        return parakeet_model_dir();
    }
    let expanded = if let Some(rest) = model.strip_prefix("~/") {
        dirs::home_dir().unwrap_or_default().join(rest)
    } else {
        PathBuf::from(model)
    };
    expanded
}

/// STT transcription with a lazily loaded model, cached across calls.
/// When built with `--features parakeet`, the engine can also be
/// parakeet (NVIDIA parakeet via the pure-Rust burn backend).
pub struct Transcriber {
    engine: SttEngine,
    model_path: PathBuf,
    language: Option<String>,
    #[cfg(feature = "whisper")]
    ctx: Option<WhisperContext>,
    #[cfg(feature = "parakeet")]
    parakeet: Option<ParakeetModelRt>,
}

#[cfg(not(feature = "whisper"))]
pub struct Transcriber {
    engine: SttEngine,
    model_path: PathBuf,
    language: Option<String>,
    #[cfg(feature = "parakeet")]
    parakeet: Option<ParakeetModelRt>,
}

impl Transcriber {
    /// Resolve (and if needed download) the ggml model, then build a
    /// Whisper-backed Transcriber.
    pub fn new(model: &str, language: &str) -> Result<Self> {
        Self::with_engine(SttEngine::Whisper, model, language)
    }

    /// Build a Transcriber for the given engine and model reference.
    /// Local paths are accepted: for whisper a `ggml-*.bin` file, for
    /// parakeet a directory with the ONNX export.
    pub fn with_engine(engine: SttEngine, model: &str, language: &str) -> Result<Self> {
        let (path, is_custom) = match engine {
            SttEngine::Whisper => resolve_whisper_model(model),
            SttEngine::Parakeet => {
                let p = resolve_parakeet_model(model);
                let is_custom = !p
                    .file_name()
                    .map(|n| n == PARAKEET_DIR_NAME)
                    .unwrap_or(false);
                (p, is_custom)
            }
        };
        match engine {
            SttEngine::Parakeet => {
                #[cfg(not(feature = "parakeet"))]
                {
                    let _ = is_custom;
                    return Err(anyhow!(
                        "VoxBox was built without the `parakeet` feature; rebuild with: \
                         cargo build --features parakeet"
                    ));
                }
                #[cfg(feature = "parakeet")]
                ensure_parakeet_ready(&path, is_custom)?;
            }
            SttEngine::Whisper => {
                #[cfg(feature = "whisper")]
                {
                    if is_custom {
                        anyhow::ensure!(
                            path.exists(),
                            "whisper model path {} does not exist",
                            path.display()
                        );
                    } else if !known_model(model_if_empty(model)) {
                        return Err(anyhow!(
                            "unknown whisper model {model:?}; pick one of: tiny, base, small, \
                             medium, large-v3, or give a path to a ggml-*.bin file"
                        ));
                    } else if !path.exists() {
                        ensure_model_downloaded(model_if_empty(model), &path)?;
                    }
                }
                #[cfg(not(feature = "whisper"))]
                {
                    let _ = (is_custom, &path);
                    return Err(anyhow!(
                        "VoxBox was built without the `whisper` feature; rebuild with: \
                         cargo build --features whisper"
                    ));
                }
            }
        }
        let language = if language.is_empty() {
            None
        } else {
            Some(language.to_string())
        };
        Ok(Self {
            engine,
            model_path: path,
            language,
            #[cfg(feature = "whisper")]
            ctx: None,
            #[cfg(feature = "parakeet")]
            parakeet: None,
        })
    }

    pub fn engine(&self) -> SttEngine {
        self.engine
    }

    /// Load the STT model eagerly so the first record is instant.
    pub fn preload(&mut self) -> Result<()> {
        match self.engine {
            SttEngine::Whisper => self.preload_whisper(),
            SttEngine::Parakeet => self.preload_parakeet(),
        }
    }

    #[cfg(feature = "whisper")]
    fn preload_whisper(&mut self) -> Result<()> {
        if self.ctx.is_none() {
            let ctx = WhisperContext::new_with_params(
                self.model_path.to_string_lossy().as_ref(),
                WhisperContextParameters::default(),
            )
            .context("failed to load whisper model")?;
            self.ctx = Some(ctx);
        }
        Ok(())
    }

    #[cfg(not(feature = "whisper"))]
    fn preload_whisper(&mut self) -> Result<()> {
        Ok(())
    }

    #[cfg(feature = "parakeet")]
    fn preload_parakeet(&mut self) -> Result<()> {
        if self.parakeet.is_none() {
            let p = ParakeetModelRt::from_pretrained(&self.model_path, None)
                .map_err(|e| anyhow!("failed to load parakeet model: {e}"))?;
            self.parakeet = Some(p);
        }
        Ok(())
    }

    #[cfg(not(feature = "parakeet"))]
    fn preload_parakeet(&mut self) -> Result<()> {
        Ok(())
    }

    /// Transcribe 16 kHz mono S16_LE PCM into text.
    ///
    /// Hallucinated non-speech ("[Music]", "Thank you." in a silent room)
    /// is filtered and returned as an empty string.
    pub fn transcribe(&mut self, pcm16: &[u8], sample_rate: u32) -> Result<String> {
        let text = match self.engine {
            SttEngine::Parakeet => self.transcribe_parakeet(pcm16, sample_rate)?,
            SttEngine::Whisper => self.transcribe_whisper(pcm16, sample_rate)?,
        };
        Ok(clean_transcript(&text))
    }

    #[cfg(feature = "whisper")]
    fn transcribe_whisper(&mut self, pcm16: &[u8], sample_rate: u32) -> Result<String> {
        anyhow::ensure!(sample_rate == 16000, "whisper expects 16 kHz audio");
        let samples: Vec<f32> = pcm16
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect();

        if self.ctx.is_none() {
            self.preload_whisper()?;
        }
        let ctx = self.ctx.as_ref().expect("ctx loaded");
        let mut state = ctx.create_state().context("failed to create whisper state")?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(self.language.as_deref());
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // Noise hardening: don't condition on previous utterances (carries
        // hallucinations forward), suppress blank/non-speech tokens (♪, [Music]).
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);

        state.full(params, &samples[..])?;

        let n = state.full_n_segments();
        let mut text = String::new();
        for i in 0..n {
            if let Some(seg) = state.get_segment(i).map(|s| s.to_str_lossy().ok()) {
                text.push_str(&seg.unwrap_or_default());
                text.push(' ');
            }
        }
        Ok(text.trim().to_string())
    }

    #[cfg(not(feature = "whisper"))]
    fn transcribe_whisper(&mut self, _pcm16: &[u8], _sample_rate: u32) -> Result<String> {
        Err(anyhow!(
            "VoxBox was built without the `whisper` feature; rebuild with: \
             cargo build --features whisper"
        ))
    }

    #[cfg(feature = "parakeet")]
    fn transcribe_parakeet(&mut self, pcm16: &[u8], sample_rate: u32) -> Result<String> {
        anyhow::ensure!(sample_rate == 16000, "parakeet expects 16 kHz audio");
        if self.parakeet.is_none() {
            self.preload_parakeet()?;
        }
        let p = self.parakeet.as_mut().expect("parakeet loaded");
        let samples: Vec<f32> = pcm16
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect();
        let result = p
            .transcribe_samples(samples, sample_rate, 1, None)
            .map_err(|e| anyhow!("parakeet transcription failed: {e}"))?;
        Ok(result.text.trim().to_string())
    }

    #[cfg(not(feature = "parakeet"))]
    fn transcribe_parakeet(&mut self, _pcm16: &[u8], _sample_rate: u32) -> Result<String> {
        Err(anyhow!(
            "VoxBox was built without the `parakeet` feature; rebuild with: \
             cargo build --features parakeet"
        ))
    }
}

fn model_if_empty(model: &str) -> &str {
    if model.is_empty() {
        "base"
    } else {
        model
    }
}

/// Make sure the parakeet model files exist at `dir`; custom directories
/// must already hold the export, the bundled dir is downloaded on first use.
#[cfg(feature = "parakeet")]
pub fn ensure_parakeet_ready(dir: &Path, is_custom: bool) -> Result<()> {
    if is_custom {
        anyhow::ensure!(
            dir.is_dir(),
            "parakeet model directory {} does not exist",
            dir.display()
        );
        return Ok(());
    }
    if PARAKEET_FILES.iter().all(|f| dir.join(f).exists()) {
        return Ok(());
    }
    println!(
        "  ↓ downloading parakeet model (~2.5 GB, first use only) to {}",
        dir.display()
    );
    std::fs::create_dir_all(dir).context("creating parakeet model dir")?;
    for f in PARAKEET_FILES {
        let dest = dir.join(f);
        if dest.exists() {
            continue;
        }
        let url = format!("{PARAKEET_BASE_URL}/{f}");
        download_file(&url, &dest).with_context(|| format!("downloading {f}"))?;
    }
    Ok(())
}

/// Download a file to `dest` (`.part` staging), showing progress.
pub fn download_file(url: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("part");
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(1800))
        .call()
        .map_err(|e| anyhow!("download failed: {e}"))?;
    let mut file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    let mut reader = resp.into_reader();
    let mut buffer = [0u8; 512 * 1024];
    let mut total = 0usize;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buffer[..n])?;
        total += n;
        if total % (16 * 1024 * 1024) < buffer.len() {
            println!("  … {:.0} MB", total as f64 / 1_048_576.0);
        }
    }
    std::fs::rename(&tmp, dest).context("finalizing download")?;
    println!("  ✓ {} ({:.0} MB)", dest.display(), total as f64 / 1_048_576.0);
    Ok(())
}

/// Download the ggml model from HuggingFace, showing progress.
pub fn ensure_model_downloaded(model: &str, dest: &PathBuf) -> Result<()> {
    let url = format!("{MODEL_BASE_URL}/ggml-{model}.bin");
    println!(
        "  ↓ downloading whisper model {model:?} to {}",
        dest.display()
    );
    download_file(&url, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_models() {
        assert!(known_model("base"));
        assert!(known_model("large-v3"));
        assert!(!known_model("gigantic"));
    }

    #[test]
    fn rejects_unknown_model() {
        assert!(Transcriber::new("nope", "en").is_err());
    }

    #[test]
    fn engine_parsing() {
        assert_eq!(engine_from("whisper"), SttEngine::Whisper);
        assert_eq!(engine_from("Parakeet"), SttEngine::Parakeet);
        assert_eq!(engine_from("parakeet "), SttEngine::Parakeet);
        assert_eq!(engine_from("bogus"), SttEngine::Whisper);
        assert_eq!(engine_from(""), SttEngine::Whisper);
        assert_eq!(SttEngine::Parakeet.as_str(), "parakeet");
    }

    #[test]
    fn whisper_paths_names_and_custom() {
        let (p, custom) = resolve_whisper_model("base");
        assert!(!custom);
        assert!(p.ends_with("ggml-base.bin"));
        let (p, custom) = resolve_whisper_model("");
        assert!(!custom);
        assert!(p.ends_with("ggml-base.bin"));
        let (p, custom) = resolve_whisper_model("/opt/voices/ggml-custom.bin");
        assert!(custom);
        assert_eq!(p, PathBuf::from("/opt/voices/ggml-custom.bin"));
        let (p, custom) = resolve_whisper_model("~/models/ggml-mine.bin");
        assert!(custom);
        assert!(p.starts_with(dirs::home_dir().unwrap_or_default()));
    }

    #[test]
    fn parakeet_paths_default_and_custom() {
        let p = resolve_parakeet_model("");
        assert!(p.file_name().unwrap() == PARAKEET_DIR_NAME);
        let p = resolve_parakeet_model("/home/me/models/my-parakeet");
        assert_eq!(p, PathBuf::from("/home/me/models/my-parakeet"));
    }

    #[test]
    fn custom_whisper_model_path_bypasses_known_list() {
        let missing = std::env::temp_dir().join("voxbox-test-missing-model.bin");
        let _ = std::fs::remove_file(&missing);
        let err = Transcriber::with_engine(SttEngine::Whisper, &missing.to_string_lossy(), "en");
        assert!(err.is_err());
    }
}
