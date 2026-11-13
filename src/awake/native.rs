//! Native integrations register sessions without any terminal-host dependency.
//! Titles are display metadata only; lifecycle state never depends on their text.
use super::{Agent, command, kimi};
use serde::Deserialize;
use std::{collections::HashMap, path::Path, process::Command};

#[derive(Deserialize)]
struct Record {
    version: u32,
    provider: String,
    session_id: String,
    pid: u32,
    process_started: String,
    status: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    project: String,
    updated_at: f64,
}

#[derive(Default)]
pub(super) struct Native {
    kimi: kimi::Kimi,
}
impl Native {
    pub fn scan(&mut self) -> (Vec<Agent>, Vec<String>) {
        let Some(home) = std::env::var_os("HOME") else {
            return (vec![], vec!["HOME is unavailable".into()]);
        };
        let home = Path::new(&home);
        if !home.join(".alloy/agents").exists() && !home.join(".claude/sessions").exists() {
            return (vec![], vec![]);
        }
        match processes() {
            Ok(processes) => self.scan_at(home, &processes),
            Err(error) => (vec![], vec![error]),
        }
    }

    fn scan_claude_only(
        &mut self,
        home: &Path,
        processes: &HashMap<u32, String>,
    ) -> (Vec<Agent>, Vec<String>) {
        let mut records = HashMap::new();
        let mut errors = Vec::new();
        self.claude_records(home, processes, &mut records, &mut errors);
        (
            records
                .into_iter()
                .filter(|(_, r)| r.status == "working")
                .map(|(id, r)| Agent {
                    id,
                    name: "Claude Code".into(),
                    title: r.title,
                    project: r.project,
                    source: "Claude Code native".into(),
                })
                .collect(),
            errors,
        )
    }

    fn claude_records(
        &self,
        home: &Path,
        processes: &HashMap<u32, String>,
        records: &mut HashMap<String, Record>,
        errors: &mut Vec<String>,
    ) {
        let Ok(entries) = std::fs::read_dir(home.join(".claude/sessions")) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(pid) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            if path.extension().is_none_or(|s| s != "json") || !processes.contains_key(&pid) {
                continue;
            }
            let value: serde_json::Value = match std::fs::read(&path)
                .ok()
                .filter(|b| b.len() <= 64 * 1024)
                .and_then(|b| serde_json::from_slice(&b).ok())
            {
                Some(value) => value,
                None => {
                    errors.push(format!("Claude Code 状态不可用: {}", path.display()));
                    continue;
                }
            };
            let Some(started) = value["procStart"].as_str() else {
                continue;
            };
            let started = started.split_whitespace().collect::<Vec<_>>().join(" ");
            if value["pid"].as_u64() != Some(pid as u64) || processes.get(&pid) != Some(&started) {
                continue;
            }
            let Some(id) = value["sessionId"].as_str().filter(|id| !id.is_empty()) else {
                continue;
            };
            let status = match value["status"].as_str() {
                Some("busy") => "working",
                Some("idle" | "waiting" | "shell") => "idle",
                _ => continue,
            };
            let title = if value["nameSource"] == "derived" {
                ""
            } else {
                value["name"].as_str().unwrap_or_default()
            };
            records.insert(
                format!("claude:{id}"),
                Record {
                    version: 1,
                    provider: "claude".into(),
                    session_id: id.into(),
                    pid,
                    process_started: started,
                    status: status.into(),
                    title: title.into(),
                    project: value["cwd"].as_str().unwrap_or_default().into(),
                    updated_at: 0.0,
                },
            );
        }
    }

    fn scan_at(
        &mut self,
        home: &Path,
        processes: &HashMap<u32, String>,
    ) -> (Vec<Agent>, Vec<String>) {
        let mut errors = Vec::new();
        let mut records: HashMap<String, Record> = HashMap::new();
        let entries = match std::fs::read_dir(home.join(".alloy/agents")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return self.scan_claude_only(home, processes);
            }
            Err(error) => return (vec![], vec![format!("Agent 状态不可用: {error}")]),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let record = (|| {
                let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
                if file.metadata().map_err(|e| e.to_string())?.len() > 64 * 1024 {
                    return Err("agent record is too large".into());
                }
                serde_json::from_reader::<_, Record>(file).map_err(|e| e.to_string())
            })();
            let record = match record {
                Ok(record) => record,
                Err(error) => {
                    errors.push(format!("Agent {}: {error}", path.display()));
                    continue;
                }
            };
            if record.version != 1
                || record.session_id.is_empty()
                || record.process_started.is_empty()
                || !record.updated_at.is_finite()
                || !matches!(record.provider.as_str(), "kimi" | "claude" | "opencode")
            {
                errors.push(format!("Unsupported agent record: {}", path.display()));
                continue;
            }
            // A reused PID must never resurrect an abandoned working record.
            if processes.get(&record.pid) != Some(&record.process_started) {
                continue;
            }
            let id = format!("{}:{}", record.provider, record.session_id);
            if records
                .get(&id)
                .is_none_or(|old| old.updated_at <= record.updated_at)
            {
                records.insert(id, record);
            }
        }
        self.claude_records(home, processes, &mut records, &mut errors);
        let mut agents = Vec::new();
        for (id, record) in records {
            let mut working = record.status == "working";
            let mut title = record.title;
            let mut project = record.project;
            if record.provider == "kimi" {
                match self
                    .kimi
                    .inspect(&home.join(".kimi-code"), &record.session_id)
                {
                    Ok((active, native_title, cwd)) => {
                        working = active;
                        title = native_title;
                        project = cwd;
                    }
                    Err(error) => {
                        if working {
                            errors.push(format!("Kimi 状态不可用: {error}"));
                        }
                        working = false;
                    }
                }
            }
            if !working {
                continue;
            }
            let name = match record.provider.as_str() {
                "kimi" => "Kimi",
                "claude" => "Claude Code",
                "opencode" => "OpenCode",
                _ => unreachable!(),
            };
            agents.push(Agent {
                id,
                name: name.into(),
                title,
                project,
                source: format!("{name} native"),
            });
        }
        agents.sort_by(|a, b| a.id.cmp(&b.id));
        (agents, errors)
    }
}

