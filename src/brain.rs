use serde_json::json;

use crate::config::BrainConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Execute a shell command line on the PC.
    Shell { payload: String, speak_result: bool, source: &'static str },
    /// Reply conversationally (spoken + printed).
    Say { payload: String, source: &'static str },
}

const SYSTEM_PROMPT: &str = r#"You are the brain of a Linux voice shell. The user's speech has been transcribed. Decide what to do and reply ONLY with JSON:
{"kind":"shell","payload":"<one shell command line>"}
or
{"kind":"say","payload":"<short spoken answer>"}
Use kind=shell when the user wants an action performed on the computer (open apps, run commands, control the system). Use kind=say for questions or conversation. The payload for shell must be a single plain command line with no explanations."#;

const APP_HINTS: &[(&str, &str)] = &[
    ("firefox", "firefox"),
    ("chrome", "chromium"),
    ("chromium", "chromium"),
    ("terminal", "x-terminal-emulator"),
    ("file manager", "xdg-open ."),
    ("files", "xdg-open ."),
    ("calculator", "gnome-calculator"),
    ("settings", "gnome-control-center"),
    ("code editor", "code"),
    ("vs code", "code"),
    ("editor", "xdg-open"),
];

pub fn clean_command_phrase(text: &str) -> String {
    let mut text = text.trim().trim_end_matches(['?', '.', '!']).trim().to_string();
    let lower = text.to_lowercase();
    for prefix in ["please ", "could you ", "can you ", "would you "] {
        if lower.starts_with(prefix) {
            text = text[prefix.len()..].trim().to_string();
            break;
        }
    }
    for prefix in ["run ", "execute ", "type ", "start ", "launch ", "open "] {
        let low = text.to_lowercase();
        if low.starts_with(prefix) {
            text = text[prefix.len()..].trim().to_string();
            break;
        }
    }
    let lower = text.to_lowercase();
    if lower.starts_with("command ") || lower.starts_with("shell ") {
        text = text[7..].to_string();
    } else if lower.starts_with("app ") || lower.starts_with("application ") {
        text = text[4..].to_string();
    }
    text.trim().to_string()
}

pub fn rules_decide(text: &str) -> Decision {
    let t = text.trim().to_lowercase();
    if t.is_empty() {
        return Decision::Say {
            payload: "I didn't catch that.".into(),
            source: "rules",
        };
    }
    if ["hello", "hi", "hey", "good morning", "good evening"]
        .iter()
        .any(|g| t == *g || t.starts_with(&format!("{g} ")))
    {
        return Decision::Say {
            payload: "Hello. Ready for a command.".into(),
            source: "greeting",
        };
    }
    if t == "what time is it" || t == "what time is it now" || t == "tell me the time" {
        return Decision::Shell {
            payload: "date '+%H:%M on %A, %d %B %Y'".into(),
            speak_result: true,
            source: "rules",
        };
    }
    for (phrase, cmd) in APP_HINTS {
        let open = format!("open {phrase}");
        let launch = format!("launch {phrase}");
        if t == open || t == launch {
            let background = if cmd.starts_with("xdg-open") { "" } else { " &" };
            return Decision::Shell {
                payload: format!("{cmd}{background}"),
                speak_result: false,
                source: "rules",
            };
        }
    }
    for prefix in ["run ", "execute ", "type "] {
        if t.starts_with(prefix) {
            return Decision::Shell {
                payload: clean_command_phrase(text),
                speak_result: true,
                source: "rules",
            };
        }
    }
    if t.starts_with("what ")
        || t.starts_with("who ")
        || t.starts_with("when ")
        || t.starts_with("why ")
        || t.starts_with("how ")
        || t.starts_with("tell me ")
        || t.starts_with("explain ")
        || t.starts_with("can you help")
    {
        return Decision::Say {
            payload: text.trim().to_string(),
            source: "chat",
        };
    }
    Decision::Shell {
        payload: clean_command_phrase(text),
        speak_result: true,
        source: "fallback",
    }
}

pub fn llm_decide(text: &str, cfg: &BrainConfig) -> Option<Decision> {
    let url = format!("{}/chat/completions", cfg.api_base.trim_end_matches('/'));
    let body = json!({
        "model": cfg.model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": text},
        ],
        "temperature": 0.2,
        "max_tokens": 300,
    });
    let mut req = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(30))
        .set("Content-Type", "application/json");
    if !cfg.api_key.is_empty() {
        req = req.set("Authorization", &format!("Bearer {}", cfg.api_key));
    }
    let resp = req.send_json(body).ok()?;
    let data: serde_json::Value = resp.into_json().ok()?;
    let content = data["choices"][0]["message"]["content"].as_str()?.trim().to_string();
    let start = content.find('{')?;
    let end = content.rfind('}')? + 1;
    let parsed: serde_json::Value = serde_json::from_str(&content[start..end]).ok()?;
    let payload = parsed["payload"].as_str()?.trim().to_string();
    match parsed["kind"].as_str()? {
        "shell" if !payload.is_empty() => Some(Decision::Shell {
            payload,
            speak_result: true,
            source: "llm",
        }),
        "say" if !payload.is_empty() => Some(Decision::Say { payload, source: "llm" }),
        _ => None,
    }
}

