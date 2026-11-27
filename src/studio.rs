//! VoxBox Studio — the desktop window UI.
//!
//! The WisprFlow shape: the resident lives in the tray, the desktop holds a
//! plain icon (a `.desktop` launcher, not a window), and Studio is the menu
//! UI — one window, opened from the tray or `vbd --window`, running in its
//! own process. It drives the resident over the IPC socket (`crate::ipc`).
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::dictate::{Controls, Signal};
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
                            *self.test_status.lock().expect("status") =
                                "voice shell opened in a new terminal window".into();
                        }
                        if ui.button("Text mode").clicked() {
                            crate::tray::spawn_shell(&["--text"]);
                            *self.test_status.lock().expect("status") =
                                "text shell opened in a new terminal window".into();
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
                        *self.test_status.lock().expect("status") =
                            format!("voice saved: {}", self.labels[self.selected]);
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
                        *self.test_status.lock().expect("status") =
                            format!("spoken replies: {}", if self.tts_on { "on" } else { "off" });
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
                    ui.add_space(6.0);
                    if ui.button("Show the desktop toggle icon").clicked() {
                        match crate::ipc::send("tile") {
                            Ok(_) => {
                                self.status = "toggle icon shown".into();
                                self.last_poll = Instant::now();
                            }
                            Err(_) => {
                                Self::launch_resident();
                                self.status = "resident waking…".into();
                            }
                        }
                    }
                    ui.weak("The toggle icon rides with the resident: click = talk · \
                             double-click = constant mode · drag = move · right-click = menu.");

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

// ------------------------------------------------- desktop toggle icon ---
//
// The WisprFlow Flow Bar equivalent: a small frameless always-on-top icon
// riding above the desktop. NOT a window — no title bar, no taskbar entry.
//   single click   → one utterance
//   double click   → constant mode on/off
//   hold + release → push-to-talk
//   drag           → move it anywhere
//   right-click    → Open Studio / Hide

/// Gesture thresholds for the desktop toggle icon.
pub const HOLD_MS: u64 = 350;
pub const DOUBLE_MS: u64 = 300;
/// Movement (px) beyond which a press is a drag, not a gesture.
pub const DRAG_PX: f32 = 6.0;
/// Side of the square toggle icon in pixels (the whole window surface).
pub const TILE_PX: f32 = 104.0;

/// What the tile wants done; `launch_tile` maps these onto the resident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileEvent {
    Toggle,
    DoubleClick,
    HoldStart,
    HoldEnd,
}

/// Everything the toggle icon needs from the resident (same process).
pub struct TileCtx {
    pub trigger: Option<mpsc::Sender<Signal>>,
    pub controls: Option<Arc<Controls>>,
    pub open_flag: Arc<AtomicBool>,
}

/// Pure pointer state machine for the tile gestures. `now` is injected so
/// tests can drive it deterministically.
#[derive(Debug, Default)]
pub struct TilePointer {
    press_at: Option<Instant>,
    hold_sent: bool,
    last_release: Option<Instant>,
    suppress_toggle: bool,
}

impl TilePointer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one frame of pointer state; returns the gesture (if any).
    pub fn step(
        &mut self,
        pressed_now: bool,
        released_now: bool,
        now: Instant,
        constant_on: bool,
    ) -> Option<TileEvent> {
        if pressed_now && self.press_at.is_none() {
            if let Some(prev) = self.last_release {
                if now.duration_since(prev) <= Duration::from_millis(DOUBLE_MS) {
                    self.last_release = None;
                    self.press_at = Some(now);
                    self.hold_sent = false;
                    self.suppress_toggle = true;
                    return Some(TileEvent::DoubleClick);
                }
            }
            self.press_at = Some(now);
            self.hold_sent = false;
            self.suppress_toggle = false;
            return None;
        }
        if pressed_now {
            if self.press_at.is_some()
                && !self.hold_sent
                && now.duration_since(self.press_at.expect("press known"))
                    >= Duration::from_millis(HOLD_MS)
            {
                self.hold_sent = true;
                return Some(TileEvent::HoldStart);
            }
            return None;
        }
        // Not pressed right now.
        if released_now {
            if let Some(start) = self.press_at.take() {
                if self.hold_sent || now.duration_since(start) >= Duration::from_millis(HOLD_MS) {
                    self.last_release = None;
                    return Some(TileEvent::HoldEnd);
                }
                if self.suppress_toggle {
                    self.suppress_toggle = false;
                } else {
                    self.last_release = Some(now);
                }
            }
            return None;
        }
        if self.press_at.is_some() {
            return None;
        }
        if let Some(rel) = self.last_release {
            if now.duration_since(rel) > Duration::from_millis(DOUBLE_MS) {
                self.last_release = None;
                return Some(TileEvent::Toggle);
            }
        }
        let _ = constant_on;
        None
    }
}

