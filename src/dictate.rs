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
use crate::stt::{self, SttEngine, Transcriber};

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;

/// A control signal for the dictation resident. Sources: global hotkey
/// watchers, the tray Being, the Studio window, and the desktop tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Toggle,
    HoldStart,
    Constant(bool),
    Reload,
}

/// Atomics shared with the tile window. Written directly for hold-release
/// so a recording in flight can be stopped without channel round-trips.
#[derive(Debug, Default)]
pub struct Controls {
    pub constant: AtomicBool,
    pub hold_release: AtomicBool,
}

impl Controls {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    pub fn constant(&self) -> bool {
        self.constant.load(Ordering::Relaxed)
    }
}

/// What to warm up in the background when (re)building the Transcriber.
struct WarmSpec {
    engine: SttEngine,
    model: String,
    language: String,
}

impl WarmSpec {
    fn from_config(config: &Config) -> Self {
        Self {
            engine: stt::engine_from(&config.stt.engine),
            model: config.stt.model.clone(),
            language: config.stt.language.clone(),
        }
    }
}

/// Single-instance guard: hold an exclusive lock on a lockfile for the
/// lifetime of the resident so double-launches (desktop icon, autostart,
/// Studio) are no-ops instead of fighting over the hotkey and tray.
fn acquire_resident_lock() -> Result<()> {
    use std::os::unix::io::AsRawFd;
    let path = crate::stt::model_cache_dir().join("vbd.lock");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| anyhow!("lockfile {}: {e}", path.display()))?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return Err(anyhow!("lockfile held by another vbd"));
    }
    // Intentionally leak the file handle — the lock must outlive this call;
    // the OS drops it when the process exits either way.
    std::mem::forget(file);
    Ok(())
}

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
fn spawn_monitors(code: u16, tx: mpsc::Sender<Signal>) -> Result<usize> {
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

fn watch_device(path: &str, hotkey: u16, tx: mpsc::Sender<Signal>) {
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
            if tx.send(Signal::Toggle).is_err() {
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

/// Warm a Transcriber in the background; sends the built instance (or the
/// error) through the channel.
fn spawn_warmup(spec: WarmSpec, tx: mpsc::Sender<Result<Transcriber>>) {
    println!(
        "  [stt] warming {} ({}) in the background…",
        spec.engine.as_str(),
        if spec.model.is_empty() {
            "bundled"
        } else {
            &spec.model
        }
    );
    let _ = std::thread::Builder::new()
        .name("voxbox-stt-warm".into())
        .spawn(move || {
            let _ = tx.send(
                Transcriber::with_engine(spec.engine, &spec.model, &spec.language).and_then(
                    |mut t| {
                        t.preload()?;
                        Ok(t)
                    },
                ),
            );
        });
}

/// Resident entry point: wait for signals, then record → transcribe → inject.
pub fn run(config: Config, config_path: Option<&str>) -> Result<()> {
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
    let mut transcriber: Option<Transcriber> = None;

    if let Err(e) = acquire_resident_lock() {
        println!("  vbd resident: already awake ({e:#}) — nothing to do");
        return Ok(());
    }

    let (tx, rx) = mpsc::channel::<Signal>();
    let quit = Arc::new(AtomicBool::new(false));
    let controls = Controls::new();
    let tile_open = Arc::new(AtomicBool::new(false));
    let tray = crate::tray::spawn(
        &key_name,
        tx.clone(),
        Arc::clone(&quit),
        Arc::clone(&controls),
        Arc::clone(&tile_open),
    );

    // The desktop tile rides with the resident — visible immediately,
    // no Studio digging required.
    {
        let tile_ctx = crate::studio::TileCtx {
            trigger: Some(tx.clone()),
            controls: Some(Arc::clone(&controls)),
            open_flag: Arc::clone(&tile_open),
        };
        let _ = std::thread::Builder::new()
            .name("voxbox-tile-spawn".into())
            .spawn(move || crate::studio::launch_tile(tile_ctx));
    }

    let (pre_tx, pre_rx) = mpsc::channel::<Result<Transcriber>>();
    spawn_warmup(WarmSpec::from_config(&config), pre_tx);

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

    // Warm-up receiver for the currently selected engine; reset on Reload.
    let mut warm: Option<mpsc::Receiver<Result<Transcriber>>> = Some(pre_rx);

    loop {
        let sig = match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(s) => s,
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
        };
        if quit.load(Ordering::Relaxed) {
            println!("  vbd resident: consent received, resting");
            let _ = tracer.emit_ok(crate::trace::Verb::Yield, cell_id);
            break Ok(());
        }
        // Drain repeats/extra signals from the same burst.
        while rx.try_recv().is_ok() {}

        match sig {
            Signal::Reload => {
                transcriber = None;
                let fresh = Config::load(config_path);
                let (t2, r2) = mpsc::channel::<Result<Transcriber>>();
                warm = Some(r2);
                spawn_warmup(WarmSpec::from_config(&fresh), t2);
                println!("  [stt] reloading engine/settings in the background…");
                continue;
            }
            Signal::Constant(on) => {
                controls.constant.store(on, Ordering::Relaxed);
                if !on {
                    // Ends any utterance currently in flight.
                    controls.hold_release.store(true, Ordering::Relaxed);
                    println!("  ◉ constant mode OFF");
                    notify(notifications, "VoxBox", "Constant mode off");
                    continue;
                }
                println!(
                    "  ◉ constant mode ON — speak freely; double-click the tile (or press {key_display}) to stop"
                );
                notify(notifications, "VoxBox", "Constant mode on");
                while controls.constant() {
                    crate::tray::set_state(&tray, true);
                    let rec = recorder.record_with(Some(&controls.hold_release), false);
                    crate::tray::set_state(&tray, false);
                    match rec {
                        Ok(r) => deliver(
                            &mut transcriber,
                            &mut warm,
                            &r,
                            &mut tracer,
                            cell_id,
                            notifications,
                            &injector_hint,
                        ),
                        Err(e) => {
                            if !controls.constant() {
                                break;
                            }
                            unusable(&mut tracer, cell_id, notifications, e);
                        }
                    }
                }
                continue;
            }
            Signal::HoldStart => {
                controls.hold_release.store(false, Ordering::Relaxed);
                println!("  ● hold recorded — release to land the text");
                crate::tray::set_state(&tray, true);
                let rec = recorder.record_with(Some(&controls.hold_release), true);
                crate::tray::set_state(&tray, false);
                match rec {
                    Ok(r) => deliver(
                        &mut transcriber,
                        &mut warm,
                        &r,
                        &mut tracer,
                        cell_id,
                        notifications,
                        &injector_hint,
                    ),
                    Err(e) => unusable(&mut tracer, cell_id, notifications, e),
                }
                continue;
            }
            Signal::Toggle => {}
        }

        if controls.constant() {
            // A toggle while constant mode runs switches it off, ending
            // the current utterance cleanly.
            controls.constant.store(false, Ordering::Relaxed);
            controls.hold_release.store(true, Ordering::Relaxed);
            println!("  ◉ constant mode OFF");
            continue;
        }

        println!("  ● recording — speak, then pause");
        crate::tray::set_state(&tray, true);
        let rec = recorder.record_with(None, false);
        crate::tray::set_state(&tray, false);
        match rec {
            Ok(r) => deliver(
                &mut transcriber,
                &mut warm,
                &r,
                &mut tracer,
                cell_id,
                notifications,
                &injector_hint,
            ),
            Err(e) => unusable(&mut tracer, cell_id, notifications, e),
        }
    }
}

/// Record → transcribe → inject for one captured utterance.
#[allow(clippy::too_many_arguments)]
fn deliver(
    transcriber: &mut Option<Transcriber>,
    warm: &mut Option<mpsc::Receiver<Result<Transcriber>>>,
    rec: &crate::capture::Recording,
    tracer: &mut crate::trace::TraceLog,
    cell_id: u64,
    notifications: bool,
    injector_hint: &str,
) {
    if transcriber.is_none() {
        let rx = match warm {
            Some(r) => r,
            None => {
                let msg = "STT unavailable: no warm receiver".to_string();
                println!("  [stt] {msg}");
                let _ = tracer.emit_fail(
                    crate::trace::Verb::Yield,
                    crate::trace::E_NOT_IMPLEMENTED,
                    cell_id,
                );
                notify(notifications, "VoxBox", &msg);
                return;
            }
        };
        match rx.recv() {
            Ok(Ok(t)) => *transcriber = Some(t),
            Ok(Err(e)) => {
                let msg = format!("STT unavailable: {e:#}");
                println!("  [stt] {msg}");
                let _ = tracer.emit_fail(
                    crate::trace::Verb::Yield,
                    crate::trace::E_NOT_IMPLEMENTED,
                    cell_id,
                );
                notify(notifications, "VoxBox", &msg);
                return;
            }
            Err(e) => {
                let msg = format!("STT warm-up thread died: {e}");
                println!("  [stt] {msg}");
                let _ = tracer.emit_fail(
                    crate::trace::Verb::Yield,
                    crate::trace::E_NOT_IMPLEMENTED,
                    cell_id,
                );
                notify(notifications, "VoxBox", &msg);
                return;
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
            let _ = tracer.emit_fail(crate::trace::Verb::Produce, crate::trace::E_FAULT, cell_id);
            notify(notifications, "VoxBox", "No words detected");
            return;
        }
        Err(e) => {
            println!("  [stt] {e:#}");
            let _ = tracer.emit_fail(crate::trace::Verb::Produce, crate::trace::E_UTF8, cell_id);
            notify(notifications, "VoxBox", &format!("Transcription failed: {e:#}"));
            return;
        }
    };

    let injector = Injector::from_config(injector_hint, &text);
    match injector.inject_with_fallback(&text) {
        Ok(()) => {
            println!("  ▶ typed: {text}");
            let _ = tracer.emit_ok(crate::trace::Verb::Yield, cell_id);
            notify(notifications, "VoxBox", &text);
        }
        Err(e) => {
            println!("  [inject] {e:#}");
            let _ =
                tracer.emit_fail(crate::trace::Verb::Yield, crate::trace::E_PERMISSION, cell_id);
            notify(notifications, "VoxBox", &format!("Could not type: {e:#}"));
        }
    }
}

fn unusable(
    tracer: &mut crate::trace::TraceLog,
    cell_id: u64,
    notifications: bool,
    e: anyhow::Error,
) {
    println!("  [listen] {e:#}");
    let _ = tracer.emit_fail(crate::trace::Verb::Yield, crate::trace::E_NOT_READY, cell_id);
    notify(notifications, "VoxBox", "Nothing heard");
}
