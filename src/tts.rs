use anyhow::Result;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsEngine {
    Piper,
    SpdSay,
    Espeak,
    None,
}

impl TtsEngine {
    pub fn detect(requested: &str) -> TtsEngine {
        match requested {
            "piper" => return TtsEngine::Piper,
            "spd-say" => return TtsEngine::SpdSay,
            "espeak" => return TtsEngine::Espeak,
            "none" => return TtsEngine::None,
            _ => {}
        }
        if which("piper") {
            TtsEngine::Piper
        } else if which("spd-say") {
            TtsEngine::SpdSay
        } else if which("espeak-ng") || which("espeak") {
            TtsEngine::Espeak
        } else {
            TtsEngine::None
        }
    }
}

pub struct Speaker {
    pub engine: TtsEngine,
    voice: String,
}

impl Speaker {
    pub fn new(engine: &str, voice: &str, enabled: bool) -> Result<Speaker> {
        let detector = if enabled { engine }  else { "none" };
        let engine = TtsEngine::detect(detector);
        Ok(Speaker {
            engine,
            voice: voice.to_string(),
        })
    }

    pub fn speak(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.engine == TtsEngine::None {
            return;
        }
        let _ = match self.engine {
            TtsEngine::None => Ok(()),
            TtsEngine::Piper => self.speak_piper(text),
            TtsEngine::SpdSay => self.speak_spd(text),
            TtsEngine::Espeak => self.speak_espeak(text),
        };
    }

    fn speak_piper(&self, text: &str) -> Result<()> {
        let wav = std::env::temp_dir().join(format!("voxbox-{}.wav", std::process::id()));
        let mut cmd = Command::new("piper");
        cmd.arg("--output_file").arg(&wav).stdout(Stdio::null()).stderr(Stdio::null());
        if !self.voice.is_empty() {
            cmd.args(["-m", &self.voice]);
        }
        use std::io::Write;
        let mut child = cmd.stdin(Stdio::piped()).spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
        }
        child.wait()?;
        play_wav(&wav)?;
        let _ = std::fs::remove_file(&wav);
        Ok(())
    }

    fn speak_spd(&self, text: &str) -> Result<()> {
        let mut cmd = Command::new("spd-say");
        cmd.arg("-w").stdout(Stdio::null()).stderr(Stdio::null());
        if !self.voice.is_empty() {
            cmd.args(["-o", &self.voice]);
        }
        cmd.arg(text).status()?;
        Ok(())
    }

    fn speak_espeak(&self, text: &str) -> Result<()> {
        let bin = if which("espeak-ng") { "espeak-ng" } else { "espeak" };
        let mut cmd = Command::new(bin);
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        if !self.voice.is_empty() {
            cmd.args(["-v", &self.voice]);
        }
        cmd.arg(text).status()?;
        Ok(())
    }
}

pub fn play_wav(path: &std::path::Path) -> Result<()> {
    let player = if which("paplay") {
        "paplay"
    } else if which("aplay") {
        "aplay"
    } else {
        return Ok(());
    };
    Command::new(player)
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    Ok(())
}

/// Plain "is this executable on PATH" without external crates.
pub fn which(name: &str) -> bool {
    if name.contains('/') {
        return std::path::Path::new(name).exists();
    }
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = std::path::Path::new(dir).join(name);
        if let Ok(meta) = std::fs::metadata(&candidate) {
            use std::os::unix::fs::PermissionsExt;
            if meta.is_file() && meta.permissions().mode() & 0o111 != 0 {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_none_without_binaries() {
        // In a minimal sandbox none of these may exist; "explicit none" must hold.
        assert_eq!(TtsEngine::detect("none"), TtsEngine::None);
    }

    #[test]
    fn sh_is_on_path() {
        assert!(which("sh"));
        assert!(!which("definitely-not-a-binary-xyz"));
    }
}
