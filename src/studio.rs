//! VoxBox Studio — the desktop window UI.
//!
//! The WisprFlow shape: the resident lives in the tray, the desktop holds a
//! plain icon (a `.desktop` launcher, not a window), and Studio is the menu
//! UI — one window, opened from the tray or `vbd --window`, running in its
//! own process. It drives the resident over the IPC socket (`crate::ipc`).
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::stt::SttEngine;

pub static STATE: AtomicU8 = AtomicU8::new(0);
const IDLE: u8 = 0;
const RECORDING: u8 = 1;

const CATALOG_URL: &str = "https://rhasspy.github.io/piper-samples/";

/// Push the recording state to the Studio window (no-op without a host).
pub fn set_state(recording: bool) {
    STATE.store(
        if recording { RECORDING } else { IDLE },
        Ordering::Relaxed,
    );
}

/// Everything the window needs from its launcher. Studio always drives the
/// resident over the IPC socket; the resident only needs to know the
/// configured hotkey and when the window closed.
pub struct StudioCtx {
    pub open_flag: Arc<AtomicBool>,
    pub key: String,
}

/// Directory holding installed `.onnx` Piper voices.
pub fn voices_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("voxbox/voices")
}

/// Installed Piper voice names (file stems), sorted.
pub fn installed_voices(dir: &std::path::Path) -> Vec<String> {
    let mut voices: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "onnx").unwrap_or(false))
                .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    voices.sort();
    voices
}

/// Human label for a voice stem like `en_US-lessac-medium` → `EN/US — lessac (medium)`.
pub fn voice_label(name: &str) -> String {
    let parts: Vec<&str> = name.split('_').collect();
    if parts.len() < 2 {
        return name.to_string();
    }
    let mut label = parts[0].to_uppercase();
    let bits: Vec<&str> = parts[1].splitn(3, '-').collect();
    if let Some(country) = bits.first() {
        label.push_str(&format!("/{}", country.to_uppercase()));
    }
    label.push_str(" — ");
    if let Some(voice) = bits.get(1) {
        label.push_str(voice);
    }
    if let Some(quality) = bits.get(2) {
        label.push_str(&format!(" ({quality})"));
    }
    label
}

/// Current voice as `(dropdown index, summary)`. Index 0 is the system
/// default voice; index i+1 is `installed_voices()[i]`.
pub fn voice_pick(voices: &[String], voice_value: &str) -> (usize, String) {
    let stem = std::path::Path::new(voice_value)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let summary = if stem.is_empty() {
        "Piper default".to_string()
    } else {
        voice_label(&stem)
    };
    let idx = voices
        .iter()
        .position(|v| *v == stem)
        .map(|i| i + 1)
        .unwrap_or(0);
    (idx, summary)
}

/// Labels for the STT engine dropdown.
pub fn engine_label(e: SttEngine) -> &'static str {
    match e {
        SttEngine::Whisper => "Whisper — 99 languages, classic",
        SttEngine::Parakeet => "Parakeet — NVIDIA, fast with punctuation",
    }
}

pub fn engine_names() -> [SttEngine; 2] {
    [SttEngine::Whisper, SttEngine::Parakeet]
}

fn current_engine() -> SttEngine {
    crate::stt::engine_from(&current_config().stt.engine)
}

fn current_model() -> String {
    current_config().stt.model
}

fn current_config() -> crate::config::Config {
    let p = crate::config::config_path();
    let raw = std::fs::read_to_string(p).unwrap_or_default();
    let (c, _) = crate::config::Config::merge(&raw);
    c
}

struct StudioApp {
    key: String,
    status: String,
    last_poll: Instant,
    engine: SttEngine,
    model_field: String,
    voices: Vec<String>,
    labels: Vec<String>,
    selected: usize,
    tts_on: bool,
    test_status: Arc<Mutex<String>>,
    agents: Vec<crate::config::AgentConfig>,
    agent_selected: usize,
    agent_note: String,
}