pub fn decide(text: &str, cfg: &BrainConfig) -> Decision {
    if !cfg.api_base.is_empty() && !cfg.model.is_empty() {
        if let Some(d) = llm_decide(text, cfg) {
            return d;
        }
    }
    rules_decide(text)
}

pub fn summarize_output(output: &str, limit: usize) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let joined = lines.iter().take(limit).cloned().collect::<Vec<_>>().join(". ");
    joined.chars().take(240).collect()
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_prefix_is_shell() {
        let d = rules_decide("run ls -la /tmp");
        match d {
            Decision::Shell { payload, .. } => assert_eq!(payload, "ls -la /tmp"),
            _ => panic!("expected shell"),
        }
    }

    #[test]
    fn greeting_says() {
        match rules_decide("hello there") {
            Decision::Say { payload, .. } => assert!(payload.contains("Hello")),
            _ => panic!("expected say"),
        }
    }

    #[test]
    fn open_app_spawns_background() {
        match rules_decide("open firefox") {
            Decision::Shell { payload, speak_result, .. } => {
                assert!(payload.ends_with(" &"));
                assert!(!speak_result);
            }
            _ => panic!("expected shell"),
        }
    }

    #[test]
    fn time_is_shell() {
        match rules_decide("what time is it") {
            Decision::Shell { payload, .. } => assert!(payload.contains("date")),
            _ => panic!("expected shell"),
        }
    }

    #[test]
    fn fallback_assumes_command() {
        match rules_decide("ls -l") {
            Decision::Shell { payload, .. } => assert_eq!(payload, "ls -l"),
            _ => panic!("expected shell"),
        }
    }

    #[test]
    fn question_is_chat() {
        assert!(matches!(rules_decide("what is an inode"), Decision::Say { .. }));
    }

    #[test]
    fn clean_strips_politeness() {
        assert_eq!(clean_command_phrase("please run df -h"), "df -h");
        assert_eq!(clean_command_phrase("could you open firefox"), "firefox");
    }

    #[test]
    fn summarize_takes_first_lines() {
        let out = (0..10).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
        let s = summarize_output(&out, 2);
        assert!(s.contains("line0"));
        assert!(s.contains("line1"));
        assert!(!s.contains("line9"));
    }

    #[test]
    fn llm_falls_back_to_rules() {
        let cfg = BrainConfig {
            api_base: "http://127.0.0.1:9/v1".into(),
            api_key: String::new(),
            model: "x".into(),
        };
        let d = decide("run uname -a", &cfg);
        assert_eq!(
            d,
            Decision::Shell {
                payload: "uname -a".into(),
                speak_result: true,
                source: "rules"
            }
        );
    }
}
