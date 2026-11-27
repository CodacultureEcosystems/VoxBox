//! The vbd tray Being: resident status icon (StatusNotifierItem over D-Bus).
//! Left-click opens the VoxBox Studio window; the menu offers Record
//! now, opening the window, and quit.
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Category, Icon, Status, ToolTip, Tray};

use crate::dictate::{Controls, Signal};

const IDLE: u8 = 0;
const RECORDING: u8 = 1;

/// Human label for the current resident state.
pub fn state_label(state: u8) -> &'static str {
    match state {
        RECORDING => "● recording — voice will be typed into the focused app",
        _ => "idle — press the hotkey or use Record now",
    }
}

/// SNI status derived from the resident state.
pub fn state_status(state: u8) -> Status {
    match state {
        RECORDING => Status::NeedsAttention,
        _ => Status::Active,
    }
}

/// Menu caption for the record entry given the configured hotkey.
pub fn record_label(key: &str) -> String {
    format!("Record now  (same as pressing {})", key.to_uppercase())
}

/// The ARGB32 bytes of a decoded PNG, for icon_pixmap.
pub fn decode_png_argb(png_bytes: &[u8]) -> Option<(i32, i32, Vec<u8>)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let mut argb = Vec::with_capacity(w * h * 4);
    match info.color_type {
        png::ColorType::Rgba => {
            for px in buf[..w * h * 4].chunks_exact(4) {
                argb.extend_from_slice(&[px[3], px[0], px[1], px[2]]); // RGBA8 -> ARGB32
            }
        }
        png::ColorType::Rgb => {
            for px in buf[..w * h * 3].chunks_exact(3) {
                argb.extend_from_slice(&[255, px[0], px[1], px[2]]); // RGB8 -> opaque ARGB32
            }
        }
        _ => return None,
    }
    Some((info.width as i32, info.height as i32, argb))
}

const TRAY_PNG: &[u8] = include_bytes!("../assets/voxbox-tray.png");

pub struct VoxTray {
    state: AtomicU8,
    quit: Arc<AtomicBool>,
    key: String,
    trigger: mpsc::Sender<Signal>,
    controls: Arc<Controls>,
}

/// Open VoxBox Studio as its OWN process — a window must own its process's
/// main thread (winit panics otherwise), so the tray never spawns one in-process.
pub fn open_studio() {
    let exe = std::env::current_exe()
        .ok()
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("vbd"));
    let result = std::process::Command::new(exe)
        .arg("--window")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Err(e) = result {
        eprintln!("  [studio] could not launch: {e}");
    }
}

fn item(
    label: String,
    enabled: bool,
    on: impl Fn(&mut VoxTray) + Send + 'static,
) -> StandardItem<VoxTray> {
    StandardItem {
        label,
        enabled,
        activate: Box::new(on),
        icon_name: String::new(),
        icon_data: Vec::new(),
        shortcut: Vec::new(),
        disposition: ksni::menu::Disposition::Normal,
        visible: true,
    }
}

impl VoxTray {
    fn glyph(&self) -> &'static str {
        match self.state.load(Ordering::Relaxed) {
            RECORDING => "●",
            _ => "○",
        }
    }
}

impl Tray for VoxTray {
    fn id(&self) -> String {
        "voxbox-vbd".into()
    }

    fn title(&self) -> String {
        "VoxBox".into()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    fn status(&self) -> Status {
        state_status(self.state.load(Ordering::Relaxed))
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        open_studio();
    }

    fn icon_name(&self) -> String {
        "voxbox".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        match decode_png_argb(TRAY_PNG) {
            Some((w, h, data)) => vec![Icon {
                width: w,
                height: h,
                data,
            }],
            None => Vec::new(),
        }
    }

    fn tool_tip(&self) -> ToolTip {
        let s = self.state.load(Ordering::Relaxed);
        ToolTip {
            icon_name: "voxbox".into(),
            icon_pixmap: Vec::new(),
            title: format!("VoxBox — Bsew@vbd {}", self.glyph()),
            description: format!(
                "{}\nHotkey: {}",
                state_label(s),
                self.key.to_uppercase()
            ),
        }
    }

    fn menu(&self) -> Vec<MenuItem<VoxTray>> {
        let trigger = self.trigger.clone();
        let constant_trigger = self.trigger.clone();
        let constant_on = self.controls.constant();
        vec![
            MenuItem::Standard(item(record_label(&self.key), true, move |_| {
                let _ = trigger.send(Signal::Toggle);
            })),
            MenuItem::Standard(item(
                format!(
                    "{} Constant mode",
                    if constant_on { "☒ Stop" } else { "☐ Start" }
                ),
                true,
                move |tray| {
                    let next = !tray.controls.constant();
                    let _ = constant_trigger.send(Signal::Constant(next));
                },
            )),
            MenuItem::Standard(item(
                "Edit settings (config.toml)".into(),
                true,
                |_| open_config(),
            )),
            MenuItem::Separator,
            MenuItem::Standard(item("Open VoxBox Studio".into(), true, |_| open_studio())),
            MenuItem::Separator,
            MenuItem::Standard(item("Quit".into(), true, move |tray| {
                tray.quit.store(true, Ordering::Relaxed);
            })),
        ]
    }
}

/// Open config.toml in the user's default editor. Loud on failure — no
/// menu entry may be a dead end.
pub fn open_config() {
    let path = crate::config::config_path();
    if !path.exists() {
        eprintln!("  [tray] config not found at {} — run install.sh first", path.display());
        return;
    }
    match std::process::Command::new("xdg-open").arg(&path).spawn() {
        Ok(_) => eprintln!("  [tray] opening {} in your editor", path.display()),
        Err(e) => eprintln!(
            "  [tray] could not open an editor for {} ({e}) — edit it in any text editor",
            path.display()
        ),
    }
}

/// Pick the first available terminal emulator. Pure: `available` is the
/// list of candidates found on PATH, so tests can drive it.
pub fn terminal_pick(available: &[&str]) -> Option<&'static str> {
    for candidate in ["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "xterm"] {
        if available.contains(&candidate) {
            return Some(candidate);
        }
    }
    None
}