impl StudioApp {
    fn new(c: &StudioCtx) -> Self {
        let (engine, voice_value, tts_on) = crate::config::read_tts();
        let dir = voices_dir();
        let voices = installed_voices(&dir);
        let (selected, summary) = voice_pick(&voices, &voice_value);
        let mut labels = vec!["Piper default (en_US-lessac)".to_string()];
        labels.extend(voices.iter().map(|v| voice_label(v)));
        let agents = crate::config::list_agents();
        let agent_note = if agents.is_empty() {
            format!("no agents found in {}", crate::config::agents_dir().display())
        } else {
            format!("{} agents available", agents.len())
        };
        let model_value = current_model();
        Self {
            key: c.key.clone(),
            status: crate::ipc::send("status").unwrap_or_else(|_| "resident not running".into()),
            last_poll: Instant::now(),
            engine: current_engine(),
            model_field: model_value,
            voices,
            labels,
            selected,
            tts_on,
            test_status: Arc::new(Mutex::new(format!(
                "tts engine: {engine} — voice: {summary}"
            ))),
            agents,
            agent_selected: 0,
            agent_note,
        }
    }

    fn voice_path(&self) -> String {
        if self.selected == 0 {
            String::new()
        } else {
            match self.voices.get(self.selected - 1) {
                Some(v) => format!("{}/{}.onnx", voices_dir().display(), v),
                None => String::new(),
            }
        }
    }

    fn launch_resident() {
        let home = std::env::var("HOME").unwrap_or_default();
        let bin = format!("{home}/.local/bin/vbd");
        let path = if std::path::Path::new(&bin).exists() {
            bin
        } else {
            "vbd".into()
        };
        let _ = std::process::Command::new(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    fn speak_sample(voice_path: &str) -> anyhow::Result<()> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let wav = std::env::temp_dir().join("voxbox-voice-test.wav");
        let mut cmd = Command::new("piper");
        cmd.arg("--output_file")
            .arg(&wav)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if !voice_path.is_empty() {
            cmd.args(["-m", voice_path]);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("piper: {e}"))?;
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all("Hello. I am VoxBox, the voice of this box.".as_bytes());
        }
        child
            .wait()
            .map_err(|e| anyhow::anyhow!("piper run: {e}"))?;
        crate::tts::play_wav(&wav)?;
        let _ = std::fs::remove_file(&wav);
        Ok(())
    }

    /// Ask the resident to reload its STT engine; harmless if it is asleep.
    fn request_reload(&self) {
        if let Err(e) = crate::ipc::send("reload") {
            *self.test_status.lock().expect("status") = format!(
                "resident not running — the change applies when vbd starts ({e:#})"
            );
        }
    }
}

impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new("VoxBox Studio")
                                    .heading()
                                    .color(egui::Color32::from_rgb(34, 197, 94)),
                            ),
                        );
                        ui.weak("Bsew@vbd — speech in, words out");
                    });
                    ui.add_space(10.0);

                    // Poll the resident's state over the IPC socket.
                    if self.last_poll.elapsed() >= Duration::from_millis(500) {
                        self.last_poll = Instant::now();
                        self.status = crate::ipc::send("status")
                            .unwrap_or_else(|_| "resident not running".into());
                    }
                    let banner = if self.status == "recording" {
                        "\u{25cf}  recording — speak, press again to land the text"
                    } else {
                        "\u{25cb}  idle — everything quiet"
                    };
                    ui.label(egui::RichText::new(banner).size(16.0));
                    ui.weak(&self.status);
                    ui.add_space(10.0);

                    ui.heading("Listening");
                    ui.add_space(4.0);
                    if ui
                        .add_enabled(true, egui::Button::new("Record now"))
                        .clicked()
                    {
                        match crate::ipc::send("toggle") {
                            Ok(state) => {
                                self.status = state;
                                self.last_poll = Instant::now();
                            }
                            Err(_) => {
                                Self::launch_resident();
                                self.status = "resident waking…".into();
                            }
                        }
                    }
                    ui.weak(format!(
                        "Hotkey: {} — change in ~/.config/voxbox/config.toml [dictation]",
                        self.key.to_uppercase()
                    ));
                    ui.add_space(10.0);

                    ui.heading("Conversation");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Voice mode — talk, it replies out loud").clicked() {
                            crate::tray::spawn_shell(&[]);
                        }
                        if ui.button("Text mode").clicked() {
                            crate::tray::spawn_shell(&["--text"]);
                        }
                    });
                    ui.add_space(10.0);

                    ui.heading("Voice");
                    ui.add_space(4.0);
                    let before = self.selected;
                    egui::ComboBox::from_label("")
                        .selected_text(self.labels[self.selected].clone())
                        .show_ui(ui, |ui| {
                            for (i, l) in self.labels.iter().enumerate() {
                                ui.selectable_value(&mut self.selected, i, l);
                            }
                        });
                    if self.selected != before {
                        let _ = crate::config::save_tts(None, Some(&self.voice_path()), None);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Speak a test line").clicked() {
                            let path = self.voice_path();
                            let status = Arc::clone(&self.test_status);
                            *status.lock().expect("status") = "speaking…".into();
                            let _ = std::thread::Builder::new()
                                .name("voxbox-voice-test".into())
                                .spawn(move || {
                                    let note = match Self::speak_sample(&path) {
                                        Ok(()) => "speaking: done".to_string(),
                                        Err(e) => format!("voice test failed: {e:#}"),
                                    };
                                    *status.lock().expect("status") = note;
                                });
                        }
                        if ui.button("more voices…").clicked() {
                            // egui's hyperlink opener can fail silently;
                            // xdg-open with a visible error cannot.
                            let status = Arc::clone(&self.test_status);
                            match std::process::Command::new("xdg-open")
                                .arg(CATALOG_URL)
                                .spawn()
                            {
                                Ok(_) => {
                                    *status.lock().expect("status") =
                                        "opening the voice catalog in your browser".into();
                                }
                                Err(e) => {
                                    *status.lock().expect("status") =
                                        format!("could not open browser: {e} — visit {CATALOG_URL}");
                                }
                            }
                        }
                    });
                    let response = ui.checkbox(&mut self.tts_on, "Speak replies out loud");
                    if response.changed() {
                        let _ = crate::config::save_tts(None, None, Some(self.tts_on));
                    }
                    ui.add_space(6.0);
                    let note = self
                        .test_status
                        .lock()
                        .map(|s| s.clone())
                        .unwrap_or_default();
                    ui.weak(note);
                    ui.add_space(10.0);

                    ui.heading("Speech engine");
                    ui.add_space(4.0);
                    let engines = engine_names();
                    let before_engine = self.engine;
                    egui::ComboBox::from_label("")
                        .selected_text(engine_label(self.engine))
                        .show_ui(ui, |ui| {
                            for e in engines {
                                ui.selectable_value(&mut self.engine, e, engine_label(e));
                            }
                        });
                    if self.engine != before_engine {
                        let _ = crate::config::save_stt(Some(self.engine.as_str()), None);
                        self.request_reload();
                        *self.test_status.lock().expect("status") = format!(
                            "engine → {} (warming in the background)",
                            self.engine.as_str()
                        );
                    }
                    ui.weak(
                        "Parakeet's model (~2.5 GB) downloads on first use; local paths work in the field below.",
                    );
                    ui.horizontal(|ui| {
                        let field_w = ui.available_width() - 70.0;
                        ui.add_sized(
                            [field_w, 22.0],
                            egui::TextEdit::singleline(&mut self.model_field)
                                .hint_text("leave empty for the bundled model, or a local path"),
                        );
                        if ui.button("Load").clicked() {
                            let m = self.model_field.trim().to_string();
                            let _ = crate::config::save_stt(None, Some(&m));
                            self.request_reload();
                            *self.test_status.lock().expect("status") =
                                "model loaded — warming in the background".into();
                        }
                    });
                    ui.weak(
                        "whisper: path to a ggml-*.bin file · parakeet: path to an ONNX folder \
                         (encoder-model.onnx + vocab.txt · tokenizer.json).",
                    );
                    ui.add_space(10.0);

                    ui.heading("AI agents");
                    ui.add_space(4.0);
                    if self.agents.is_empty() {
                        ui.weak(&self.agent_note);
                        ui.weak(format!(
                            "Add one file per agent in {} — e.g. local.toml with \
                             name, api_base, api_key, model.",
                            crate::config::agents_dir().display()
                        ));
                    } else {
                        let names: Vec<String> =
                            self.agents.iter().map(|a| a.name.clone()).collect();
                        let before_agent = self.agent_selected;
                        egui::ComboBox::from_label("")
                            .selected_text(names[self.agent_selected.min(names.len() - 1)].clone())
                            .show_ui(ui, |ui| {
                                for (i, n) in names.iter().enumerate() {
                                    ui.selectable_value(&mut self.agent_selected, i, n);
                                }
                            });
                        if self.agent_selected != before_agent {
                            let agents = self.agents.clone();
                            let idx = self.agent_selected.min(agents.len() - 1);
                            let agent = agents[idx].clone();
                            match crate::config::activate_agent(&agent) {
                                Ok(()) => {
                                    self.agent_note = format!(
                                        "agent {} active (brain routes there now)",
                                        agent.name
                                    );
                                }
                                Err(e) => self.agent_note = format!("activation failed: {e}"),
                            }
                        }
                    }
                    if ui.button("Rescan agents").clicked() {
                        self.agents = crate::config::list_agents();
                        self.agent_selected = 0;
                        self.agent_note = if self.agents.is_empty() {
                            "no agents found".into()
                        } else {
                            format!("{} agents available", self.agents.len())
                        };
                    }
                    ui.add_space(6.0);
                    ui.weak(&self.agent_note);

                    ui.add_space(10.0);
                    ui.weak("Desktop icon: click “VoxBox Talk” on your desktop to talk — \
                             the resident types into the focused app.");
                    ui.weak("Voice changes apply to the next Conversation; Listen uses no voice.");
                });
            });
        ui.ctx()
            .request_repaint_after(Duration::from_millis(200));
    }
}

