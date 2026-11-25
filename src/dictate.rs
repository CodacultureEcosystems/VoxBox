//! System-wide dictation: the vbd Being — a background resident that watches
//! a global hotkey, records an utterance, transcribes it locally, and types
//! the text into whatever application is currently focused (WisprFlow-style).
use anyhow::{anyhow, Result};
use std::os::unix::io::AsRawFd;
use std::sync::mpsc;

use crate::capture::Recorder;
use crate::config::Config;
use crate::inject::Injector;
use crate::stt::Transcriber;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;

/// Map a hotkey name to its evdev key code.
pub fn hotkey_code(name: &str) -> Option<u16> {
    Some(match name.trim().to_lowercase().as_str() {
        "scrolllock" => 70,
        "numlock" => 69,
        "capslock" => 58,
        "pause" => 119,
        "insert" => 110,
        "home" => 102,
        "end" => 107,
        "pageup" => 104,
        "pagedown" => 109,
        "print" => 99,
        "f1" => 59,
        "f2" => 60,
        "f3" => 61,
        "f4" => 62,
        "f5" => 63,
        "f6" => 64,
        "f7" => 65,
        "f8" => 66,
        "f9" => 67,
        "f10" => 68,
        "f11" => 87,
        "f12" => 88,
        "f13" => 183,
        "f14" => 184,
        "f15" => 185,
        "f16" => 186,
        "f17" => 187,
        "f18" => 188,
        "f19" => 189,
        "f20" => 190,
        _ => return None,
    })
}

/// Spawn one watcher thread per input device; each forwards hotkey presses.
fn spawn_monitors(code: u16, tx: mpsc::Sender<()>) -> Result<usize> {
    let mut spawned = 0usize;
    let mut devices: Vec<String> = std::fs::read_dir("/dev/input")?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("event"))
                .unwrap_or(false)
        })
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    devices.sort();
    for path in devices {
        let tx = tx.clone();
        let code = code;
        if std::thread::Builder::new()
            .name(format!("voxbox-hotkey-{code}"))
            .spawn(move || watch_device(&path, code, tx))
            .is_ok()
        {
            spawned += 1;
        }
    }
    Ok(spawned)
}

fn watch_device(path: &str, hotkey: u16, tx: mpsc::Sender<()>) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let fd = file.as_raw_fd();
    let mut bytes = [0u8; 24];
    loop {
        let mut filled = 0usize;
        while filled < bytes.len() {
            let n = unsafe {
                libc::read(
                    fd,
                    bytes[filled..].as_mut_ptr() as *mut libc::c_void,
                    bytes.len() - filled,
                )
            };
            if n <= 0 {
                return;
            }
            filled += n as usize;
        }
        let ev_type = u16::from_le_bytes([bytes[16], bytes[17]]);
        let ev_code = u16::from_le_bytes([bytes[18], bytes[19]]);
        let value = i32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        if ev_type == EV_KEY && ev_code == hotkey && value == 1 {
            if tx.send(()).is_err() {
                return;
            }
        }
        let _ = EV_SYN;
    }
}

