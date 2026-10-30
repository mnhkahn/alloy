//! Herdr's public read-only JSON API includes every workspace in a session.
use super::Agent;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) fn scan() -> (Vec<Agent>, Vec<String>) {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return (vec![], vec!["HOME is unavailable".into()]);
    };
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let mut sockets = HashSet::new();
    if let Some(path) = std::env::var_os("HERDR_SOCKET_PATH") {
        sockets.insert(PathBuf::from(path));
    }
    for name in ["herdr", "herdr-dev"] {
        let root = config.join(name);
        let default = root.join("herdr.sock");
        if default.exists() {
            sockets.insert(default);
        }
        if let Ok(sessions) = std::fs::read_dir(root.join("sessions")) {
            for session in sessions.flatten() {
                let socket = session.path().join("herdr.sock");
                if socket.exists() {
                    sockets.insert(socket);
                }
            }
        }
    }
    let mut active = Vec::new();
    let mut errors = Vec::new();
    for socket in sockets {
        match query(&socket) {
            Ok(mut value) => match exclude_quota_blocked(&mut value, &socket)
                .and_then(|()| working_agents(&value, &socket))
            {
                Ok(agents) => active.extend(agents),
                Err(error) => errors.push(format!("Herdr {}: {error}", socket.display())),
            },
            // Old sockets can remain after a server exits.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                ) => {}
            Err(error) => errors.push(format!("Herdr {}: {error}", socket.display())),
        }
    }
    (active, errors)
}

#[cfg(unix)]
fn query(path: &Path) -> std::io::Result<Value> {
    request(path, "agent.list", json!({}))
}