/// eframe options whose event loop is allowed to live on a non-main thread.
///
/// winit panics if an `EventLoop` is created off the main thread unless the
/// platform extension trait's `with_any_thread(true)` is applied — which
/// killed the desktop tile (`voxbox-tile-spawn` thread) and the tray-opened
/// Studio window (`voxbox-studio` thread) with exactly that panic. This hook
/// sets the flag for both the X11 and Wayland backends.
fn native_opts(viewport: egui::ViewportBuilder) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport,
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(target_os = "linux")]
            {
                winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(builder, true);
                winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(builder, true);
            }
            #[cfg(not(target_os = "linux"))]
            let _ = builder;
        })),
        ..Default::default()
    }
}

/// Open the Studio window. Runs its own event loop; call from any thread.
pub fn launch(c: StudioCtx) {
    let app = StudioApp::new(&c);
    let opts = native_opts(
        egui::ViewportBuilder::default()
            .with_inner_size([460.0, 640.0])
            .with_min_inner_size([420.0, 520.0])
            .with_title("VoxBox Studio"),
    );
    let flag = Arc::clone(&c.open_flag);
    if let Err(e) = eframe::run_native(
        "VoxBox Studio",
        opts,
        Box::new(move |_cc| Ok(Box::new(app) as Box<dyn eframe::App>)),
    ) {
        eprintln!("  [studio] could not open: {e}");
    }
    flag.store(false, Ordering::Relaxed);
}

/// Current resident state as one word — the IPC `status` reply.
pub fn state_word() -> String {
    match STATE.load(Ordering::Relaxed) {
        RECORDING => "recording".to_string(),
        _ => "idle".to_string(),
    }
}
