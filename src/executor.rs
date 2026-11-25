use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::config::ExecutorConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    pub ok: bool,
    pub output: String,
    pub error: String,
    pub returncode: i32,
    pub command: String,
    pub asked_confirmation: bool,
}

const FORK_BOMB_SIGNS: &[&str] = &[":(){", ":(){:|", "& };:"];

fn lower_contains_any(hay: &str, needles: &[String]) -> bool {
    let low = hay.to_lowercase();
    needles
        .iter()
        .any(|p| !p.trim().is_empty() && low.contains(&p.trim().to_lowercase()))
}

pub fn is_blocked(command: &str, cfg: &ExecutorConfig) -> bool {
    let low = command.to_lowercase();
    if FORK_BOMB_SIGNS.iter().any(|s| low.contains(s)) {
        return true;
    }
    let stripped = low
        .strip_prefix("sudo ")
        .map(str::to_string)
        .unwrap_or(low);
    for flag in ["-rf", "-fr", "-r", "-f"] {
        let prefix = format!("rm {flag} ");
        if let Some(rest) = stripped.strip_prefix(&prefix) {
            let rest = rest.trim().trim_matches(['"', '\'']);
            let parts: Vec<&str> = rest.split_whitespace().filter(|p| !p.starts_with('-')).collect();
            if let Some(first) = parts.first() {
                let first = first.trim_end_matches(['/', '*', ')']);
                let first = first.trim_matches(['\'', '"']);
                if matches!(first, "" | "/" | "*" | "~" | "$home") {
                    return true;
                }
            }
        }
    }
    lower_contains_any(command, &cfg.blocklist)
}

pub fn needs_confirmation(command: &str, cfg: &ExecutorConfig) -> bool {
    lower_contains_any(command, &cfg.confirm_patterns)
}

fn drain(stream: Option<impl Read + Send + 'static>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    match stream {
        Some(mut s) => {
            std::thread::spawn(move || {
                let mut buf = String::new();
                let _ = s.read_to_string(&mut buf);
                let _ = tx.send(buf);
            });
        }
        None => {
            let _ = tx.send(String::new());
        }
    }
    rx
}

fn finish(child: &mut Child, timeout: Duration, command: &str, risky: bool) -> ExecResult {
    let out_rx = drain(child.stdout.take());
    let err_rx = drain(child.stderr.take());
    let deadline = Instant::now() + timeout.max(Duration::from_secs(1));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = out_rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
                let err = err_rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
                let code = status.code().unwrap_or(1);
                return ExecResult {
                    ok: status.success(),
                    output: out.trim().to_string(),
                    error: err.trim().to_string(),
                    returncode: code,
                    command: command.into(),
                    asked_confirmation: risky,
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out_rx.recv_timeout(Duration::from_secs(1));
                    let _ = err_rx.recv_timeout(Duration::from_secs(1));
                    return ExecResult {
                        ok: false,
                        output: String::new(),
                        error: format!("Timed out after {} seconds.", timeout.as_secs()),
                        returncode: 124,
                        command: command.into(),
                        asked_confirmation: risky,
                    };
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                return ExecResult {
                    ok: false,
                    output: String::new(),
                    error: format!("failed to run command: {e}"),
                    returncode: 127,
                    command: command.into(),
                    asked_confirmation: risky,
                };
            }
        }
    }
}

/// Execute a command line with safety gates.
///
/// `confirm` decides whether risky commands run; `None` refuses them.
pub fn run_command(
    command: &str,
    cfg: &ExecutorConfig,
    confirm: Option<&dyn Fn(&str) -> bool>,
) -> ExecResult {
    let command = command.trim();
    let empty = ExecResult {
        ok: true,
        output: String::new(),
        error: String::new(),
        returncode: 0,
        command: String::new(),
        asked_confirmation: false,
    };
    if command.is_empty() {
        return empty;
    }
    if is_blocked(command, cfg) {
        return ExecResult {
            ok: false,
            output: String::new(),
            error: "That command is blocked by the safety policy.".into(),
            returncode: 126,
            command: command.into(),
            asked_confirmation: false,
        };
    }
    let risky = needs_confirmation(command, cfg);
    if risky && confirm.map_or(true, |f| !f(command)) {
        return ExecResult {
            ok: false,
            output: String::new(),
            error: "Cancelled; the command was not run.".into(),
            returncode: 130,
            command: command.into(),
            asked_confirmation: true,
        };
    }

    let spawn = Command::new("bash")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawn {
        Ok(c) => c,
        Err(e) => {
            return ExecResult {
                ok: false,
                output: String::new(),
                error: format!("failed to start command: {e}"),
                returncode: 127,
                command: command.into(),
                asked_confirmation: risky,
            };
        }
    };
    finish(
        &mut child,
        Duration::from_secs(cfg.timeout_secs),
        command,
        risky,
    )
}