fn decode_png_rgba(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..w as usize * h as usize * 4].to_vec(),
        png::ColorType::Rgb => buf[..w as usize * h as usize * 3]
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    Some((w, h, rgba))
}

const LOGO_PNG: &[u8] = include_bytes!("../assets/voxbox.png");

struct TileApp {
    trigger: Option<mpsc::Sender<Signal>>,
    controls: Option<Arc<Controls>>,
    texture: Option<egui::TextureHandle>,
    pointer: TilePointer,
    was_pressed: bool,
    positioned: bool,
    pos: Option<egui::Pos2>,
    drag_dist: f32,
    menu_open: bool,
    last_event_note: String,
}

impl TileApp {
    fn note(&mut self, event: TileEvent) {
        self.last_event_note = match event {
            TileEvent::Toggle => "one utterance…".into(),
            TileEvent::DoubleClick => "double click — constant mode".into(),
            TileEvent::HoldStart => "hold — talk, release to stop".into(),
            TileEvent::HoldEnd => "released — landing text…".into(),
        };
    }
}

impl eframe::App for TileApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // The window surface itself is fully transparent — only the painted
        // icon below is visible. Combined with override-redirect the window
        // manager never draws a frame, shadow, or title bar on this surface:
        // it is pixels floating on the desktop, exactly like a conky widget
        // or a notification popup.
        ui.style_mut().visuals.panel_fill = egui::Color32::TRANSPARENT;

        if !self.positioned {
            let screen = ui.ctx().input(|i| i.viewport_rect());
            if screen.width() > 200.0 {
                let pos = egui::pos2(
                    screen.right() - TILE_PX - 24.0,
                    screen.bottom() - TILE_PX - 24.0,
                );
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                self.pos = Some(pos);
                self.positioned = true;
            }
        }

        let recording = STATE.load(Ordering::Relaxed) == RECORDING;
        let constant = self
            .controls
            .as_ref()
            .map(|c| c.constant())
            .unwrap_or(false);

        // One hit area: the whole little surface IS the icon.
        let full = ui.max_rect();
        let resp = ui.allocate_rect(full, egui::Sense::click_and_drag());

        // Drag-to-move: past DRAG_PX a press is a move, not a gesture.
        let delta = resp.drag_delta();
        let moved = self.was_pressed && delta.length() > 0.0;
        if moved {
            self.drag_dist += delta.length();
            if self.drag_dist > DRAG_PX {
                self.pointer = TilePointer::new(); // a drag is never a gesture
                if let Some(p) = self.pos.as_mut() {
                    *p += delta;
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::OuterPosition(*p));
                }
            }
        }

        let pressed_now = resp.is_pointer_button_down_on() || resp.dragged();
        let released_now = self.was_pressed && !pressed_now;
        if released_now {
            self.drag_dist = 0.0;
        }

        let event = self.pointer.step(pressed_now, released_now, Instant::now(), constant);
        self.was_pressed = pressed_now;
        if let Some(ev) = event {
            match ev {
                TileEvent::Toggle => {
                    let _ = self.trigger.as_ref().map(|t| t.send(Signal::Toggle));
                }
                TileEvent::DoubleClick => {
                    let next = !constant;
                    if let Some(c) = &self.controls {
                        c.constant.store(next, Ordering::Relaxed);
                    }
                    let _ = self
                        .trigger
                        .as_ref()
                        .map(|t| t.send(Signal::Constant(next)));
                }
                TileEvent::HoldStart => {
                    let _ = self.trigger.as_ref().map(|t| t.send(Signal::HoldStart));
                }
                TileEvent::HoldEnd => {
                    if let Some(c) = &self.controls {
                        c.hold_release.store(true, Ordering::Relaxed);
                    }
                }
            }
            self.note(ev);
        }

        // The icon: a rounded square centered in the surface, logo inside,
        // tinted by state. Green = recording, blue = constant mode.
        let icon = egui::Rect::from_center_size(
            full.center(),
            egui::vec2(TILE_PX - 10.0, TILE_PX - 10.0),
        );
        let tint = if recording {
            egui::Color32::from_rgb(120, 255, 140)
        } else if constant {
            egui::Color32::from_rgb(120, 200, 255)
        } else {
            egui::Color32::WHITE
        };
        let bg = if recording {
            egui::Color32::from_rgba_unmultiplied(16, 46, 26, 242)
        } else if constant {
            egui::Color32::from_rgba_unmultiplied(14, 32, 46, 242)
        } else {
            egui::Color32::from_rgba_unmultiplied(12, 24, 16, 235)
        };
        ui.painter().rect_filled(icon, 26.0, bg);
        ui.painter().rect_stroke(
            icon,
            26.0,
            egui::Stroke::new(2.0, tint.gamma_multiply(0.7)),
            egui::StrokeKind::Outside,
        );
        if let Some(tex) = &self.texture {
            ui.painter().image(
                tex.id(),
                icon.shrink(14.0),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                tint,
            );
        } else {
            ui.painter().text(
                icon.center(),
                egui::Align2::CENTER_CENTER,
                "VOX",
                egui::FontId::proportional(24.0),
                egui::Color32::from_rgb(34, 197, 94),
            );
        }
        let word = if recording {
            "listening"
        } else if constant {
            "constant"
        } else {
            "tap"
        };
        ui.painter().text(
            egui::pos2(icon.center().x, icon.bottom() - 10.0),
            egui::Align2::CENTER_CENTER,
            word,
            egui::FontId::proportional(10.0),
            tint.gamma_multiply(0.9),
        );

        resp.clone().on_hover_text(format!(
            "VoxBox toggle — {}\nsingle click: one utterance\ndouble click: constant mode\n\
             hold: push-to-talk\ndrag: move · right-click: menu",
            self.last_event_note
        ));

        // Right-click: a small menu floating inside the surface (it cannot
        // render outside the window, so it overlays the icon).
        let mut picked: Option<u8> = None;
        if resp.secondary_clicked() {
            self.menu_open = !self.menu_open;
        }
        if self.menu_open {
            egui::Area::new(egui::Id::unique("voxbox-tile-menu"))
                .fixed_pos(egui::pos2(full.left() + 8.0, full.top() + 8.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(88.0);
                        if ui.small_button("Studio").clicked() {
                            picked = Some(0);
                        }
                        if ui.small_button("Hide").clicked() {
                            picked = Some(1);
                        }
                    });
                });
        }
        match picked {
            Some(0) => {
                self.menu_open = false;
                crate::tray::open_studio();
            }
            Some(1) => {
                self.menu_open = false;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }

        ui.ctx()
            .request_repaint_after(Duration::from_millis(150));
    }
}

