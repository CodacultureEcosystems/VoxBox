use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SttConfig {
    pub model: String,
    pub language: String,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            model: "base".into(),
            language: "en".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub silence_ms: u64,
    pub max_seconds: u64,
    pub threshold: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            silence_ms: 900,
            max_seconds: 20,
            threshold: 0.004,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    pub engine: String,
    pub voice: String,
    pub enabled: bool,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            engine: "auto".into(),
            voice: String::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BrainConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            api_base: String::new(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DictationConfig {
    pub key: String,
    pub injector: String,
    pub notifications: bool,
}

impl Default for DictationConfig {
    fn default() -> Self {
        Self {
            key: "scrolllock".into(),
            injector: "auto".into(),
            notifications: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ExecutorConfig {
    pub timeout_secs: u64,
    pub blocklist: Vec<String>,
    pub confirm_patterns: Vec<String>,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 20,
            blocklist: default_blocklist(),
            confirm_patterns: default_confirm_patterns(),
        }
    }
}

fn default_blocklist() -> Vec<String> {
    [
        "mkfs",
        "dd if=",
        ":(){",
        "wget -O- | sh",
        "curl -L | sh",
        "chmod -R 777 /",
        ">/dev/sda",
        "history -c",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_confirm_patterns() -> Vec<String> {
    [
        "rm ", "shutdown", "reboot", "poweroff", "systemctl stop",
        "systemctl disable", "kill ", "pkill", "killall", "apt remove",
        "dnf remove", "pacman -R", "mv ", "cp -r", "sudo", "chown", "git push",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub feedback: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            feedback: "full".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub stt: SttConfig,
    pub audio: AudioConfig,
    pub tts: TtsConfig,
    pub dictation: DictationConfig,
    pub brain: BrainConfig,
    pub executor: ExecutorConfig,
    pub ui: UiConfig,
}

impl Config {
    fn merge_section<T: for<'de> Deserialize<'de> + Default>(
        value: Option<&toml::Value>,
        name: &str,
        warnings: &mut Vec<String>,
    ) -> T {
        match value {
            Some(v) => match T::deserialize(v.clone()) {
                Ok(parsed) => parsed,
                Err(e) => {
                    warnings.push(format!("[{name}] ignored, invalid entries: {e}"));
                    T::default()
                }
            },
            None => T::default(),
        }
    }

    /// Parse config text; a bad section is reported and replaced with
    /// defaults instead of discarding the whole file.
    pub fn merge(raw: &str) -> (Config, Vec<String>) {
        let mut warnings = Vec::new();
        if let Ok(c) = toml::from_str::<Config>(raw) {
            return (c, warnings);
        }
        warnings.push("whole-file parse failed; recovering per section".into());
        let mut config = Config::default();
        if let Ok(v) = toml::from_str::<toml::Value>(raw) {
            config.stt = Self::merge_section(v.get("stt"), "stt", &mut warnings);
            config.audio = Self::merge_section(v.get("audio"), "audio", &mut warnings);
            config.tts = Self::merge_section(v.get("tts"), "tts", &mut warnings);
            config.dictation = Self::merge_section(v.get("dictation"), "dictation", &mut warnings);
            config.brain = Self::merge_section(v.get("brain"), "brain", &mut warnings);
            config.executor = Self::merge_section(v.get("executor"), "executor", &mut warnings);
            config.ui = Self::merge_section(v.get("ui"), "ui", &mut warnings);
            return (config, warnings);
        }
        let mut blocks = section_blocks(raw);
        for (name, lines) in blocks.drain(..) {
            let body = lines.join("\n");
            match name.as_str() {
                "stt" => match toml::from_str::<SttConfig>(&body) {
                    Ok(c) => config.stt = c,
                    Err(e) => warnings.push(format!("[{name}] defaulted: {e}")),
                },
                "audio" => match toml::from_str::<AudioConfig>(&body) {
                    Ok(c) => config.audio = c,
                    Err(e) => warnings.push(format!("[{name}] defaulted: {e}")),
                },
                "tts" => match toml::from_str::<TtsConfig>(&body) {
                    Ok(c) => config.tts = c,
                    Err(e) => warnings.push(format!("[{name}] defaulted: {e}")),
                },
                "dictation" => match toml::from_str::<DictationConfig>(&body) {
                    Ok(c) => config.dictation = c,
                    Err(e) => warnings.push(format!("[{name}] defaulted: {e}")),
                },
                "brain" => match toml::from_str::<BrainConfig>(&body) {
                    Ok(c) => config.brain = c,
                    Err(e) => warnings.push(format!("[{name}] defaulted: {e}")),
                },
                "executor" => match toml::from_str::<ExecutorConfig>(&body) {
                    Ok(c) => config.executor = c,
                    Err(e) => warnings.push(format!("[{name}] defaulted: {e}")),
                },
                "ui" => match toml::from_str::<UiConfig>(&body) {
                    Ok(c) => config.ui = c,
                    Err(e) => warnings.push(format!("[{name}] defaulted: {e}")),
                },
                _ => {
                    if !name.contains('.') {
                        warnings.push(format!("[{name}] skipped (unknown section)"));
                    }
                }
            }
        }
        (config, warnings)
    }

    pub fn load(path: Option<&str>) -> Config {
        let p = match path {
            Some(p) => std::path::PathBuf::from(p),
            None => dirs::config_dir()
                .map(|d| d.join("voxbox/config.toml"))
                .unwrap_or_else(|| std::path::PathBuf::from("config.toml")),
        };
        let (mut config, warnings) = match std::fs::read_to_string(&p) {
            Ok(raw) => Self::merge(&raw),
            Err(_) => (Config::default(), Vec::new()),
        };
        for w in &warnings {
            eprintln!("  [config] {w}");
        }
        if let Ok(key) = std::env::var("VOXBOX_API_KEY") {
            config.brain.api_key = key;
        }
        config
    }
}

/// Split raw TOML into `(section name, body lines)` pairs, top-level tables
/// only. Lines before any header are dropped.
fn section_blocks(raw: &str) -> Vec<(String, Vec<String>)> {
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') && !t.starts_with("[[") {
            blocks.push((t[1..t.len() - 1].trim().to_string(), Vec::new()));
        } else if !blocks.is_empty() {
            blocks
                .last_mut()
                .expect("checked non-empty")
                .1
                .push(line.to_string());
        }
    }
    blocks
}
