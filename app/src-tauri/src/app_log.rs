use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_ENTRIES: usize = 2_000;
const MAX_MESSAGE_CHARS: usize = 4_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" | "warning" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            _ => Err("日志级别无效".into()),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub timestamp_ms: u64,
    pub level: LogLevel,
    pub source: String,
    pub message: String,
}

#[derive(Default)]
struct LogBuffer {
    next_id: u64,
    entries: VecDeque<LogEntry>,
}

#[derive(Clone)]
pub struct AppLogState {
    buffer: Arc<Mutex<LogBuffer>>,
    spool_path: Arc<PathBuf>,
}

impl Default for AppLogState {
    fn default() -> Self {
        Self::new(std::env::temp_dir().join("LangbaiH3Studio").join("logs"))
    }
}

impl AppLogState {
    pub fn new(root: PathBuf) -> Self {
        let _ = fs::create_dir_all(&root);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            buffer: Arc::new(Mutex::new(LogBuffer::default())),
            spool_path: Arc::new(root.join(format!("session-{nonce}.jsonl"))),
        }
    }

    pub fn append(&self, level: LogLevel, source: &str, message: &str) -> LogEntry {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut buffer = self.buffer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        buffer.next_id = buffer.next_id.saturating_add(1);
        let entry = LogEntry {
            id: buffer.next_id,
            timestamp_ms,
            level,
            source: sanitize(source),
            message: sanitize(message),
        };
        buffer.entries.push_back(entry.clone());
        while buffer.entries.len() > MAX_ENTRIES {
            buffer.entries.pop_front();
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(self.spool_path.as_ref()) {
            if let Ok(json) = serde_json::to_string(&entry) {
                let _ = writeln!(file, "{json}");
            }
        }
        entry
    }

    pub fn list(&self) -> Vec<LogEntry> {
        self.buffer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .iter()
            .cloned()
            .collect()
    }

    pub fn clear(&self) {
        self.buffer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .clear();
    }

    fn spool_path(&self) -> &Path { self.spool_path.as_ref() }
}

fn sanitize(value: &str) -> String {
    let mut text: String = value.chars().take(MAX_MESSAGE_CHARS).collect();
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let home = home.to_string_lossy();
        if !home.is_empty() {
            text = text.replace(home.as_ref(), "%USERPROFILE%");
        }
    }
    for marker in ["api_key=", "apikey=", "token=", "authorization:", "bearer "] {
        text = redact_after_marker(&text, marker);
    }
    text
}

fn redact_after_marker(value: &str, marker: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let Some(start) = lower.find(marker) else {
        return value.to_string();
    };
    let secret_start = start + marker.len();
    let tail = &value[secret_start..];
    let secret_len = tail
        .find(|character: char| character.is_whitespace() || matches!(character, '&' | ',' | ';'))
        .unwrap_or(tail.len());
    format!("{}{}<已隐藏>{}", &value[..start], &value[start..secret_start], &tail[secret_len..])
}

pub fn save(state: &AppLogState, destination: &Path, errors_only: bool) -> Result<PathBuf, String> {
    if !destination.is_absolute()
        || !matches!(destination.extension().and_then(|value| value.to_str()), Some("log" | "txt"))
    {
        return Err("请选择以 .log 或 .txt 结尾的绝对保存路径".into());
    }
    let mut output = String::from("Langbai H3 Studio 运行日志\n");
    output.push_str("敏感凭据和用户目录已自动隐藏。\n\n");
    let file = fs::File::open(state.spool_path()).map_err(|error| format!("读取本次会话日志失败：{error}"))?;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| format!("读取会话日志失败：{error}"))?;
        let entry: LogEntry = serde_json::from_str(&line).map_err(|error| format!("会话日志损坏：{error}"))?;
        if errors_only && entry.level != LogLevel::Error { continue; }
        output.push_str(&format!(
            "[{}] [{}] [{}] {}\n",
            entry.timestamp_ms,
            entry.level.label(),
            entry.source,
            entry.message.replace('\n', " ↩ ")
        ));
    }
    let parent = destination.parent().ok_or_else(|| "日志保存目录无效".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建日志目录失败：{error}"))?;
    let temporary = destination.with_extension("log.tmp");
    fs::write(&temporary, output).map_err(|error| format!("写入日志失败：{error}"))?;
    if destination.exists() {
        fs::remove_file(destination).map_err(|error| format!("替换已有日志失败：{error}"))?;
    }
    fs::rename(&temporary, destination).map_err(|error| format!("保存日志失败：{error}"))?;
    Ok(destination.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_is_bounded_and_secrets_are_redacted() {
        let state = AppLogState::default();
        for index in 0..2_010 {
            state.append(LogLevel::Info, "test", &format!("row {index} token=secret"));
        }
        let entries = state.list();
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert!(!entries.last().unwrap().message.contains("secret"));
        assert!(entries.last().unwrap().message.contains("<已隐藏>"));
    }

    #[test]
    fn errors_only_export_excludes_other_levels() {
        let state = AppLogState::default();
        state.append(LogLevel::Info, "runtime", "started");
        state.append(LogLevel::Error, "runtime", "failed");
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("errors.log");
        save(&state, &destination, true).unwrap();
        let raw = fs::read_to_string(destination).unwrap();
        assert!(raw.contains("failed"));
        assert!(!raw.contains("started"));
    }

    #[test]
    fn clearing_ui_ring_keeps_full_session_spool() {
        let root = tempfile::tempdir().unwrap();
        let state = AppLogState::new(root.path().join("spool"));
        state.append(LogLevel::Info, "runtime", "before clear");
        state.clear();
        assert!(state.list().is_empty());
        let destination = root.path().join("full.log");
        save(&state, &destination, false).unwrap();
        assert!(fs::read_to_string(destination).unwrap().contains("before clear"));
    }
}
