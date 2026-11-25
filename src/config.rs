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
    pub fn load(path: Option<&str>) -> Config {
        let p = match path {
            Some(p) => std::path::PathBuf::from(p),
            None => dirs::config_dir()
                .map(|d| d.join("voxbox/config.toml"))
                .unwrap_or_else(|| std::path::PathBuf::from("config.toml")),
        };
        let mut config = match std::fs::read(&p) {
            Ok(bytes) => {
                toml::from_str::<Config>(&String::from_utf8_lossy(&bytes)).unwrap_or_default()
            }
            Err(_) => Config::default(),
        };
        if let Ok(key) = std::env::var("VOXBOX_API_KEY") {
            config.brain.api_key = key;
        }
        config
    }
}