#[cfg(unix)]
fn request(path: &Path, method: &str, params: Value) -> std::io::Result<Value> {
    let mut socket = std::os::unix::net::UnixStream::connect(path)?;
    let timeout = Some(Duration::from_millis(400));
    socket.set_read_timeout(timeout)?;
    socket.set_write_timeout(timeout)?;
    writeln!(
        socket,
        "{}",
        json!({"id":"alloy-keep-awake","method":method,"params":params})
    )?;
    let mut response = String::new();
    BufReader::new(socket.take(4 * 1024 * 1024)).read_line(&mut response)?;
    serde_json::from_str(&response)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
#[cfg(not(unix))]
fn query(_: &Path) -> std::io::Result<Value> {
    Err(std::io::Error::other("Herdr sockets require Unix"))
}

#[cfg(not(unix))]
fn request(_: &Path, _: &str, _: Value) -> std::io::Result<Value> {
    Err(std::io::Error::other("Herdr sockets require Unix"))
}

fn exclude_quota_blocked(value: &mut Value, socket: &Path) -> Result<(), String> {
    filter_quota_blocked(value, |pane| {
        request(
            socket,
            "pane.read",
            json!({
                "pane_id": pane, "source": "recent", "lines": 16,
                "format": "text", "strip_ansi": true
            }),
        )
        .ok()
    })
}

fn filter_quota_blocked(
    value: &mut Value,
    mut read: impl FnMut(&str) -> Option<Value>,
) -> Result<(), String> {
    let agents = value["result"]["agents"]
        .as_array_mut()
        .ok_or("Invalid agent.list response")?;
    for agent in agents {
        if agent["agent_status"] != "working" {
            continue;
        }
        let Some(pane) = agent["pane_id"].as_str().filter(|id| !id.is_empty()) else {
            continue;
        };
        // Hooks and spinner detection can leave a terminal marked working after
        // its provider refuses further requests. Read only the current tail;
        // never search session history for errors from earlier turns.
        let Some(snapshot) = read(pane) else { continue };
        if snapshot["result"]["read"]["text"]
            .as_str()
            .is_some_and(quota_blocked)
        {
            agent["agent_status"] = json!("blocked");
        }
    }
    Ok(())
}

fn quota_blocked(text: &str) -> bool {
    // Match provider diagnostics, not mentions of quota in a task or the normal
    // usage meter. A short 429/backoff by itself must still hold awake.
    for line in text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(16)
    {
        let line = line.trim().to_lowercase();
        let message = line.trim_start_matches(|c: char| !c.is_alphanumeric());
        let message = message
            .strip_prefix("llm provider error: ")
            .or_else(|| message.strip_prefix("error: "))
            .unwrap_or(message);
        if [
            "quota exceeded",
            "usage limit reached",
            "usage limit exceeded",
            "you've hit your usage limit",
            "you have reached your usage limit",
            "insufficient balance",
            "credit balance is too low",
            "membership expired, please renew",
            "额度已耗尽",
            "额度已用尽",
            "已达到使用限额",
            "已超出使用限额",
        ]
        .iter()
        .any(|prefix| message.starts_with(prefix))
        {
            return true;
        }
        // A fresh Kimi progress line below an old diagnostic indicates recovery.
        if message.starts_with("thinking...")
            || message.starts_with("working...")
            || message.starts_with("using ")
        {
            return false;
        }
    }
    false
}

fn field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        value[*key]
            .as_str()
            .filter(|value| !value.trim().is_empty())
    })
}
fn working_agents(value: &Value, socket: &Path) -> Result<Vec<Agent>, String> {
    let agents = value["result"]["agents"]
        .as_array()
        .ok_or("Invalid agent.list response")?;
    Ok(agents
        .iter()
        .filter(|agent| agent["agent_status"] == "working")
        .map(|agent| {
            let project = field(agent, &["foreground_cwd", "cwd"]).unwrap_or_default();
            let session = &agent["agent_session"];
            let id = if session["agent"] == "codex" && session["kind"] == "id" {
                session["value"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(|id| format!("codex:{id}"))
            } else {
                None
            }
            .unwrap_or_else(|| {
                format!(
                    "herdr:{}:{}",
                    socket.display(),
                    agent["terminal_id"].as_str().unwrap_or_default()
                )
            });
            Agent {
                id,
                name: field(agent, &["name", "display_agent", "agent"])
                    .unwrap_or_default()
                    .into(),
                // Terminal titles often contain only the executable name (e.g.
                // "OpenCode"). They are not task titles; leave absent titles blank.
                title: field(agent, &["title"])
                    .filter(|title| {
                        !["agent", "display_agent", "name"].iter().any(|key| {
                            agent[*key]
                                .as_str()
                                .is_some_and(|name| name.trim().eq_ignore_ascii_case(title.trim()))
                        })
                    })
                    .unwrap_or_default()
                    .into(),
                project: project.into(),
                source: "Herdr".into(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quota_diagnostics_release_hold_and_progress_restores_it() {
        for diagnostic in [
            "Quota exceeded, please upgrade your plan or retry later\nServer: 403\n❯",
            "LLM provider error: usage limit reached. Resets in 5 hours\n❯",
            "Error: You've hit your usage limit. Try again later",
            "✗ 额度已耗尽，请稍后重试",
        ] {
            assert!(quota_blocked(diagnostic), "{diagnostic}");
            assert!(!quota_blocked(&format!("{diagnostic}\n⠋ Thinking...")));
        }
        for normal in [
            "5h quota: 0% left", // usage display alone isn't a stopped request
            "Please handle quota exceeded errors in this project",
            "Rate limit reached, retrying in 2 seconds",
            "HTTP 429 Too Many Requests",
            "Thinking...",
        ] {
            assert!(!quota_blocked(normal), "{normal}");
        }
        let old_error = format!("Quota exceeded\n{}", "new output\n".repeat(16));
        assert!(!quota_blocked(&old_error));
    }

    #[test]
    fn quota_filter_only_suppresses_affected_pane_and_recovers_next_poll() {
        let original = json!({"result":{"agents":[
            {"agent_status":"working","pane_id":"quota","terminal_id":"1"},
            {"agent_status":"working","pane_id":"busy","terminal_id":"2"},
            {"agent_status":"idle","pane_id":"idle"},
            {"agent_status":"working","terminal_id":"legacy"}
        ]}});
        let mut value = original.clone();
        let mut reads = Vec::new();
        filter_quota_blocked(&mut value, |pane| {
            reads.push(pane.to_string());
            Some(json!({"result":{"read":{"text": if pane == "quota" {
                "Quota exceeded, please upgrade your plan or retry later"
            } else { "Thinking..." }}}}))
        })
        .unwrap();
        assert_eq!(reads, ["quota", "busy"]);
        assert_eq!(
            working_agents(&value, Path::new("/test.sock"))
                .unwrap()
                .len(),
            2
        );
        // Every poll re-evaluates live text, so no timeout or manual reset is needed.
        let mut resumed = original.clone();
        filter_quota_blocked(&mut resumed, |_| {
            Some(json!({"result":{"read":{"text":"Thinking..."}}}))
        })
        .unwrap();
        assert_eq!(
            working_agents(&resumed, Path::new("/test.sock"))
                .unwrap()
                .len(),
            3
        );
        for unavailable in [None, Some(json!({"error":{"message":"unsupported"}}))] {
            let mut value = original.clone();
            filter_quota_blocked(&mut value, |_| unavailable.clone()).unwrap();
            assert_eq!(
                working_agents(&value, Path::new("/test.sock"))
                    .unwrap()
                    .len(),
                3
            );
        }
    }
    #[test]
    fn preserves_agent_identity_name_and_task_title() {
        let value = json!({"result":{"agents":[{"terminal_id":"one","agent_status":"working","name":"Reviewer","agent":"codex","title":"Review login","cwd":"/project","agent_session":{"agent":"codex","kind":"id","value":"thread-a"}},{"terminal_id":"two","agent_status":"working","display_agent":"Claude Code","title":"Fix login","terminal_title_stripped":"Claude Code","cwd":"/project"}]}});
        let agents = working_agents(&value, Path::new("/session/herdr.sock")).unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].id, "codex:thread-a");
        assert_eq!(agents[0].name, "Reviewer");
        assert_eq!(agents[0].title, "Review login");
        assert_eq!(agents[1].name, "Claude Code");
        assert_eq!(agents[1].title, "Fix login");
        assert_ne!(agents[0].id, agents[1].id);
    }
    #[test]
    fn missing_fields_stay_blank_and_tool_name_is_not_a_task_title() {
        let value = json!({"result":{"agents":[{"terminal_id":"one","agent_status":"working","agent":"opencode","title":"OpenCode","terminal_title_stripped":"OpenCode","cwd":"/project"},{"terminal_id":"two","agent_status":"working"}]}});
        let agents = working_agents(&value, Path::new("/session/herdr.sock")).unwrap();
        assert_eq!(agents[0].name, "opencode");
        assert!(agents[0].title.is_empty());
        assert_eq!(agents[0].project, "/project");
        assert!(agents[1].name.is_empty());
        assert!(agents[1].title.is_empty());
        assert!(agents[1].project.is_empty());
    }
    #[test]
    fn only_working_agents_hold_awake() {
        let value = json!({"result":{"agents":[{"agent_status":"idle"},{"agent_status":"blocked"},{"agent_status":"working","cwd":"/project-a"},{"agent_status":"working","cwd":"/project-b"}]}});
        assert_eq!(
            working_agents(&value, Path::new("/test.sock"))
                .unwrap()
                .iter()
                .map(|agent| agent.project.as_str())
                .collect::<Vec<_>>(),
            ["/project-a", "/project-b"]
        );
        assert!(
            working_agents(
                &json!({"error":{"message":"failed"}}),
                Path::new("/test.sock")
            )
            .is_err()
        );
    }
    #[cfg(unix)]
    #[test]
    fn reads_real_socket_protocol() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&request).unwrap()["method"],
                "agent.list"
            );
            writeln!(
                stream,
                "{}",
                json!({"result":{"agents":[{"agent_status":"working","pane_id":"w1:p1","cwd":"/project"}]}})
            )
            .unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["method"], "pane.read");
            assert_eq!(request["params"]["pane_id"], "w1:p1");
            assert_eq!(request["params"]["lines"], 16);
            assert_eq!(request["params"]["strip_ansi"], true);
            writeln!(
                stream,
                "{}",
                json!({"result":{"read":{"text":
                    "Quota exceeded, please upgrade your plan or retry later"
                }}})
            )
            .unwrap();
        });
        let mut value = query(&path).unwrap();
        assert_eq!(
            working_agents(&value, &path).unwrap()[0].project,
            "/project"
        );
        exclude_quota_blocked(&mut value, &path).unwrap();
        assert!(working_agents(&value, &path).unwrap().is_empty());
        server.join().unwrap();
    }
}
