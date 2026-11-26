use anyhow::{anyhow, Context, Result};
use std::io::Read;
use std::path::PathBuf;

#[cfg(feature = "whisper")]
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub const MODEL_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

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

/// Whisper transcription with a lazily loaded model, cached across calls.
pub struct Transcriber {
    model_path: PathBuf,
    language: Option<String>,
    #[cfg(feature = "whisper")]
    ctx: Option<WhisperContext>,
}

#[cfg(not(feature = "whisper"))]
pub struct Transcriber {
    model_path: PathBuf,
    language: Option<String>,
}

impl Transcriber {
    /// Resolve (and if needed download) the ggml model, then build a Transcriber.
    pub fn new(model: &str, language: &str) -> Result<Self> {
        let model = if model.is_empty() { "base" } else { model };
        if !known_model(model) {
            return Err(anyhow!(
                "unknown whisper model {model:?}; pick one of: tiny, base, small, medium, large-v3"
            ));
        }
        let path = model_cache_dir().join(format!("ggml-{model}.bin"));
        if !path.exists() {
            ensure_model_downloaded(model, &path)?;
        }
        let language = if language.is_empty() {
            None
        } else {
            Some(language.to_string())
        };
        Ok(Self {
            model_path: path,
            language,
            #[cfg(feature = "whisper")]
            ctx: None,
        })
    }

    #[cfg(feature = "whisper")]
    /// Load the whisper model eagerly so the first record is instant.
    pub fn preload(&mut self) -> Result<()> {
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
    pub fn preload(&mut self) -> Result<()> {
        Ok(())
    }

    #[cfg(feature = "whisper")]
    pub fn transcribe(&mut self, pcm16: &[u8], sample_rate: u32) -> Result<String> {
        anyhow::ensure!(sample_rate == 16000, "whisper expects 16 kHz audio");
        let samples: Vec<f32> = pcm16
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect();

        if self.ctx.is_none() {
            let ctx = WhisperContext::new_with_params(
                self.model_path.to_string_lossy().as_ref(),
                WhisperContextParameters::default(),
            )
            .context("failed to load whisper model")?;
            self.ctx = Some(ctx);
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
    pub fn transcribe(&mut self, _pcm16: &[u8], _sample_rate: u32) -> Result<String> {
        Err(anyhow!(
            "built without the `whisper` feature; rebuild with: cargo build --features whisper"
        ))
    }
}

/// Download the ggml model from HuggingFace, showing progress.
pub fn ensure_model_downloaded(model: &str, dest: &PathBuf) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let url = format!("{MODEL_BASE_URL}/ggml-{model}.bin");
    let tmp = dest.with_extension("part");
    println!("  ↓ downloading whisper model {model:?} to {}", dest.display());
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(600))
        .call()
        .map_err(|e| anyhow!("model download failed: {e}"))?;
    let mut file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    let mut reader = resp.into_reader();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0usize;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buffer[..n])?;
        total += n;
        if total % (4 * 1024 * 1024) < buffer.len() {
            println!("  … {:.0} MB", total as f64 / 1_048_576.0);
        }
    }
    std::fs::rename(&tmp, dest).context("finalizing model download")?;
    println!("  ✓ {:.0} MB", total as f64 / 1_048_576.0);
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
}
