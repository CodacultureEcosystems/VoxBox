use anyhow::Result;

use crate::brain::{self, Decision};
use crate::capture::{have_arecord, Recorder, Recording};
use crate::config::Config;
use crate::executor;
use crate::stt::Transcriber;
use crate::tts::Speaker;

pub struct VoxBox {
    config: Config,
    speaker: Speaker,
    text_only: bool,
    transcriber: Option<Transcriber>,
    recorder: Recorder,
}

const BANNER: &str = r#"
 ██╗   ██╗  ██████╗  ██╗  ██╗  ██████╗  ██╗  ██╗
 ██║   ██║ ██╔═══██╗ ╚██╗██╔╝ ██╔═══██╗ ╚██╗██╔╝
 ██║   ██║ ██║   ██║  ╚███╔╝  ██║   ██║  ╚███╔╝
 ╚██╗ ██╔╝ ██║   ██║  ██╔██╗  ██║   ██║  ██╔██╗
  ╚████╔╝  ╚██████╔╝ ██╔╝ ██╗ ╚██████╔╝ ██╔╝ ██╗
   ╚═══╝    ╚═════╝  ╚═╝  ╚═╝  ╚═════╝  ╚═╝  ╚═╝
  voice-first shell for Linux   (type 'exit' to quit)
"#;

impl VoxBox {
    pub fn new(config: Config, text_only: bool, no_tts: bool) -> Self {
        let cfg_tts = config.tts.clone();
        let speaker = if no_tts {
            Speaker::new("none", "", false).expect("silent speaker")
        } else {
            Speaker::new(&cfg_tts.engine, &cfg_tts.voice, cfg_tts.enabled)
                .expect("tts speaker")
        };
        let audio = config.audio.clone();
        let recorder = Recorder {
            sample_rate: audio.sample_rate,
            silence_ms: audio.silence_ms,
            max_seconds: audio.max_seconds,
            threshold: audio.threshold,
            frame_ms: 30,
        };
        Self {
            config,
            speaker,
            text_only,
            transcriber: None,
            recorder,
        }
    }

    fn listen(&mut self) -> Result<Recording> {
        let rec = self.recorder.record()?;
        Ok(rec)
    }

    fn transcribe(&mut self, rec: &Recording) -> Result<String> {
        if self.transcriber.is_none() {
            let stt = &self.config.stt;
            self.transcriber = Some(Transcriber::with_engine(
                crate::stt::engine_from(&stt.engine),
                &stt.model,
                &stt.language,
            )?);
        }
        self.transcriber
            .as_mut()
            .expect("transcriber")
            .transcribe(&rec.pcm, rec.sample_rate)
    }

    pub fn run(&mut self) -> Result<()> {
        println!("{BANNER}");
        let mode = if self.text_only {
            "TEXT MODE (--text)".to_string()
        } else if have_arecord() {
            "voice via arecord".to_string()
        } else {
            anyhow::bail!(
                "no audio backend (arecord missing); install alsa-utils or run with --text"
            );
        };
        println!("  mode: {mode}");
        self.speaker.speak("VoxBox shell ready.");
        loop {
            let prompt = if self.text_only { "  command> " } else { "  [Enter=speak, or type] > " };
            let line = match read_line(prompt) {
                Some(l) => l,
                None => {
                    println!();
                    break;
                }
            };
            let line = line.trim().to_string();
            let text = if line.is_empty() && !self.text_only {
                println!("  ...listening (speak, then stay quiet to stop)");
                let rec = match self.listen() {
                    Ok(r) => r,
                    Err(e) => {
                        println!("  [listen] {e:#}");
                        self.speaker.speak("I could not hear you.");
                        continue;
                    }
                };
                let text = match self.transcribe(&rec) {
                    Ok(t) => t,
                    Err(e) => {
                        println!("  [stt] {e:#}");
                        continue;
                    }
                };
                println!("  you> {text}");
                text
            } else if !line.is_empty() {
                line
            } else {
                continue;
            };

            let low = text.to_lowercase();
            if low == "exit" || low == "quit" || low == "q" {
                self.speaker.speak("Goodbye.");
                break;
            }
            self.handle(&text)?;
        }
        Ok(())
    }

    pub fn handle(&mut self, text: &str) -> Result<()> {
        let decision = brain::decide(text, &self.config.brain);
        match decision {
            Decision::Say { payload, .. } => {
                println!("  ai> {payload}");
                self.speaker.speak(&payload);
            }
            Decision::Shell {
                payload,
                speak_result,
                ..
            } => {
                if payload.trim().is_empty() {
                    self.speaker.speak("I don't know how to do that.");
                    return Ok(());
                }
                let feedback = self.config.ui.feedback.clone();
                let result = executor::run_command(
                    &payload,
                    &self.config.executor,
                    Some(&Self::confirm),
                );
                if result.ok {
                    let detail = brain::summarize_output(&result.output, 2);
                    if !result.output.is_empty() {
                        println!("  out> {}", truncate(&result.output, 2000));
                    }
                    if feedback == "full" && speak_result {
                        let mut msg = format!("ran: {}", result.command);
                        if !detail.is_empty() {
                            msg = format!("{msg}. output: {detail}");
                        }
                        self.speaker.speak(&msg);
                    }
                } else {
                    let err = if result.error.is_empty() {
                        format!("command failed with code {}", result.returncode)
                    } else {
                        result.error.clone()
                    };
                    println!("  err> {err}");
                    if feedback != "none" {
                        self.speaker.speak(&err);
                    }
                }
            }
        }
        Ok(())
    }

    fn confirm(command: &str) -> bool {
        eprint!("  (warning) run {command:?}? [y/N] ");
        use std::io::Write;
        let _ = std::io::stderr().flush();
        match read_line("") {
            Some(a) => matches!(a.trim(), "y" | "Y" | "yes"),
            None => false,
        }
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

fn read_line(prompt: &str) -> Option<String> {
    use std::io::BufRead;
    use std::io::Write;
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let n = std::io::stdin().lock().read_line(&mut line).ok()?;
    if n == 0 {
        return None;
    }
    Some(line)
}

/// Free entry point used by the `voxbox` binary.
pub fn run(config: Config, text_only: bool, no_tts: bool) -> Result<()> {
    let mut station = VoxBox::new(config, text_only, no_tts);
    station.run()
}