pub fn notify(enabled: bool, title: &str, body: &str) {
    if !enabled || !crate::tts::which("notify-send") {
        return;
    }
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "VoxBox", "-i", "voxbox", title, body])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Resident entry point: wait for the hotkey, then record → transcribe → inject.
pub fn run(config: Config, no_tts: bool) -> Result<()> {
    let key_name = config.dictation.key.clone();
    let code = hotkey_code(&key_name)
        .ok_or_else(|| anyhow!("unknown hotkey {key_name:?} (see config [dictation].key)"))?;
    let notifications = config.dictation.notifications;
    let redraw_text = !no_tts && config.tts.enabled;

    let recorder = Recorder {
        sample_rate: config.audio.sample_rate,
        silence_ms: config.audio.silence_ms,
        max_seconds: config.audio.max_seconds,
        threshold: config.audio.threshold,
        frame_ms: 30,
    };
    let stt_cfg = config.stt.clone();
    let mut transcriber: Option<Transcriber> = None;

    let (tx, rx) = mpsc::channel::<()>();
    let started = spawn_monitors(code, tx)?;
    if started == 0 {
        return Err(anyhow!(
            "no readable input devices under /dev/input; add your user to the input group: \
             sudo usermod -aG input $USER (log out and back in)"
        ));
    }

    let injector_hint = config.dictation.injector.clone();
    let cell_id = (std::process::id() as u64) << 16 | (code as u64);
    let mut tracer = crate::trace::TraceLog::open(crate::trace::TraceLog::default_path())?;
    let _ = tracer.emit_ok(crate::trace::Verb::Sprout, cell_id);
    println!(
        "  VoxBox dictation: press {} to talk; text lands in the focused app",
        key_name.to_uppercase()
    );
    println!("  injector: {injector_hint}; notifications: {notifications}");
    notify(notifications, "VoxBox", &format!("Listening — press {}", key_name.to_uppercase()));

    loop {
        rx.recv().map_err(|_| anyhow!("hotkey watchers stopped"))?;
        // Debounce repeats/extra presses.
        while rx.try_recv().is_ok() {}
        std::thread::sleep(std::time::Duration::from_millis(150));
        while rx.try_recv().is_ok() {}

        println!("  ● recording — speak, then pause");
        let rec = match recorder.record() {
            Ok(r) => r,
            Err(e) => {
                println!("  [listen] {e:#}");
                let _ = tracer.emit_fail(
                    crate::trace::Verb::Yield,
                    crate::trace::E_NOT_READY,
                    cell_id,
                );
                notify(notifications, "VoxBox", "Nothing heard");
                continue;
            }
        };

        if transcriber.is_none() {
            match Transcriber::new(&stt_cfg.model, &stt_cfg.language) {
                Ok(t) => transcriber = Some(t),
                Err(e) => {
                    let msg = format!("STT unavailable: {e:#}");
                    println!("  [stt] {msg}");
                    let _ = tracer.emit_fail(
                        crate::trace::Verb::Yield,
                        crate::trace::E_NOT_IMPLEMENTED,
                        cell_id,
                    );
                    notify(notifications, "VoxBox", &msg);
                    continue;
                }
            }
        }
        let text = match transcriber
            .as_mut()
            .expect("transcriber")
            .transcribe(&rec.pcm, rec.sample_rate)
        {
            Ok(t) if !t.is_empty() => t,
            Ok(_) => {
                let _ = tracer.emit_fail(
                    crate::trace::Verb::Produce,
                    crate::trace::E_FAULT,
                    cell_id,
                );
                notify(notifications, "VoxBox", "No words detected");
                continue;
            }
            Err(e) => {
                println!("  [stt] {e:#}");
                let _ = tracer.emit_fail(
                    crate::trace::Verb::Produce,
                    crate::trace::E_UTF8,
                    cell_id,
                );
                notify(notifications, "VoxBox", &format!("Transcription failed: {e:#}"));
                continue;
            }
        };

        let injector = Injector::from_config(&injector_hint, &text);
        match injector.inject(&text) {
            Ok(()) => {
                println!("  ▶ typed: {text}");
                let _ = tracer.emit_ok(crate::trace::Verb::Yield, cell_id);
                notify(notifications, "VoxBox", &text);
                if redraw_text {
                    // keep the shell speaker out of the dictation hot path;
                    // a soft chime like audio could go here later
                }
            }
            Err(e) => {
                println!("  [inject] {e:#}");
                let _ = tracer.emit_fail(crate::trace::Verb::Yield, crate::trace::E_PERMISSION, cell_id);
                notify(notifications, "VoxBox", &format!("Could not type: {e:#}"));
            }
        }
    }
}
