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

#[test]
fn config_merge_recovers_per_section() {
    let raw = "[stt]\nmodel = \"tiny\"\n[dictation]\nkey = \"pause\"\nbad line here!!\n[tts]\nengine = \"piper\"\n";
    let (cfg, warnings) = crate::config::Config::merge(raw);
    assert_eq!(cfg.stt.model, "tiny");
    assert_eq!(cfg.tts.engine, "piper");
    assert_eq!(cfg.dictation.key, "scrolllock", "bad dictation section must default");
    assert_eq!(warnings.len(), 2, "exactly recovery + dictation warnings, got {warnings:?}");
    assert!(warnings[1].contains("[dictation]"), "{warnings:?}");
}

#[test]
fn config_merge_clean_file_is_silent() {
    let (cfg, warnings) = crate::config::Config::merge("model = \"base\"\n[dictation]\nkey = \"f13\"\n");
    assert_eq!(cfg.dictation.key, "f13");
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn config_merge_empty_is_default() {
    let (cfg, warnings) = crate::config::Config::merge("");
    assert_eq!(cfg.dictation.key, "scrolllock");
    assert!(warnings.is_empty());
}

#[test]
fn config_set_subkey_replaces_existing() {
    let raw = "[tts]\nengine = \"piper\"\nvoice = \"old\"\n[ui]\nfeedback = \"full\"\n";
    let out = crate::config::set_subkey(raw, "tts", "voice", "voice = \"new\"");
    assert!(out.contains("voice = \"new\"",), "{out}");
    assert!(!out.contains("old"), "{out}");
    assert!(out.contains("engine = \"piper\""));
    assert!(out.contains("feedback = \"full\""));
}

#[test]
fn config_set_subkey_inserts_and_appends() {
    let out = crate::config::set_subkey(
        "[tts]\nvoice = \"x\"\n",
        "tts",
        "enabled",
        "enabled = false",
    );
    assert!(out.contains("enabled = false"), "{out}");
    let out = crate::config::set_subkey("[ui]\nfeedback = \"full\"\n", "tts", "voice", "voice = \"y\"");
    assert!(out.contains("[tts]"), "{out}");
    assert!(out.contains("voice = \"y\""));
    assert!(crate::config::set_subkey("", "tts", "voice", "voice = \"z\"").contains("[tts]"));
}

#[test]
fn voice_helpers_label_and_pick() {
    use crate::studio::{installed_voices, voice_label, voice_pick};
    assert_eq!(voice_label("en_US-lessac-medium"), "EN/US — lessac (medium)");
    let dir = std::env::temp_dir().join("voxbox-voice-test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("en_GB-alba-medium.onnx"), b"x").unwrap();
    std::fs::write(dir.join("en_US-lessac-medium.onnx"), b"x").unwrap();
    std::fs::write(dir.join("not-a-voice.txt"), b"x").unwrap();
    let voices = installed_voices(&dir);
    assert_eq!(voices, vec!["en_GB-alba-medium", "en_US-lessac-medium"]);
    let (idx, summary) = voice_pick(&voices, &format!("{}/en_GB-alba-medium.onnx", dir.display()));
    assert_eq!(idx, 1);
    assert!(summary.contains("alba"));
    assert_eq!(voice_pick(&voices, "").0, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn list_agents_reads_and_skips_invalid() {
    use crate::config::{list_agents_in, AgentConfig};
    let dir = std::env::temp_dir().join("voxbox-agents-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("b-good.toml"),
        "name = \"bartender\"\napi_base = \"http://localhost:8080/v1\"\nmodel = \"qwen\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("a-good.toml"),
        "name = \"apothecary\"\napi_base = \"http://localhost:9000/v1\"\napi_key = \"k\"\nmodel = \"m\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("broken.toml"), "name = \"\"").unwrap();
    std::fs::write(dir.join("ignore.txt"), "name = \"x\"\napi_base = \"y\"").unwrap();
    let agents: Vec<AgentConfig> = list_agents_in(&dir);
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].name, "apothecary", "agents are sorted by name");
    assert_eq!(agents[1].model, "qwen");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_agents_dir_is_empty_not_error() {
    use crate::config::list_agents_in;
    let agents = list_agents_in(std::path::Path::new("/nonexistent/voxbox-agents"));
    assert!(agents.is_empty());
}

#[test]
fn tile_pointer_is_testable_purely() {
    use crate::studio::{TileEvent, TilePointer, DOUBLE_MS, HOLD_MS};
    assert!(DOUBLE_MS < HOLD_MS);
    let mut p = TilePointer::new();
    let t0 = std::time::Instant::now();
    assert_eq!(p.step(true, false, t0, false), None);
    assert_eq!(
        p.step(
            false,
            true,
            t0 + std::time::Duration::from_millis(60),
            false
        ),
        None
    );
    assert_eq!(TileEvent::Toggle, TileEvent::Toggle);
}

#[test]
fn hallucination_bracketed_noise_is_filtered() {
    use crate::stt::is_hallucination;
    // The exact garbage the user saw typed in a quiet house:
    assert!(is_hallucination("[Music]"));
    assert!(is_hallucination("[MUSIC PLAYING]"));
    assert!(is_hallucination("(upbeat music)"));
    assert!(is_hallucination("What music is playing?"));
    assert!(is_hallucination("[Music]  What music is playing?"));
    assert!(is_hallucination("[Music] Hello?".replace("Hello?", "").trim()));
    assert!(is_hallucination("\u{266A} \u{266A}"));
    assert!(is_hallucination(""));
    assert!(is_hallucination("   "));
    assert!(is_hallucination("Thank you."));
    assert!(is_hallucination("Thanks for watching!"));
    assert!(is_hallucination("SUBSCRIBE"));
}

#[test]
fn hallucination_real_speech_survives() {
    use crate::stt::is_hallucination;
    // Real words must always pass through, even with noise markers around them.
    assert!(!is_hallucination("Hello?"));
    assert!(!is_hallucination("[Music]  Hello?"));
    assert!(!is_hallucination("And so, my fellow Americans, ask not what your country can do for you."));
    assert!(!is_hallucination("open the terminal"));
    assert!(!is_hallucination("(upbeat music) turn off the lights"));
    assert!(!is_hallucination("What music is playing in the other room?"));
}

#[test]
fn config_merge_duplicate_sections_heals() {
    // The user's actual broken file shape: example [tts] + installer-appended [tts].
    use crate::config::merge_duplicate_sections;
    let raw = "[stt]\nmodel = \"base\"\n\n[tts]\nengine = \"auto\"\nvoice = \"\"\nenabled = true\n\n[dictation]\nkey = \"pause\"\n\n[tts]\nengine = \"piper\"\nvoice = \"/home/Architect/.local/share/voxbox/voices/en_US-lessac-medium.onnx\"\nenabled = true\n";
    let healed = merge_duplicate_sections(raw);
    let (cfg, warnings) = crate::config::Config::merge(&healed);
    assert!(warnings.is_empty(), "healed file must parse whole: {warnings:?}");
    assert_eq!(cfg.tts.engine, "piper", "later occurrence wins");
    assert_eq!(
        cfg.tts.voice,
        "/home/Architect/.local/share/voxbox/voices/en_US-lessac-medium.onnx"
    );
    assert_eq!(cfg.stt.model, "base");
    assert_eq!(cfg.dictation.key, "pause");
    assert_eq!(healed.matches("[tts]").count(), 1, "exactly one [tts] table");
    // Set_subkey on the healed file must not resurrect the duplicate.
    let again = crate::config::set_subkey(&healed, "tts", "voice", "voice = \"x.onnx\"");
    assert_eq!(again.matches("[tts]").count(), 1);
    assert!(again.contains("voice = \"x.onnx\""));
}

#[test]
fn config_merge_warns_on_duplicate_table() {
    let raw = "[tts]\nengine = \"auto\"\n[tts]\nengine = \"piper\"\n";
    let (_, warnings) = crate::config::Config::merge(raw);
    assert!(
        warnings.iter().any(|w| w.contains("duplicate [tts]")),
        "expected a duplicate-table warning, got {warnings:?}"
    );
}

#[test]
fn config_merge_clean_file_has_no_duplicate_warning() {
    let (_, warnings) = crate::config::Config::merge("[tts]\nengine = \"auto\"\n[stt]\nmodel = \"base\"\n");
    assert!(warnings.is_empty(), "{warnings:?}");
}
