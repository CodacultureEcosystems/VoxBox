//! Inject text into whatever application is currently focused.
//!
//! Backend priority (all native, no cloud):
//! 1. `uinput` kernel virtual keyboard (works on X11 and most Wayland compositors)
//! 2. `wtype` (Wayland native)
//! 3. `xdotool` (X11)
//! 4. `ydotool` (needs ydotoold)
use anyhow::{anyhow, Result};

use crate::keys::{ascii_typeable, char_to_keypress, KEY_LEFTSHIFT, KEY_SPACE};
use crate::tts::which;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Injector {
    Uinput,
    WType,
    XDoTool,
    YDoTool,
    None,
}

impl Injector {
    pub fn as_str(&self) -> &'static str {
        match self {
            Injector::Uinput => "uinput",
            Injector::WType => "wtype",
            Injector::XDoTool => "xdotool",
            Injector::YDoTool => "ydotool",
            Injector::None => "none",
        }
    }

    /// Runtime detection. `pure_ascii` lets uinput win only when the text is
    /// typeable on a US layout.
    pub fn detect(text: &str) -> Injector {
        let uinput_ok = ascii_typeable(text) && uinput_available();
        if uinput_ok {
            return Injector::Uinput;
        }
        if which("wtype") {
            return Injector::WType;
        }
        if which("xdotool") {
            return Injector::XDoTool;
        }
        if which("ydotool") {
            return Injector::YDoTool;
        }
        if uinput_available() {
            return Injector::Uinput;
        }
        Injector::None
    }

    /// Resolve the configured injector. `auto` defers to detect(); explicit
    /// names fall back to auto-detection only when the requested backend is
    /// unavailable and the text needs it.
    pub fn from_config(name: &str, text: &str) -> Injector {
        match name.trim().to_lowercase().as_str() {
            "uinput" | "native" => {
                if uinput_available() && (ascii_typeable(text) || !has_cli_injector()) {
                    Injector::Uinput
                } else {
                    Injector::detect(text)
                }
            }
            "wtype" => Injector::WType,
            "xdotool" => Injector::XDoTool,
            "ydotool" => Injector::YDoTool,
            "none" | "off" => Injector::None,
            _ => Injector::detect(text),
        }
    }

    /// Backends worth trying after the primary one fails.
    fn fallbacks(self, text: &str) -> Vec<Injector> {
        fallback_order(
            self,
            text,
            which("wtype"),
            which("xdotool"),
            which("ydotool"),
            uinput_available(),
        )
    }

    /// Type the text, retrying with the other available backends if the
    /// primary fails (e.g. /dev/uinput opens but UI_DEV_CREATE is denied).
    pub fn inject_with_fallback(self, text: &str) -> Result<()> {
        match self.inject(text) {
            Ok(()) => Ok(()),
            Err(first) => {
                for alt in self.fallbacks(text) {
                    if alt.inject(text).is_ok() {
                        return Ok(());
                    }
                }
                Err(first)
            }
        }
    }

    pub fn inject(self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        match self {
            Injector::Uinput => {
                let kbd = UinputKeyboard::open()?;
                kbd.type_text(text)
            }
            Injector::WType => run_injector("wtype", wtype_args(text)),
            Injector::XDoTool => run_injector("xdotool", xdotool_args(text)),
            Injector::YDoTool => run_injector("ydotool", ydotool_args(text)),
            Injector::None => Err(anyhow!(
                "no text injector available; install xdotool (X11), wtype (Wayland), \
                 or enable /dev/uinput access (add your user to the uinput group)"
            )),
        }
    }
}

/// Deterministic alternate ordering: CLI injectors first (wtype, xdotool,
/// ydotool), uinput last, so a denied uinput device still gets a second
/// chance via the others.
fn fallback_order(
    primary: Injector,
    text: &str,
    has_wtype: bool,
    has_xdotool: bool,
    has_ydotool: bool,
    uinput_ok: bool,
) -> Vec<Injector> {
    let mut alts = Vec::new();
    if has_wtype && primary != Injector::WType {
        alts.push(Injector::WType);
    }
    if has_xdotool && primary != Injector::XDoTool {
        alts.push(Injector::XDoTool);
    }
    if has_ydotool && primary != Injector::YDoTool {
        alts.push(Injector::YDoTool);
    }
    if uinput_ok && ascii_typeable(text) && primary != Injector::Uinput {
        alts.push(Injector::Uinput);
    }
    alts
}

