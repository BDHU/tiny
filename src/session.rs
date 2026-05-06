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
        let stamp = format!("{now}");
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_default();
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
        self.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_default();
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
    metas.sort_by(|a, b| {
        let an = a.updated_at.parse::<u128>().unwrap_or(0);
        let bn = b.updated_at.parse::<u128>().unwrap_or(0);
        bn.cmp(&an)
    });
    Ok(metas)
}

pub fn load(id: &SessionId) -> Result<Session> {
    read_session(&path_for(id))
}

pub fn save(session: &Session) -> Result<()> {
    let dir = sessions_dir();
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let final_path = path_for(&session.id);
    let tmp_path = final_path.with_extension("json.tmp");

    let data = serde_json::to_vec_pretty(session).context("serialize session")?;
    fs::write(&tmp_path, &data).with_context(|| format!("write {}", tmp_path.display()))?;
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
            created_at: "1234567890".into(),
            updated_at: "1234567890".into(),
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
