use anyhow::{anyhow, Context, Result};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::vad::EnergyVad;

pub struct Recording {
    pub pcm: Vec<u8>,
    pub sample_rate: u32,
}

pub struct Recorder {
    pub sample_rate: u32,
    pub silence_ms: u64,
    pub max_seconds: u64,
    pub threshold: f32,
    pub frame_ms: usize,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            silence_ms: 900,
            max_seconds: 20,
            threshold: 0.004,
            frame_ms: 30,
        }
    }
}

pub fn have_arecord() -> bool {
    Command::new("arecord")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

impl Recorder {
    /// Record one utterance via `arecord` piped into a local Rust VAD.
    /// Returns once trailing silence is detected (or the hard cap is hit).
    pub fn record(&self) -> Result<Recording> {
        self.record_with(None, false)
    }

    /// Record with external control:
    /// - `stop`: when set to true, the recording ends immediately
    ///   (push-to-talk release, constant-mode shutdown).
    /// - `force_speech`: keep every frame between start and stop instead of
    ///   trimming to VAD speech (hold-to-talk must not drop quiet words).
    pub fn record_with(
        &self,
        stop: Option<&AtomicBool>,
        force_speech: bool,
    ) -> Result<Recording> {
        if !have_arecord() {
            return Err(anyhow!(
                "arecord not found; install alsa-utils or run with --text"
            ));
        }
        let frame_bytes = self.sample_rate as usize * self.frame_ms / 1000 * 2;
        let mut child = Command::new("arecord")
            .args(["-q", "-f", "S16_LE", "-r", &self.sample_rate.to_string(), "-c", "1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to start arecord")?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("arecord stdout unavailable"))?;
        let vad = EnergyVad::new(self.threshold);
        let mut pcm: Vec<u8> = Vec::new();
        let mut quiet_frames = 0u32;
        let mut spoken = false;
        let quiet_limit = (self.silence_ms as usize / self.frame_ms).max(1) as u32;
        let max_bytes = self.sample_rate as usize * self.max_seconds as usize * 2;
        let mut buf = vec![0u8; frame_bytes];

        let mut keep_last: Vec<Vec<u8>> = Vec::new();
        loop {
            if let Some(flag) = stop {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
            }
            let n = read_exact_or_eof(&mut stdout, &mut buf)?;
            if n == 0 {
                break;
            }
            if pcm.len() >= max_bytes {
                break;
            }
            let frame: Vec<i16> = buf[..n]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            let speech = force_speech || vad.is_speech(&frame);
            if speech {
                quiet_frames = 0;
                spoken = true;
                pcm.extend_from_slice(&buf[..n]);
            } else if spoken && !force_speech {
                quiet_frames += 1;
                pcm.extend_from_slice(&buf[..n]);
                if quiet_frames >= quiet_limit {
                    break;
                }
            } else {
                keep_last.push(buf[..n].to_vec());
                if keep_last.len() > 3 {
                    keep_last.remove(0);
                }
            }
        }
        let _ = child.kill();
        let _ = child.wait();

        if !spoken {
            return Err(anyhow!("no speech detected"));
        }
        if !keep_last.is_empty() {
            let mut full = keep_last.concat();
            full.extend_from_slice(&pcm);
            pcm = full;
        }
        Ok(Recording {
            pcm,
            sample_rate: self.sample_rate,
        })
    }
}

fn read_exact_or_eof(stream: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e).context("reading arecord output"),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_exact_or_eof_short_reads() {
        struct Chop;
        impl Read for Chop {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if buf.len() >= 3 {
                    Ok(3)
                } else {
                    Ok(buf.len())
                }
            }
        }
        let mut buf = vec![0u8; 10];
        let n = read_exact_or_eof(&mut Chop, &mut buf).unwrap();
        assert_eq!(n, 10);
    }
}
