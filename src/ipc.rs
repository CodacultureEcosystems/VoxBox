//! Tiny Unix-socket bridge for the VoxBox resident (`Bsew@vbd`).
//!
//! The WisprFlow shape: the resident is a background tray process, and every
//! surface that wants to talk to it — the desktop icon, the Studio window, a
//! CLI — is its own short-lived process. This socket is the wiring: the
//! resident `serve`s, clients `send` one line and read one line back.
//!
//! Commands: `toggle`, `hold`, `constant on|off`, `reload`, `tile`, `status`.
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::Sender;
use std::time::Duration;

/// Where the resident listens. One socket, in the user's cache dir.
pub fn socket_path() -> std::path::PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("voxbox/vbd.sock")
}

/// Parse one client command line. Pure, tested; unknown commands are None.
pub fn parse_cmd(line: &str) -> Option<crate::dictate::Signal> {
    match line.trim() {
        "toggle" => Some(crate::dictate::Signal::Toggle),
        "hold" => Some(crate::dictate::Signal::HoldStart),
        "constant on" => Some(crate::dictate::Signal::Constant(true)),
        "constant off" => Some(crate::dictate::Signal::Constant(false)),
        "reload" => Some(crate::dictate::Signal::Reload),
        "tile" => Some(crate::dictate::Signal::TileShow),
        _ => None,
    }
}

/// Accept clients forever, forwarding commands to the resident loop.
/// `answer` produces the reply line for a command (used to report state).
pub fn serve_at(path: &std::path::Path, tx: Sender<crate::dictate::Signal>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(path); // stale socket from a dead resident
    let listener = UnixListener::bind(path)?;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let Ok(write_half) = stream.try_clone() else { continue };
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            continue;
        }
        let reply = match line.trim() {
            "status" => crate::studio::state_word(),
            cmd => match parse_cmd(cmd) {
                Some(sig) => {
                    let _ = tx.send(sig);
                    crate::studio::state_word()
                }
                None => "err unknown command".to_string(),
            },
        };
        let mut out = write_half;
        let _ = out.write_all(reply.as_bytes());
        let _ = out.write_all(b"\n");
    }
    Ok(())
}

/// Serve on the default socket path; errors are the caller's to print.
pub fn serve(tx: Sender<crate::dictate::Signal>) -> std::io::Result<()> {
    serve_at(&socket_path(), tx)
}

/// Send one command and read the resident's reply line.
pub fn send_at(path: &std::path::Path, cmd: &str) -> anyhow::Result<String> {
    let mut stream = UnixStream::connect(path)
        .map_err(|e| anyhow::anyhow!("resident not reachable at {}: {e}", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    writeln!(stream, "{cmd}")?;
    let mut resp = String::new();
    BufReader::new(stream).read_line(&mut resp)?;
    Ok(resp.trim().to_string())
}

/// Send one command on the default socket path.
pub fn send(cmd: &str) -> anyhow::Result<String> {
    send_at(&socket_path(), cmd)
}
