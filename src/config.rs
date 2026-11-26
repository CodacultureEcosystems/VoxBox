use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SttConfig {
    pub engine: String,
    pub model: String,
    pub language: String,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            engine: "whisper".into(),
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

/// The user's config file path (same one `load` reads).
pub fn config_path() -> std::path::PathBuf {
    dirs::config_dir()
        .map(|d| d.join("voxbox/config.toml"))
        .unwrap_or_else(|| std::path::PathBuf::from("config.toml"))
}

/// One local AI agent declaration from `~/.config/voxbox/agents/*.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub api_base: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
}

/// Where agent files live: one `.toml` per agent.
pub fn agents_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("voxbox/agents")
}

/// Load agents from a directory; invalid files are skipped.
pub fn list_agents_in(dir: &std::path::Path) -> Vec<AgentConfig> {
    let mut agents: Vec<AgentConfig> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "toml").unwrap_or(false))
                .filter_map(|p| {
                    let raw = std::fs::read_to_string(&p).ok()?;
                    let agent: AgentConfig = toml::from_str(&raw).ok()?;
                    if agent.name.is_empty() || agent.api_base.is_empty() {
                        return None;
                    }
                    Some(agent)
                })
                .collect()
        })
        .unwrap_or_default();
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    agents
}

/// Load the agents the resident can route `[brain]` requests to.
pub fn list_agents() -> Vec<AgentConfig> {
    list_agents_in(&agents_dir())
}

/// Activate an agent: copy its settings into the `[brain]` section of the
/// user's config file, in place.
pub fn activate_agent(agent: &AgentConfig) -> Result<(), String> {
    let p = config_path();
    let raw = std::fs::read_to_string(&p).unwrap_or_default();
    let mut text = raw;
    text = set_subkey(&text, "brain", "api_base", &format!("api_base = \"{}\"", agent.api_base));
    text = set_subkey(&text, "brain", "model", &format!("model = \"{}\"", agent.model));
    if !agent.api_key.is_empty() {
        text = set_subkey(&text, "brain", "api_key", &format!("api_key = \"{}\"", agent.api_key));
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, text).map_err(|e| e.to_string())
}

/// Set `[stt]` settings (engine, model reference) in the user's config
/// file, in place.
pub fn save_stt(engine: Option<&str>, model: Option<&str>) -> Result<(), String> {
    let p = config_path();
    let raw = std::fs::read_to_string(&p).unwrap_or_default();
    let mut text = raw;
    if let Some(e) = engine {
        text = set_subkey(&text, "stt", "engine", &format!("engine = \"{e}\""));
    }
    if let Some(m) = model {
        text = set_subkey(&text, "stt", "model", &format!("model = \"{m}\""));
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, text).map_err(|e| e.to_string())
}

/// Read just the `[tts]` section as `(engine, voice, enabled)`, honoring the
/// same recovery rules as `merge` (never panics, never prints).
pub fn read_tts() -> (String, String, bool) {
    let raw = std::fs::read_to_string(config_path()).unwrap_or_default();
    let (c, _) = Config::merge(&raw);
    (c.tts.engine, c.tts.voice, c.tts.enabled)
}

/// Set `[tts]` settings by rewriting the user's config file in place;
/// untouched lines (including broken ones elsewhere) are preserved.
pub fn save_tts(
    engine: Option<&str>,
    voice: Option<&str>,
    enabled: Option<bool>,
) -> Result<(), String> {
    let p = config_path();
    let raw = std::fs::read_to_string(&p).unwrap_or_default();
    let mut text = raw;
    if let Some(e) = engine {
        text = set_subkey(&text, "tts", "engine", &format!("engine = \"{e}\""));
    }
    if let Some(v) = voice {
        text = set_subkey(&text, "tts", "voice", &format!("voice = \"{v}\""));
    }
    if let Some(b) = enabled {
        text = set_subkey(&text, "tts", "enabled", &format!("enabled = {b}"));
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, text).map_err(|e| e.to_string())
}

/// Rewrite/insert `value_line` for `key` inside `[section]` of raw TOML text,
/// keeping every other line byte-identical. Missing section is appended.
pub fn set_subkey(raw: &str, section: &str, key: &str, value_line: &str) -> String {
    fn is_header(t: &str) -> bool {
        t.starts_with('[') && t.ends_with(']') && !t.starts_with("[[")
    }
    fn header_name(t: &str) -> &str {
        t[1..t.len() - 1].trim()
    }
    fn is_our_key(t: &str, key: &str) -> bool {
        t.starts_with(key) && t[key.len()..].trim_start().starts_with('=')
    }

    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut header_idx: Option<usize> = None;
    let mut replaced = false;
    for line in raw.lines() {
        let t = line.trim();
        if is_header(t) {
            in_section = header_name(t) == section;
            if in_section && header_idx.is_none() {
                header_idx = Some(out.len());
            }
        }
        if in_section && !is_header(t) && is_our_key(t, key) {
            if !replaced {
                out.push(value_line.to_string());
                replaced = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    match (header_idx, replaced) {
        (Some(idx), false) => out.insert(idx + 1, value_line.to_string()),
        (None, false) => {
            if out.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
                out.push(String::new());
            }
            out.push(format!("[{section}]"));
            out.push(value_line.to_string());
        }
        _ => {}
    }
    let mut joined = out.join("\n");
    if !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
}
