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
            Ok(value) => match working_agents(&value, &socket) {
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
    let mut socket = std::os::unix::net::UnixStream::connect(path)?;
    let timeout = Some(Duration::from_millis(400));
    socket.set_read_timeout(timeout)?;
    socket.set_write_timeout(timeout)?;
    writeln!(
        socket,
        "{}",
        json!({"id":"alloy-keep-awake","method":"agent.list","params":{}})
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
                    .unwrap_or("Herdr Agent")
                    .into(),
                title: field(
                    agent,
                    &["title", "terminal_title_stripped", "terminal_title"],
                )
                .or_else(|| {
                    Path::new(project)
                        .file_name()
                        .and_then(|name| name.to_str())
                })
                .unwrap_or("工作中的任务")
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
    fn preserves_agent_identity_name_and_task_title() {
        let value = json!({"result":{"agents":[{"terminal_id":"one","agent_status":"working","name":"Reviewer","agent":"codex","title":"Review login","cwd":"/project","agent_session":{"agent":"codex","kind":"id","value":"thread-a"}},{"terminal_id":"two","agent_status":"working","display_agent":"Claude Code","terminal_title_stripped":"Fix login","cwd":"/project"}]}});
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
                json!({"result":{"agents":[{"agent_status":"working","cwd":"/project"}]}})
            )
            .unwrap();
        });
        assert_eq!(
            working_agents(&query(&path).unwrap(), &path).unwrap()[0].project,
            "/project"
        );
        server.join().unwrap();
    }
}