fn processes() -> Result<HashMap<u32, String>, String> {
    let (status, bytes) = command::output(
        Command::new("/bin/ps")
            .args(["-axo", "pid=,lstart="])
            .env("LC_ALL", "C")
            .env("TZ", "UTC"),
    )?;
    if !status.success() {
        return Err(format!("Agent process discovery failed: {status}"));
    }
    Ok(String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let started = fields.collect::<Vec<_>>().join(" ");
            Some((pid, started))
        })
        .collect())
}

/// Explicit setup command, also available from a packaged Alloy executable.
/// No provider configuration is changed just by starting the application.
pub fn install_integrations() -> Result<(), String> {
    let home = std::env::var_os("HOME").ok_or("HOME is unavailable")?;
    let assets = Path::new(&home).join(".alloy/integrations");
    std::fs::create_dir_all(&assets).map_err(|e| e.to_string())?;
    for (name, content) in [
        (
            "agent-hook.py",
            include_str!("../../integrations/agent-hook.py"),
        ),
        (
            "opencode-plugin.js",
            include_str!("../../integrations/opencode-plugin.js"),
        ),
    ] {
        std::fs::write(assets.join(name), content).map_err(|e| e.to_string())?;
    }
    let result = Command::new("python3")
        .args([
            "-c",
            include_str!("../../integrations/install.py"),
            "--assets",
        ])
        .arg(assets)
        .status()
        .map_err(|e| e.to_string())?;
    if result.success() {
        Ok(())
    } else {
        Err(format!("Integration setup failed: {result}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn misleading_or_missing_titles_never_change_lifecycle_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".alloy/agents");
        std::fs::create_dir_all(&root).unwrap();
        let processes = HashMap::from([(123, "start".into())]);
        let mut native = Native::default();
        for provider in ["claude", "opencode"] {
            for title in [
                "",
                "Working...",
                "Thinking...",
                "已完成",
                "idle",
                "等待审批",
            ] {
                for status in ["working", "idle", "waiting"] {
                    std::fs::write(root.join("event.json"), json!({"version":1,"provider":provider,"session_id":"session","pid":123,"process_started":"start","status":status,"title":title,"updated_at":1}).to_string()).unwrap();
                    let (agents, errors) = native.scan_at(dir.path(), &processes);
                    assert!(errors.is_empty());
                    assert_eq!(
                        agents.len(),
                        usize::from(status == "working"),
                        "{provider} {status} {title}"
                    );
                }
            }
        }
    }

    #[test]
    fn claude_native_status_discovers_sessions_and_overrides_stale_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".claude/sessions");
        std::fs::create_dir_all(&root).unwrap();
        let processes = HashMap::from([(123, "Fri Nov 13 1:00:00 2026".into())]);
        let mut native = Native::default();
        let write = |status: &str, start: &str| {
            std::fs::write(root.join("123.json"), json!({"pid":123,"sessionId":"direct","cwd":"/project","procStart":start,"status":status,"name":"Fix issue"}).to_string()).unwrap();
        };
        write("busy", "Fri Nov 13  1:00:00 2026");
        let (agents, errors) = native.scan_at(dir.path(), &processes);
        assert!(errors.is_empty());
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].title, "Fix issue");
        let hooks = dir.path().join(".alloy/agents");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(hooks.join("stale.json"), json!({"version":1,"provider":"claude","session_id":"direct","pid":123,"process_started":"Fri Nov 13 1:00:00 2026","status":"working","updated_at":1}).to_string()).unwrap();
        for status in ["idle", "waiting", "shell"] {
            write(status, "Fri Nov 13 1:00:00 2026");
            assert!(native.scan_at(dir.path(), &processes).0.is_empty());
        }
        // PID reuse must not discover an unrelated native session.
        std::fs::remove_dir_all(hooks).unwrap();
        write("busy", "old process");
        assert!(native.scan_at(dir.path(), &processes).0.is_empty());
    }

    #[test]
    fn multiple_native_sessions_and_pid_reuse_are_handled_without_a_terminal_host() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".alloy/agents");
        std::fs::create_dir_all(&root).unwrap();
        let write = |name: &str,
                     provider: &str,
                     session: &str,
                     status: &str,
                     pid: u32,
                     started: &str,
                     time: u32| {
            std::fs::write(root.join(format!("{name}.json")), json!({"version":1,"provider":provider,"session_id":session,"pid":pid,"process_started":started,"status":status,"title":"Native task","project":"/same-project","updated_at":time}).to_string()).unwrap();
        };
        write("a", "opencode", "one", "working", 1, "start", 1);
        write("b", "claude", "two", "working", 2, "start", 1);
        write("c", "claude", "three", "waiting", 2, "start", 1);
        write("d", "claude", "dead", "working", 3, "start", 1);
        write("e", "opencode", "reused", "working", 4, "old", 1);
        let processes =
            HashMap::from([(1, "start".into()), (2, "start".into()), (4, "new".into())]);
        let mut native = Native::default();
        let (agents, errors) = native.scan_at(dir.path(), &processes);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(agents.len(), 2);
        assert!(
            agents
                .iter()
                .all(|a| a.project == "/same-project" && a.title == "Native task")
        );
        // A new idle event wins over an older busy record of the same session.
        write("f", "opencode", "one", "idle", 1, "start", 2);
        assert_eq!(native.scan_at(dir.path(), &processes).0.len(), 1);
        write("b", "claude", "two", "idle", 2, "start", 2);
        assert!(native.scan_at(dir.path(), &processes).0.is_empty());
        // Truncated writes and unknown schemas are errors, never busy agents.
        std::fs::write(root.join("bad.json"), "{").unwrap();
        assert_eq!(native.scan_at(dir.path(), &processes).1.len(), 1);
    }
}
