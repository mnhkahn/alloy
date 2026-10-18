//! Follow only rollouts held open by live Codex processes. Old, abandoned
//! task_started records on disk must never keep the computer awake forever.
use super::{Agent, command};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Default)]
struct Rollout {
    offset: u64,
    turn: Option<String>,
    cwd: String,
    id: String,
}
impl Rollout {
    fn read(&mut self, file: File) -> Result<(), String> {
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
            let count = reader
                .read_until(b'\n', &mut line)
                .map_err(|e| e.to_string())?;
            // A writer can be halfway through a record. Retry it next poll.
            if count == 0 || line.last() != Some(&b'\n') {
                break;
            }
            self.offset += count as u64;
            let value: Value = match serde_json::from_slice(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let payload = &value["payload"];
            if value["type"] == "session_meta" {
                self.cwd = payload["cwd"].as_str().unwrap_or_default().into();
                self.id = payload["id"].as_str().unwrap_or_default().into();
            }
            if value["type"] != "event_msg" {
                continue;
            }
            let id = payload["turn_id"].as_str().unwrap_or("");
            match payload["type"].as_str().unwrap_or("") {
                "task_started" => self.turn = Some(id.into()),
                "task_complete" | "task_completed" | "turn_aborted" => {
                    if id.is_empty() || self.turn.as_deref() == Some(id) {
                        self.turn = None;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Codex {
    files: HashMap<PathBuf, Rollout>,
}
impl Codex {
    pub fn scan(&mut self) -> (Vec<Agent>, Vec<String>) {
        let paths = match open_rollouts() {
            Ok(paths) => paths,
            Err(error) => return (vec![], vec![format!("Codex: {error}")]),
        };
        self.files.retain(|path, _| paths.contains(path));
        let mut active = Vec::new();
        let mut homes = HashSet::new();
        let mut errors = Vec::new();
        for path in paths {
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    errors.push(format!("Codex: {e}"));
                    continue;
                }
            };
            let rollout = self.files.entry(path.clone()).or_default();
            if let Err(error) = rollout.read(file) {
                errors.push(format!("Codex: {error}"));
                continue;
            }
            if rollout.turn.is_some() {
                if let Some(home) = path
                    .ancestors()
                    .find(|p| p.file_name().is_some_and(|n| n == "sessions"))
                    .and_then(Path::parent)
                {
                    homes.insert(home.to_path_buf());
                }
                active.push(Agent {
                    id: format!(
                        "codex:{}",
                        if rollout.id.is_empty() {
                            path.to_string_lossy().into_owned()
                        } else {
                            rollout.id.clone()
                        }
                    ),
                    name: "Codex".into(),
                    title: "工作中的 Codex 任务".into(),
                    project: rollout.cwd.clone(),
                    source: "Codex".into(),
                });
            }
        }
        for home in homes {
            if let Err(error) = enrich_titles(&home, &mut active) {
                errors.push(format!("Codex 标题: {error}"));
            }
        }
        (active, errors)
    }
}

fn enrich_titles(home: &Path, agents: &mut [Agent]) -> Result<(), String> {
    // Discover the current state schema version; only query metadata for live
    // agents. The database is never used to decide whether a task is working.
    let database = std::fs::read_dir(home)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = name
                .strip_prefix("state_")?
                .strip_suffix(".sqlite")?
                .parse::<u32>()
                .ok()?;
            Some((version, entry.path()))
        })
        .max_by_key(|(version, _)| *version)
        .map(|(_, path)| path);
    let Some(database) = database else {
        return Ok(());
    };
    let ids: Vec<String> = agents
        .iter()
        .filter_map(|agent| agent.id.strip_prefix("codex:"))
        .map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let query = format!(
        "SELECT id, COALESCE(NULLIF(name,''), title) AS title, agent_nickname FROM threads WHERE id IN ({})",
        ids.join(",")
    );
    let (status, bytes) = command::output(
        Command::new("/usr/bin/sqlite3")
            .args(["-batch", "-readonly", "-json"])
            .arg(database)
            .arg(&query),
    )?;
    if !status.success() {
        return Err(format!("Cannot read thread titles ({status})"));
    }
    if bytes.is_empty() {
        return Ok(());
    }
    let rows: Vec<Value> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    for row in rows {
        if let Some(agent) = agents
            .iter_mut()
            .find(|agent| agent.id.strip_prefix("codex:") == row["id"].as_str())
        {
            if let Some(title) = row["title"]
                .as_str()
                .filter(|title| !title.trim().is_empty())
            {
                agent.title = title.into();
            }
            if let Some(name) = row["agent_nickname"]
                .as_str()
                .filter(|name| !name.trim().is_empty())
            {
                agent.name = format!("Codex · {name}");
            }
        }
    }
    Ok(())
}

fn open_rollouts() -> Result<HashSet<PathBuf>, String> {
    let (status, bytes) =
        command::output(Command::new("/usr/sbin/lsof").args(["-n", "-P", "-c", "codex", "-Fn"]))?;
    // lsof returns 1 when no matching process has open files.
    if !status.success() && status.code() != Some(1) {
        return Err(format!("lsof failed: {status}"));
    }
    let mut paths = HashSet::new();
    for line in String::from_utf8_lossy(&bytes).lines() {
        if let Some(name) = line.strip_prefix('n') {
            let path = PathBuf::from(name);
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
            {
                paths.insert(path);
            }
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn reads_display_titles_and_nicknames_without_modifying_database() {
        let home = tempfile::tempdir().unwrap();
        let database = home.path().join("state_5.sqlite");
        let (status, _) = command::output(Command::new("/usr/bin/sqlite3").arg(&database).arg("CREATE TABLE threads(id TEXT, name TEXT, title TEXT, agent_nickname TEXT); INSERT INTO threads VALUES('a','修复登录','original prompt','Atlas'),('b',NULL,'优化页面',NULL);")).unwrap();
        assert!(status.success());
        let before = std::fs::read(&database).unwrap();
        let mut agents = ["a", "b"].map(|id| Agent {
            id: format!("codex:{id}"),
            name: "Codex".into(),
            ..Agent::default()
        });
        enrich_titles(home.path(), &mut agents).unwrap();
        assert_eq!(agents[0].title, "修复登录");
        assert_eq!(agents[0].name, "Codex · Atlas");
        assert_eq!(agents[1].title, "优化页面");
        assert_eq!(agents[1].name, "Codex");
        assert_eq!(std::fs::read(database).unwrap(), before);
    }
    #[test]
    fn follows_turn_lifecycle_and_partial_records() {
        let mut file = tempfile::tempfile().unwrap();
        let mut rollout = Rollout::default();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"a"}})
        )
        .unwrap();
        rollout.read(file.try_clone().unwrap()).unwrap();
        assert_eq!(rollout.turn.as_deref(), Some("a"));
        write!(
            file,
            "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"a\"}}"
        )
        .unwrap();
        rollout.read(file.try_clone().unwrap()).unwrap();
        assert!(rollout.turn.is_some());
        writeln!(file, "}}").unwrap();
        rollout.read(file.try_clone().unwrap()).unwrap();
        assert!(rollout.turn.is_none());
        for (kind, expected) in [
            ("task_started", true),
            ("turn_aborted", false),
            ("task_started", true),
            ("task_complete", false),
        ] {
            writeln!(
                file,
                "{}",
                serde_json::json!({"type":"event_msg","payload":{"type":kind,"turn_id":"b"}})
            )
            .unwrap();
            rollout.read(file.try_clone().unwrap()).unwrap();
            assert_eq!(rollout.turn.is_some(), expected);
        }
    }
}