/// Open the desktop toggle icon (one at a time). Frameless: no title bar,
/// no taskbar entry — an icon, not a window.
pub fn launch_tile(c: TileCtx) {
    if c.open_flag
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let texture: Option<egui::TextureHandle> = None;
    let app = TileApp {
        trigger: c.trigger.clone(),
        controls: c.controls.clone(),
        texture,
        pointer: TilePointer::new(),
        was_pressed: false,
        positioned: false,
        pos: None,
        drag_dist: 0.0,
        menu_open: false,
        last_event_note: String::new(),
    };
    let flag = Arc::clone(&c.open_flag);
    let opts = native_opts(
        egui::ViewportBuilder::default()
            .with_inner_size([TILE_PX, TILE_PX])
            .with_resizable(false)
            .with_decorations(false)
            // Override-redirect: the X11 window manager (Cinnamon) never
            // manages this surface — no frame, no shadow, no title bar,
            // not in alt-tab. This is how conky/notification popups work.
            .with_override_redirect(true)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_window_type(egui::X11WindowType::Dock)
            .with_title("VoxBox"),
    );
    let result = eframe::run_native(
        "VoxBox tile",
        opts,
        Box::new(move |cc| {
            let mut app = app;
            if let Some((w, h, rgba)) = decode_png_rgba(LOGO_PNG) {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [w as usize, h as usize],
                    &rgba,
                );
                app.texture = Some(cc.egui_ctx.load_texture("voxbox-logo", image, Default::default()));
            }
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    );
    if let Err(e) = result {
        eprintln!("  [tile] could not open: {e}");
    }
    flag.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tile_tests {
    use super::*;

    fn base() -> Instant {
        Instant::now()
    }

    #[test]
    fn quick_tap_yields_one_toggle_after_the_double_click_window() {
        let mut p = TilePointer::new();
        let t0 = base();
        assert_eq!(p.step(true, false, t0, false), None);
        assert_eq!(p.step(false, true, t0 + Duration::from_millis(80), false), None);
        assert_eq!(
            p.step(
                false,
                false,
                t0 + Duration::from_millis(80 + DOUBLE_MS as u64 + 50),
                false
            ),
            Some(TileEvent::Toggle)
        );
    }

    #[test]
    fn double_click_yields_one_double_click_not_a_toggle() {
        let mut p = TilePointer::new();
        let t0 = base();
        p.step(true, false, t0, false);
        p.step(false, true, t0 + Duration::from_millis(80), false);
        assert_eq!(p.step(true, false, t0 + Duration::from_millis(200), false), Some(TileEvent::DoubleClick));
        p.step(false, true, t0 + Duration::from_millis(280), false);
        assert_eq!(
            p.step(false, false, t0 + Duration::from_millis(700), false),
            None,
            "second click of the pair must not become a late single-click Toggle"
        );
    }

    #[test]
    fn hold_crossing_the_threshold_starts_and_releases() {
        let mut p = TilePointer::new();
        let t0 = base();
        p.step(true, false, t0, false);
        assert_eq!(p.step(true, false, t0 + Duration::from_millis(100), false), None);
        assert_eq!(
            p.step(true, false, t0 + Duration::from_millis(HOLD_MS as u64 + 10), false),
            Some(TileEvent::HoldStart)
        );
        assert_eq!(
            p.step(false, true, t0 + Duration::from_millis(1200), false),
            Some(TileEvent::HoldEnd)
        );
    }

    #[test]
    fn slow_release_without_tick_still_counts_as_hold() {
        let mut p = TilePointer::new();
        let t0 = base();
        p.step(true, false, t0, false);
        assert_eq!(
            p.step(false, true, t0 + Duration::from_millis(HOLD_MS as u64 + 5), false),
            Some(TileEvent::HoldEnd)
        );
    }

    #[test]
    fn three_quick_taps_give_double_click_then_one_toggle() {
        let mut p = TilePointer::new();
        let t0 = base();
        p.step(true, false, t0, false);
        p.step(false, true, t0 + Duration::from_millis(60), false);
        assert_eq!(p.step(true, false, t0 + Duration::from_millis(150), false), Some(TileEvent::DoubleClick));
        p.step(false, true, t0 + Duration::from_millis(220), false);
        p.step(true, false, t0 + Duration::from_millis(320), false);
        p.step(false, true, t0 + Duration::from_millis(380), false);
        assert_eq!(
            p.step(false, false, t0 + Duration::from_millis(800), false),
            Some(TileEvent::Toggle)
        );
    }
}
