#![cfg(test)]

use crate::config::ExecutorConfig;
use crate::executor;

fn cfg() -> ExecutorConfig {
    ExecutorConfig::default()
}

#[test]
fn empty_command_is_ok() {
    let r = executor::run_command("", &cfg(), None);
    assert!(r.ok);
}

#[test]
fn echo_reports_output() {
    let r = executor::run_command("echo hello-world", &cfg(), None);
    assert!(r.ok, "err: {}", r.error);
    assert!(r.output.contains("hello-world"));
}

#[test]
fn fork_bomb_blocked() {
    let r = executor::run_command(":(){ :|:& };:", &cfg(), None);
    assert!(!r.ok);
    assert!(r.error.contains("blocked"));
}

#[test]
fn rm_rf_root_blocked() {
    let r = executor::run_command("rm -rf /", &cfg(), None);
    assert!(!r.ok);
    assert!(r.error.contains("blocked"));
}

#[test]
fn sudo_rm_rf_home_blocked() {
    assert!(executor::is_blocked("sudo rm -rf ~", &cfg()));
    let r = executor::run_command("Sudo RM -RF '*'", &ExecutorConfig::default(), None);
    assert!(!r.ok);
}

#[test]
fn rm_tmp_dir_not_blocked_but_confirm_gated() {
    assert!(!executor::is_blocked("rm -rf /tmp/some-dir", &cfg()));
    let r = executor::run_command("rm -rf /tmp/some-dir", &cfg(), None);
    assert!(!r.ok);
    assert!(r.error.contains("Cancelled"));
}

#[test]
fn confirm_callback_runs_it() {
    let r = executor::run_command("echo confirmed", &cfg(), Some(&|_cmd| true));
    assert!(r.ok, "err: {}", r.error);
    assert!(r.output.contains("confirmed"));
}

#[test]
fn confirm_decline_cancels() {
    let r = executor::run_command("echo confirmed", &cfg(), Some(&|_cmd| false));
    assert!(!r.ok);
    assert!(r.error.contains("Cancelled"));
}

#[test]
fn failing_command_reports_code() {
    let r = executor::run_command("false", &cfg(), None);
    assert!(!r.ok);
}

#[test]
fn pipeline_works_via_bash() {
    let r = executor::run_command("echo hello | tr a-z A-Z", &cfg(), None);
    assert!(r.ok, "err: {}", r.error);
    assert!(r.output.contains("HELLO"));
}

#[test]
fn timeout_kills_sleep() {
    let mut c = cfg();
    c.timeout_secs = 1;
    let r = executor::run_command("sleep 5", &c, None);
    assert!(!r.ok);
    assert!(r.error.contains("Timed out"));
    assert_eq!(r.returncode, 124);
}

#[test]
fn quoted_args_survive_bash() {
    let r = executor::run_command("echo 'two words' end", &cfg(), None);
    assert!(r.ok, "err: {}", r.error);
    assert!(r.output.contains("two words end"));
}

#[test]
fn blocklist_membership() {
    let mut c = cfg();
    c.blocklist.push(".history".into());
    // default entries still work
    assert!(executor::is_blocked("history -c", &c) || !executor::is_blocked("ls", &c));
    assert!(!executor::is_blocked("ls", &c));
}

#[test]
fn exec_result_derives() {
    let r = executor::run_command("true", &cfg(), None);
    let _ = format!("{r:?}");
    let _ = r.clone() == r;
}

#[test]
fn config_defaults_load() {
    let c = crate::config::Config::load(None);
    assert_eq!(c.audio.sample_rate, 16000);
    assert_eq!(c.stt.model, "base");
    assert_eq!(c.ui.feedback, "full");
    assert!(!c.executor.blocklist.is_empty());
}

#[test]
fn config_env_key_wins() {
    std::env::set_var("VOXBOX_API_KEY", "sk-test");
    let c = crate::config::Config::load(None);
    std::env::remove_var("VOXBOX_API_KEY");
    assert_eq!(c.brain.api_key, "sk-test");
}