fn run_injector(program: &str, args: Vec<String>) -> Result<()> {
    let status = std::process::Command::new(program)
        .args(&args)
        .status()
        .map_err(|e| anyhow!("{program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("{program} exited with an error"))
    }
}

pub fn wtype_args(text: &str) -> Vec<String> {
    vec![text.to_string()]
}

pub fn xdotool_args(text: &str) -> Vec<String> {
    vec![
        "type".into(),
        "--clearmodifiers".into(),
        "--delay".into(),
        "4".into(),
        "--".into(),
        text.to_string(),
    ]
}

pub fn ydotool_args(text: &str) -> Vec<String> {
    vec!["type".into(), "--".into(), text.to_string()]
}

fn has_cli_injector() -> bool {
    which("wtype") || which("xdotool") || which("ydotool")
}

pub fn uinput_available() -> bool {
    let path = "/dev/uinput";
    let available = std::fs::metadata(path).is_ok()
        && std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .is_ok();
    available
}

pub const UI_SET_EVBIT: libc::c_ulong = 0x40045564;
pub const UI_SET_KEYBIT: libc::c_ulong = 0x40045565;
pub const UI_DEV_CREATE: libc::c_ulong = 0x5501;
pub const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const EV_KEY: u16 = 0x01;
const EV_SYN: u16 = 0x00;

/// A virtual keyboard created through /dev/uinput; types plain US-ASCII.
pub struct UinputKeyboard {
    fd: i32,
}

impl UinputKeyboard {
    pub fn open() -> Result<UinputKeyboard> {
        let path = std::ffi::CString::new("/dev/uinput").expect("valid path");
        let fd = unsafe {
            libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_NONBLOCK)
        };
        if fd < 0 {
            return Err(anyhow!(
                "/dev/uinput unavailable ({}) — add yourself to the uinput group: sudo usermod -aG uinput $USER",
                std::io::Error::last_os_error()
            ));
        }
        let codes = all_required_keycodes();
        unsafe {
            if libc::ioctl(fd, UI_SET_EVBIT as libc::c_ulong, EV_KEY as libc::c_int) < 0
                || libc::ioctl(fd, UI_SET_EVBIT as libc::c_ulong, EV_SYN as libc::c_int) < 0
            {
                close_fd(fd);
                return Err(anyhow!("UI_SET_EVBIT failed"));
            }
            for &code in codes.iter() {
                if libc::ioctl(fd, UI_SET_KEYBIT as libc::c_ulong, code as libc::c_int) < 0 {
                    close_fd(fd);
                    return Err(anyhow!("UI_SET_KEYBIT failed for code {code}"));
                }
            }
            if libc::ioctl(fd, UI_DEV_CREATE as libc::c_ulong) < 0 {
                close_fd(fd);
                return Err(anyhow!("UI_DEV_CREATE failed"));
            }
        }
        Ok(UinputKeyboard { fd })
    }

    pub fn type_text(&self, text: &str) -> Result<()> {
        for c in text.chars() {
            let kp = match char_to_keypress(c) {
                Some(k) => k,
                None => {
                    self.tap(KEY_SPACE)?;
                    continue;
                }
            };
            if kp.shift {
                self.key(KEY_LEFTSHIFT, 1)?;
            }
            self.key(kp.code, 1)?;
            self.key(kp.code, 0)?;
            if kp.shift {
                self.key(KEY_LEFTSHIFT, 0)?;
            }
        }
        Ok(())
    }

    fn key(&self, code: u16, value: i32) -> Result<()> {
        self.write_event(EV_KEY, code, value)?;
        self.write_event(EV_SYN, 0, 0)?;
        Ok(())
    }

    fn tap(&self, code: u16) -> Result<()> {
        self.key(code, 1)?;
        self.key(code, 0)?;
        Ok(())
    }

    fn write_event(&self, ev_type: u16, code: u16, value: i32) -> Result<()> {
        #[repr(C)]
        struct InputEvent {
            tv_sec: i64,
            tv_usec: i64,
            ev_type: u16,
            code: u16,
            value: i32,
        }
        let ev = InputEvent {
            tv_sec: 0,
            tv_usec: 0,
            ev_type,
            code,
            value,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&ev as *const InputEvent) as *const u8,
                std::mem::size_of::<InputEvent>(),
            )
        };
        loop {
            match unsafe { libc::write(self.fd, bytes.as_ptr() as *const libc::c_void, bytes.len()) } {
                n if n == bytes.len() as isize => return Ok(()),
                -1 if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_micros(200));
                }
                -1 => return Err(anyhow!("uinput write failed: {}", std::io::Error::last_os_error())),
                _ => continue,
            }
        }
    }
}

