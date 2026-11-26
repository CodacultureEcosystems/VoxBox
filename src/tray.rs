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
    window_open: Arc<AtomicBool>,
    tile_open: Arc<AtomicBool>,
}

/// Open the Studio window (idempotent: one window at a time).
pub fn open_window(
    open_flag: Arc<AtomicBool>,
    trigger: Option<mpsc::Sender<Signal>>,
    controls: Option<Arc<Controls>>,
    tile_open: Arc<AtomicBool>,
    key: String,
) {
    if open_flag
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let ctx = crate::studio::StudioCtx {
        trigger,
        controls,
        open_flag,
        tile_open,
        key,
    };
    let _ = std::thread::Builder::new()
        .name("voxbox-studio".into())
        .spawn(move || crate::studio::launch(ctx));
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
        crate::tray::open_window(
            Arc::clone(&self.window_open),
            Some(self.trigger.clone()),
            Some(Arc::clone(&self.controls)),
            Arc::clone(&self.tile_open),
            self.key.clone(),
        );
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
                    "{} Constant mode  (double-click the desktop tile)",
                    if constant_on { "☒ Stop" } else { "☐ Start" }
                ),
                true,
                move |tray| {
                    let next = !tray.controls.constant();
                    let _ = constant_trigger.send(Signal::Constant(next));
                },
            )),
            MenuItem::Standard(item(
                format!(
                    "Hotkey: {} (config.toml [dictation])",
                    self.key.to_uppercase()
                ),
                false,
                |_| {},
            )),
            MenuItem::Separator,
            MenuItem::Standard(item(
                "Open VoxBox Studio".into(),
                true,
                |tray| open_window(
                    tray.window_open.clone(),
                    Some(tray.trigger.clone()),
                    Some(Arc::clone(&tray.controls)),
                    Arc::clone(&tray.tile_open),
                    tray.key.clone(),
                ),
            )),
            MenuItem::Separator,
            MenuItem::Standard(item("Quit".into(), true, move |tray| {
                tray.quit.store(true, Ordering::Relaxed);
            })),
        ]
    }
}

pub fn spawn_shell(args: &[&str]) {
    let home = std::env::var("HOME").unwrap_or_default();
    let bin = format!("{home}/.local/bin/voxbox");
    let path = if std::path::Path::new(&bin).exists() {
        bin
    } else {
        "voxbox".into()
    };
    let _ = std::process::Command::new(path)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
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
        window_open: Arc::new(AtomicBool::new(false)),
        tile_open: Arc::new(AtomicBool::new(false)),
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
