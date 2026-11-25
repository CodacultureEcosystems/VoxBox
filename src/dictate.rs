//! System-wide dictation: the vbd Being — a background resident that watches
//! a global hotkey, records an utterance, transcribes it locally, and types
//! the text into whatever application is currently focused (WisprFlow-style).
use anyhow::{anyhow, Result};
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

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
pub fn run(config: Config, _no_tts: bool) -> Result<()> {
    let key_name = config.dictation.key.clone();
    let key_display = key_name.to_uppercase();
    let code = hotkey_code(&key_name)
        .ok_or_else(|| anyhow!("unknown hotkey {key_name:?} (see config [dictation].key)"))?;
    let notifications = config.dictation.notifications;

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
    let quit = Arc::new(AtomicBool::new(false));
    let tray = crate::tray::spawn(&key_name, tx.clone(), Arc::clone(&quit));

    let started = spawn_monitors(code, tx.clone())?;
    let mut monitors_active = started > 0;
    if !monitors_active {
        println!(
            "  [input] no /dev/input devices readable yet — waiting (5 s retries). \
             Fix with: sudo usermod -aG input,uinput $USER, then LOG OUT and back in, \
             and restart vbd"
        );
    }

    let injector_hint = config.dictation.injector.clone();
    let cell_id = (std::process::id() as u64) << 16 | (code as u64);
    let mut tracer = crate::trace::TraceLog::open(crate::trace::TraceLog::default_path())?;
    let _ = tracer.emit_ok(crate::trace::Verb::Sprout, cell_id);
    println!(
        "  VoxBox dictation: press {key_display} (or the tray 'Record now') to talk; \
         text lands in the focused app"
    );
    println!("  injector: {injector_hint}; notifications: {notifications}");
    notify(
        notifications,
        "VoxBox",
        &format!("Listening — press {key_display}"),
    );

    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if quit.load(Ordering::Relaxed) {
                    println!("  vbd resident: consent received, resting");
                    let _ = tracer.emit_ok(crate::trace::Verb::Yield, cell_id);
                    break Ok(());
                }
                if !monitors_active {
                    if let Ok(n) = spawn_monitors(code, tx.clone()) {
                        if n > 0 {
                            monitors_active = true;
                            println!("  [input] hotkey listeners connected");
                        }
                    }
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("hotkey watchers stopped"));
            }
        }
        if quit.load(Ordering::Relaxed) {
            println!("  vbd resident: consent received, resting");
            let _ = tracer.emit_ok(crate::trace::Verb::Yield, cell_id);
            break Ok(());
        }
        // Debounce repeats/extra presses.
        while rx.try_recv().is_ok() {}
        std::thread::sleep(Duration::from_millis(150));
        while rx.try_recv().is_ok() {}

        println!("  ● recording — speak, then pause");
        crate::tray::set_state(&tray, true);
        let rec = match recorder.record() {
            Ok(r) => r,
            Err(e) => {
                crate::tray::set_state(&tray, false);
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
        crate::tray::set_state(&tray, false);

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
            }
            Err(e) => {
                println!("  [inject] {e:#}");
                let _ = tracer.emit_fail(crate::trace::Verb::Yield, crate::trace::E_PERMISSION, cell_id);
                notify(notifications, "VoxBox", &format!("Could not type: {e:#}"));
            }
        }
    }
}