fn close_fd(fd: i32) {
    unsafe {
        libc::close(fd);
    }
}

/// Every keycode the virtual keyboard must accept: US-ASCII printables
/// mapped through the layout table, plus Enter, Tab and the shift key.
/// Codes are deduplicated and sorted for deterministic device setup.
fn all_required_keycodes() -> Vec<u16> {
    let mut codes: Vec<u16> = (' '..='~')
        .chain(['\n', '\t'])
        .filter_map(|c| char_to_keypress(c))
        .map(|k| k.code)
        .collect();
    codes.push(KEY_LEFTSHIFT);
    codes.sort_unstable();
    codes.dedup();
    codes
}

impl Drop for UinputKeyboard {
    fn drop(&mut self) {
        unsafe {
            libc::ioctl(self.fd, UI_DEV_DESTROY as libc::c_ulong);
            libc::close(self.fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{KEY_APOSTROPHE, KEY_COMMA, KEY_ENTER, KEY_MINUS};

    #[test]
    fn injector_arg_shapes() {
        assert_eq!(wtype_args("hi there"), vec!["hi there".to_string()]);
        assert_eq!(
            xdotool_args("hi"),
            vec![
                "type".to_string(),
                "--clearmodifiers".to_string(),
                "--delay".to_string(),
                "4".to_string(),
                "--".to_string(),
                "hi".to_string()
            ]
        );
        assert_eq!(
            ydotool_args("hi"),
            vec!["type".to_string(), "--".to_string(), "hi".to_string()]
        );
    }

    #[test]
    fn names_round_trip() {
        for (i, name) in ["uinput", "wtype", "xdotool", "ydotool", "none"]
            .iter()
            .enumerate()
        {
            let inj = match i {
                0 => Injector::Uinput,
                1 => Injector::WType,
                2 => Injector::XDoTool,
                3 => Injector::YDoTool,
                _ => Injector::None,
            };
            assert_eq!(inj.as_str(), *name);
        }
    }

    #[test]
    fn fallback_order_prefers_cli_over_uinput() {
        let alts = fallback_order(
            Injector::Uinput,
            "hello",
            true,
            false,
            false,
            false,
        );
        assert_eq!(alts, vec![Injector::WType]);

        let alts = fallback_order(
            Injector::XDoTool,
            "unicode: & ü",
            true,
            true,
            true,
            true,
        );
        assert_eq!(
            alts,
            vec![Injector::WType, Injector::YDoTool],
            "uinput skipped for non-ascii text; no self-retry"
        );
    }

    #[test]
    fn type_text_handles_all_typeable_chars() {
        // Pure logic check: every typeable char produces a keypress.
        let text = "Hello, world! It's 10:42 a.m. (test) - _+{}|:\"~<>?";
        for c in text.chars() {
            assert!(char_to_keypress(c).is_some(), "missing mapping for {c:?}");
        }
        assert_eq!(char_to_keypress(','), Some(crate::keys::Keypress { code: KEY_COMMA, shift: false }));
        assert_eq!(char_to_keypress('-'), Some(crate::keys::Keypress { code: KEY_MINUS, shift: false }));
        assert_eq!(char_to_keypress('\''), Some(crate::keys::Keypress { code: KEY_APOSTROPHE, shift: false }));
        assert_eq!(char_to_keypress('\n'), Some(crate::keys::Keypress { code: KEY_ENTER, shift: false }));
    }
}
