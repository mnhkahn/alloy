//! Follow Kimi native turn events for sessions identified by its own hooks.
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Default)]
struct Wire {
    offset: u64,
    turn: Option<Value>,
    waiting: HashSet<String>,
    known: bool,
}

impl Wire {
    fn read(&mut self, file: File) -> Result<bool, String> {
        if file.metadata().map_err(|e| e.to_string())?.len() < self.offset {
            *self = Self::default();
        }
        let mut reader = BufReader::new(file);
        reader
            .seek(SeekFrom::Start(self.offset))
            .map_err(|e| e.to_string())?;
        let mut line = Vec::new();
        loop {
            line.clear();
            reader
                .read_until(b'\n', &mut line)
                .map_err(|e| e.to_string())?;
            // Retry a partially written record on the next poll.
            if line.last() != Some(&b'\n') {
                break;
            }
            let value: Value = serde_json::from_slice(&line).map_err(|e| e.to_string())?;
            if value["agentId"] != "main" {
                self.offset += line.len() as u64;
                continue;
            }
            match value["type"].as_str().unwrap_or_default() {
                "turn.prompt" => {
                    if value["turnId"].is_null() {
                        return Err("turn.prompt is missing turnId".into());
                    }
                    self.turn = Some(value["turnId"].clone());
                    self.waiting.clear();
                    self.known = true;
                }
                "turn.ended" if self.turn.as_ref() == Some(&value["turnId"]) => {
                    self.turn = None;
                    self.waiting.clear();
                }
                "interaction.request" if self.turn.is_some() => {
                    if let Some(id) = value["id"].as_str() {
                        self.waiting.insert(id.into());
                    }
                }
                "interaction.resolved" => {
                    if let Some(id) = value["id"].as_str() {
                        self.waiting.remove(id);
                    }
                }
                _ => {}
            }
            self.offset += line.len() as u64;
        }
        if !self.known {
            return Err("no supported Kimi turn events found".into());
        }
        Ok(self.turn.is_some() && self.waiting.is_empty())
    }
}

#[derive(Default)]
pub(super) struct Kimi {
    wires: HashMap<PathBuf, Wire>,
}

impl Kimi {
    pub fn inspect(&mut self, home: &Path, id: &str) -> Result<(bool, String, String), String> {
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err("invalid Kimi session identity".into());
        }
        let root = home.join("sessions");
        let path = std::fs::read_dir(root)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|workspace| workspace.path().join(id))
            .find(|path| path.join("state.json").is_file())
            .ok_or("session not found")?;
        let state: Value = serde_json::from_reader(
            File::open(path.join("state.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if state["id"] != id {
            return Err("session identity mismatch".into());
        }
        let wire = path.join("agents/main/wire.jsonl");
        let working = self
            .wires
            .entry(wire.clone())
            .or_default()
            .read(File::open(wire).map_err(|e| e.to_string())?)?;
        Ok((
            working,
            state["title"].as_str().unwrap_or_default().into(),
            state["cwd"].as_str().unwrap_or_default().into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn append(path: &Path, value: Value) {
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(file, "{value}").unwrap();
    }

    #[test]
    fn native_turn_lifecycle_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("sessions/workspace/session_test");
        std::fs::create_dir_all(session.join("agents/main")).unwrap();
        std::fs::write(session.join("state.json"), json!({"id":"session_test", "title":"Native task", "cwd":"/project", "lastTurnReason":"completed"}).to_string()).unwrap();
        let path = session.join("agents/main/wire.jsonl");
        std::fs::write(&path, "").unwrap();
        let mut kimi = Kimi::default();
        append(
            &path,
            json!({"type":"turn.prompt", "agentId":"main", "turnId":6}),
        );
        assert_eq!(
            kimi.inspect(dir.path(), "session_test").unwrap(),
            (true, "Native task".into(), "/project".into())
        );
        // Renaming to a completion-like title cannot end an active turn.
        std::fs::write(
            session.join("state.json"),
            json!({"id":"session_test", "title":"已完成", "cwd":"/project"}).to_string(),
        )
        .unwrap();
        assert!(kimi.inspect(dir.path(), "session_test").unwrap().0);
        append(
            &path,
            json!({"type":"turn.ended", "agentId":"main", "turnId":6,"reason":"completed"}),
        );
        assert!(!kimi.inspect(dir.path(), "session_test").unwrap().0);
        std::fs::write(
            session.join("state.json"),
            json!({"id":"session_test", "title":"Thinking... Working...", "cwd":"/project"})
                .to_string(),
        )
        .unwrap();
        assert!(!kimi.inspect(dir.path(), "session_test").unwrap().0);
        append(
            &path,
            json!({"type":"turn.prompt", "agentId":"main", "turnId":7}),
        );
        assert!(kimi.inspect(dir.path(), "session_test").unwrap().0);
        std::fs::remove_file(&path).unwrap();
        assert!(kimi.inspect(dir.path(), "session_test").is_err());
        assert!(kimi.inspect(dir.path(), "../escape").is_err());
    }

    #[test]
    fn waiting_partial_records_and_old_turn_endings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wire.jsonl");
        std::fs::write(&path, "").unwrap();
        let mut wire = Wire::default();
        let read = |wire: &mut Wire| wire.read(File::open(&path).unwrap()).unwrap();
        append(
            &path,
            json!({"type":"turn.prompt","agentId":"main","turnId":1}),
        );
        assert!(read(&mut wire));
        append(
            &path,
            json!({"type":"turn.ended","agentId":"child","turnId":1}),
        );
        append(
            &path,
            json!({"type":"turn.ended","agentId":"main","turnId":0}),
        );
        assert!(read(&mut wire));
        append(
            &path,
            json!({"type":"interaction.request","agentId":"main","id":"approval"}),
        );
        assert!(!read(&mut wire));
        append(
            &path,
            json!({"type":"interaction.resolved","agentId":"main","id":"approval"}),
        );
        assert!(read(&mut wire));
        let end = json!({"type":"turn.ended","agentId":"main","turnId":1,"reason":"cancelled"})
            .to_string();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        write!(file, "{}", &end[..10]).unwrap();
        assert!(read(&mut wire));
        writeln!(file, "{}", &end[10..]).unwrap();
        assert!(!read(&mut wire));
        std::fs::write(&path, "").unwrap();
        append(
            &path,
            json!({"type":"turn.prompt","agentId":"main","turnId":2}),
        );
        assert!(read(&mut wire));
        append(&path, json!({"type":"turn.prompt","agentId":"main"}));
        for _ in 0..2 {
            assert!(wire.read(File::open(&path).unwrap()).is_err());
        }
    }
}
