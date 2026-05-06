// Persistent chat sessions. One JSON file per session under ~/.tiny/sessions/.
//
// TODO (deferred for v1):
//   - no garbage collection of old sessions
//   - no locking; two `tiny` instances can race on the same file
//   - no migration if the `Message` shape changes

use crate::agent::Message;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn generate() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let stamp = chrono_like_stamp();
        let suffix = format!("{:04x}", (now as u64) & 0xffff);
        Self(format!("{stamp}-{suffix}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub created_at: String,
    pub updated_at: String,
    pub model: String,
    pub title: String,
    pub history: Vec<Message>,
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id: SessionId,
    pub updated_at: String,
    pub title: String,
    pub model: String,
}

impl Session {
    pub fn new(model: impl Into<String>) -> Self {
        let now = chrono_like_stamp();
        Self {
            id: SessionId::generate(),
            created_at: now.clone(),
            updated_at: now,
            model: model.into(),
            title: String::new(),
            history: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = chrono_like_stamp();
    }

    pub fn ensure_title(&mut self) {
        if !self.title.is_empty() {
            return;
        }
        if let Some(Message::User(text)) = self.history.first() {
            self.title = title_from(text);
        }
    }
}

pub fn list() -> Result<Vec<SessionMeta>> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut metas = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match read_session(&path) {
            Ok(session) => metas.push(SessionMeta {
                id: session.id,
                updated_at: session.updated_at,
                title: session.title,
                model: session.model,
            }),
            Err(_) => continue,
        }
    }
    metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(metas)
}

pub fn load(id: &SessionId) -> Result<Session> {
    read_session(&path_for(id))
}

pub fn save(session: &Session) -> Result<()> {
    let dir = sessions_dir();
    ensure_dir(&dir)?;
    let final_path = path_for(&session.id);
    let tmp_path = final_path.with_extension("json.tmp");

    let data = serde_json::to_vec_pretty(session).context("serialize session")?;
    fs::write(&tmp_path, &data).with_context(|| format!("write {}", tmp_path.display()))?;
    set_file_mode(&tmp_path, 0o600);
    fs::rename(&tmp_path, &final_path)
        .with_context(|| format!("rename {} -> {}", tmp_path.display(), final_path.display()))?;
    Ok(())
}

fn read_session(path: &std::path::Path) -> Result<Session> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

fn sessions_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".tiny").join("sessions")
}

fn path_for(id: &SessionId) -> PathBuf {
    sessions_dir().join(format!("{}.json", id.as_str()))
}

fn ensure_dir(dir: &std::path::Path) -> Result<()> {
    if dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    set_file_mode(dir, 0o700);
    Ok(())
}

#[cfg(unix)]
fn set_file_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perm = meta.permissions();
        perm.set_mode(mode);
        let _ = fs::set_permissions(path, perm);
    }
}

#[cfg(not(unix))]
fn set_file_mode(_path: &std::path::Path, _mode: u32) {}

fn title_from(text: &str) -> String {
    const LIMIT: usize = 60;
    let trimmed = text.trim().replace('\n', " ");
    let mut chars = trimmed.chars();
    let head: String = chars.by_ref().take(LIMIT).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

fn chrono_like_stamp() -> String {
    // ISO-ish timestamp without bringing in chrono. UTC.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let (year, month, day, hour, minute, second) = unix_to_ymd_hms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}-{minute:02}-{second:02}")
}

fn unix_to_ymd_hms(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let seconds_of_day = (secs % 86_400) as u32;
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;

    // Civil-from-days algorithm by Howard Hinnant (public domain).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as u32, d as u32, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Message, ToolCall, ToolResult};
    use serde_json::json;

    #[test]
    fn session_round_trips_through_json() {
        // Internal tagging on Message would break tuple variants at runtime; this
        // round-trip catches that and any future shape regression.
        let session = Session {
            id: SessionId("test-id".into()),
            created_at: "2026-05-05T00-00-00".into(),
            updated_at: "2026-05-05T00-00-00".into(),
            model: "gpt-test".into(),
            title: "hello".into(),
            history: vec![
                Message::User("hi".into()),
                Message::Assistant {
                    text: "ok".into(),
                    tool_calls: vec![ToolCall {
                        id: "c1".into(),
                        name: "echo".into(),
                        input: json!({"text": "hi"}),
                    }],
                },
                Message::Tool(ToolResult {
                    id: "c1".into(),
                    content: "hi".into(),
                    is_error: false,
                }),
            ],
        };

        let bytes = serde_json::to_vec(&session).expect("serialize");
        let back: Session = serde_json::from_slice(&bytes).expect("deserialize");
        assert_eq!(back.history.len(), 3);
        assert!(matches!(back.history[0], Message::User(ref s) if s == "hi"));
    }
}