pub fn spawn_shell(args: &[&str]) {
    let home = std::env::var("HOME").unwrap_or_default();
    let bin = format!("{home}/.local/bin/voxbox");
    let path = if std::path::Path::new(&bin).exists() {
        bin
    } else {
        "voxbox".into()
    };
    // The voice shell is interactive (it reads the mic + stdin), so it must
    // be VISIBLE: run it inside a terminal emulator. Spawning it headless
    // made the buttons look like they do nothing.
    let mut line = path.clone();
    for a in args {
        line.push(' ');
        line.push_str(a);
    }
    let candidates = ["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "xterm"];
    let found: Vec<&str> = candidates
        .iter()
        .copied()
        .filter(|t| which_exists(t))
        .collect();
    if let Some(term) = terminal_pick(&found) {
        let result = std::process::Command::new(term)
            .arg("-e")
            .arg(&line)
            .spawn();
        if let Err(e) = result {
            eprintln!("  [shell] {term} failed: {e} — running headless with a log");
        } else {
            return;
        }
    }
    // Last resort: detached, but loud about where the output went.
    let log = format!("{home}/.cache/voxbox/voxbox-shell.log");
    if let Some(f) = std::fs::File::create(&log).ok() {
        let _ = std::process::Command::new(path)
            .args(args)
            .stdout(f)
            .stderr(std::process::Stdio::null())
            .spawn();
        eprintln!("  [shell] no terminal found; output goes to {log}");
    }
}

fn which_exists(bin: &str) -> bool {
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .any(|p| p.is_file())
}

/// Spawn the tray Being; None when no SNI/D-Bus host is reachable
/// (headless shells keep working without it).
pub fn spawn(
    key: &str,
    trigger: mpsc::Sender<Signal>,
    quit: Arc<AtomicBool>,
    controls: Arc<Controls>,
) -> Option<Handle<VoxTray>> {
    let tray = VoxTray {
        state: AtomicU8::new(IDLE),
        quit,
        key: key.to_string(),
        trigger,
        controls,
    };
    match tray.spawn() {
        Ok(handle) => Some(handle),
        Err(e) => {
            println!("  [tray] no status-notifier host ({e:#}); notifications still work");
            None
        }
    }
}

/// Push the recording state to the tray (no-op without a tray host).
pub fn set_state(tray: &Option<Handle<VoxTray>>, recording: bool) {
    crate::studio::set_state(recording);
    let s = if recording { RECORDING } else { IDLE };
    if let Some(h) = tray {
        let _ = h.update(|t| t.state.store(s, Ordering::Relaxed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_png_decodes_to_argb32() {
        let (w, h, data) = decode_png_argb(TRAY_PNG).expect("embedded tray png decodes");
        assert_eq!((w, h), (32, 32));
        assert_eq!(data.len(), 32 * 32 * 4);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode_png_argb(b"not a png").is_none());
    }

    #[test]
    fn state_labels_and_status_follow_state() {
        assert!(state_label(IDLE).contains("idle"));
        assert!(state_label(RECORDING).contains("recording"));
        assert_eq!(state_status(RECORDING), Status::NeedsAttention);
        assert_eq!(state_status(IDLE), Status::Active);
    }

    #[test]
    fn record_label_uses_configured_key() {
        assert!(record_label("pause").contains("PAUSE"));
        assert!(record_label("f13").contains("F13"));
    }
}
