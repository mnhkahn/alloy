//! Minimal stdio JSON-RPC client for ACP agents.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(180);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::AcpAgent;

    #[test]
    fn manual_command_preserves_quoted_arguments_and_literal_text() {
        let agent: AcpAgent = serde_json::from_value(json!({
            "name": "custom",
            "command": "\"/path with spaces/agent\" acp --label 'work repo' --literal '$(echo nope)'"
        })).unwrap();
        assert_eq!(
            command_args(&agent).unwrap(),
            [
                "/path with spaces/agent",
                "acp",
                "--label",
                "work repo",
                "--literal",
                "$(echo nope)"
            ]
        );
    }

    #[test]
    fn legacy_path_keeps_spaces_as_one_program_argument() {
        let agent: AcpAgent = serde_json::from_value(json!({
            "name": "old", "path": "/path with spaces/xiaoli"
        }))
        .unwrap();
        assert_eq!(
            command_args(&agent).unwrap(),
            ["/path with spaces/xiaoli", "acp"]
        );
    }

    #[test]
    fn malformed_or_empty_commands_are_rejected() {
        for command in ["\"unfinished", "''", ""] {
            let agent: AcpAgent = serde_json::from_value(json!({
                "name": "invalid", "command": command
            }))
            .unwrap();
            assert!(command_args(&agent).is_err());
        }
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn send_request(
    input: &mut std::process::ChildStdin,
    request_id: &mut u64,
    method: &str,
    params: Value,
) -> Result<u64, String> {
    *request_id += 1;
    let id = *request_id;
    serde_json::to_writer(
        &mut *input,
        &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )
    .map_err(|e| e.to_string())?;
    input
        .write_all(b"\n")
        .and_then(|_| input.flush())
        .map_err(|e| format!("Could not write ACP agent request: {e}"))?;
    Ok(id)
}

pub fn command_args(agent: &crate::projects::AcpAgent) -> Result<Vec<String>, String> {
    if !agent.command.trim().is_empty() {
        let args = shlex::split(&agent.command)
            .ok_or("ACP command contains an unclosed quote or escape.")?;
        if args.first().is_none_or(|program| program.is_empty()) {
            return Err("Enter an ACP command.".into());
        }
        Ok(args)
    } else if !agent.path.trim().is_empty() {
        Ok(vec![agent.path.trim().to_owned(), "acp".into()])
    } else {
        Err("Enter an ACP command.".into())
    }
}

pub fn resolve_executable(program: &str) -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path = if let Some(relative) = program.strip_prefix("~/") {
        home.as_ref()
            .ok_or("Home directory unavailable.")?
            .join(relative)
    } else {
        PathBuf::from(program)
    };
    let executable = |path: &Path| {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    };
    if path.components().count() > 1 || path.is_absolute() {
        return if executable(&path) {
            Ok(path)
        } else {
            Err(format!(
                "ACP executable is missing or not executable: {}",
                path.display()
            ))
        };
    }
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default();
    if let Some(home) = home {
        for directory in [".opencode/bin", ".npm-global/bin", "go/bin", ".local/bin"] {
            directories.push(home.join(directory));
        }
    }
    directories
        .extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    directories
        .into_iter()
        .map(|directory| directory.join(&path))
        .find(|candidate| executable(candidate))
        .ok_or_else(|| format!("ACP executable '{program}' was not found. Enter its full path."))
}

fn configured_agent() -> Result<crate::projects::AcpAgent, String> {
    let settings = crate::projects::Projects::read(&crate::projects::store_path()?)?;
    let agent = settings
        .acp_agents
        .iter()
        .find(|agent| agent.name == settings.selected_acp)
        .ok_or("No ACP agent is configured in Alloy settings.")?;
    Ok(agent.clone())
}

/// Start the configured ACP agent, send one stateless staged-diff request and return
/// its final assistant text. The caller retains ownership of the git commit.
pub fn generate(cwd: &Path, system: &str, prompt: &str) -> Result<String, String> {
    let agent = configured_agent()?;
    let args = command_args(&agent)?;
    let binary = resolve_executable(&args[0])?;
    let manual_command = !agent.command.trim().is_empty();
    let acp_system_prompt = format!(
        "你正在为 Alloy 生成 Git 提交信息。不要调用工具或运行命令，只返回提交信息纯文本。\n\n{system}"
    );
    let is_opencode = binary
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("opencode"));
    let is_xiaoli = binary
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("xiaoli"));
    let mut command = Command::new(&binary);
    command.args(&args[1..]);
    let prompt = if manual_command || is_opencode {
        if !manual_command {
            command.arg("--cwd").arg(cwd);
        }
        format!("{acp_system_prompt}\n\n{prompt}")
    } else {
        command.arg("-prompt").arg(&acp_system_prompt);
        prompt.to_owned()
    };
    let mut child = ChildGuard(
        command
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not start {} ACP: {e}", binary.display()))?,
    );
    let mut input = child
        .0
        .stdin
        .take()
        .ok_or("Could not open ACP agent input.")?;
    let output = child
        .0
        .stdout
        .take()
        .ok_or("Could not open ACP agent output.")?;
    let stderr = child
        .0
        .stderr
        .take()
        .ok_or("Could not open ACP agent diagnostics.")?;
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).trim().to_owned()
    });
    let (line_tx, line_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if line_tx.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = line_tx.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    let mut request_id = 0_u64;
    let read_response = |child: &mut Child,
                         stderr_reader: &mut Option<thread::JoinHandle<String>>,
                         input: &mut std::process::ChildStdin,
                         reader: &Receiver<Result<String, String>>,
                         id: u64,
                         is_prompt: bool,
                         recover_after_idle: bool,
                         text: &mut String,
                         started: Instant|
     -> Result<Value, String> {
        loop {
            if started.elapsed() > TIMEOUT {
                return Err("ACP agent timed out.".into());
            }
            let idle_window = Duration::from_secs(2);
            let wait_for = if is_prompt && recover_after_idle && !text.trim().is_empty() {
                idle_window.min(TIMEOUT.saturating_sub(started.elapsed()))
            } else {
                TIMEOUT.saturating_sub(started.elapsed())
            };
            let line = match reader.recv_timeout(wait_for) {
                Ok(Ok(line)) => line,
                Ok(Err(error)) => {
                    return Err(format!("Could not read ACP agent response: {error}"));
                }
                Err(RecvTimeoutError::Timeout)
                    if is_prompt && recover_after_idle && !text.trim().is_empty() =>
                {
                    // Some agents stream the requested answer then start a
                    // tool turn without sending an ACP completion event. For
                    // one-shot commit text, use the complete idle stream.
                    return Ok(json!({"stopReason":"end_turn"}));
                }
                Err(RecvTimeoutError::Timeout) => return Err("ACP agent timed out.".into()),
                Err(RecvTimeoutError::Disconnected) => {
                    let status = child.try_wait().ok().flatten();
                    if status.is_none() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    let stderr = stderr_reader
                        .take()
                        .and_then(|reader| reader.join().ok())
                        .filter(|text| !text.is_empty());
                    return Err(match (status, stderr) {
                        (Some(status), Some(stderr)) => format!(
                            "ACP agent closed the connection before replying (exit {status}): {stderr}"
                        ),
                        (Some(status), None) => {
                            format!(
                                "ACP agent closed the connection before replying (exit {status})."
                            )
                        }
                        (None, Some(stderr)) => {
                            format!("ACP agent closed the connection before replying: {stderr}")
                        }
                        (None, None) => "ACP agent closed the connection before replying.".into(),
                    });
                }
            };
            let msg: Value = serde_json::from_str(&line)
                .map_err(|e| format!("Invalid ACP agent response: {e}"))?;
            // ACP agents may ask the client to approve a tool call while a
            // prompt is running. Commit message generation must stay read-only,
            // so explicitly reject these requests instead of leaving the prompt
            // waiting forever for a UI permission dialog that Alloy does not have.
            if msg["method"] == "session/request_permission" {
                if let Some(id) = msg.get("id") {
                    let reject_option = msg["params"]["options"]
                        .as_array()
                        .and_then(|options| {
                            options.iter().find(|option| {
                                option["kind"] == "reject_once"
                                    || option["optionId"] == "reject-once"
                                    || option["optionId"] == "reject"
                            })
                        })
                        .and_then(|option| option["optionId"].as_str())
                        .ok_or("ACP permission request has no reject option.")?;
                    serde_json::to_writer(
                        &mut *input,
                        &json!({
                            "jsonrpc":"2.0",
                            "id":id,
                            "result":{"outcome":{"outcome":"selected","optionId":reject_option}}
                        }),
                    )
                    .map_err(|e| format!("Could not reject ACP agent tool request: {e}"))?;
                    input
                        .write_all(b"\n")
                        .and_then(|_| input.flush())
                        .map_err(|e| format!("Could not reply to ACP agent tool request: {e}"))?;
                }
                continue;
            }
            if msg["method"] == "session/update" {
                let update = &msg["params"]["update"];
                if recover_after_idle
                    && update["sessionUpdate"] == "tool_call"
                    && !text.trim().is_empty()
                {
                    // An agent occasionally emits the requested text and then
                    // starts a tool turn. Alloy's commit flow is read-only; use
                    // the completed text and stop the ACP process at that point.
                    return Ok(json!({"stopReason":"end_turn"}));
                }
                if update["sessionUpdate"] == "agent_message_chunk" {
                    if let Some(chunk) = msg["params"]["update"]["content"]["text"].as_str() {
                        text.push_str(chunk);
                    }
                }
            }
            if msg["id"].as_u64() == Some(id) {
                if let Some(error) = msg.get("error") {
                    return Err(format!(
                        "ACP agent error: {}",
                        error["message"].as_str().unwrap_or("unknown error")
                    ));
                }
                return Ok(msg["result"].clone());
            }
        }
    };
    let start = Instant::now();
    let init = send_request(
        &mut input,
        &mut request_id,
        "initialize",
        json!({"protocolVersion":1,"clientInfo":{"name":"Alloy","version":env!("CARGO_PKG_VERSION")},"clientCapabilities":{}}),
    )?;
    let mut stderr_reader = Some(stderr_reader);
    read_response(
        &mut child.0,
        &mut stderr_reader,
        &mut input,
        &line_rx,
        init,
        false,
        false,
        &mut String::new(),
        start,
    )?;
    let session = send_request(
        &mut input,
        &mut request_id,
        "session/new",
        json!({"cwd":cwd,"mcpServers":[]}),
    )?;
    let session_result = read_response(
        &mut child.0,
        &mut stderr_reader,
        &mut input,
        &line_rx,
        session,
        false,
        false,
        &mut String::new(),
        start,
    )?;
    let session_id = session_result["sessionId"]
        .as_str()
        .ok_or("ACP agent did not return a session ID.")?
        .to_owned();
    if is_opencode {
        let options = session_result["configOptions"]
            .as_array()
            .and_then(|configs| configs.iter().find(|config| config["id"] == "model"))
            .and_then(|config| config["options"].as_array());
        let free_model = options
            .and_then(|options| {
                options
                    .iter()
                    .find(|option| option["value"] == "openrouter/openrouter/free")
                    .or_else(|| {
                        options.iter().find(|option| {
                            option["value"] == "opencode/big-pickle"
                                || option["value"].as_str().is_some_and(|value| {
                                    value.starts_with("opencode/") && value.ends_with("-free")
                                })
                        })
                    })
            })
            .and_then(|option| option["value"].as_str())
            .ok_or("OpenCode has no free model available for this workspace.")?;
        let model_selection = send_request(
            &mut input,
            &mut request_id,
            "session/set_config_option",
            json!({"sessionId":session_id,"configId":"model","value":free_model}),
        )?;
        read_response(
            &mut child.0,
            &mut stderr_reader,
            &mut input,
            &line_rx,
            model_selection,
            false,
            false,
            &mut String::new(),
            start,
        )?;
    }
    let prompt_id = send_request(
        &mut input,
        &mut request_id,
        "session/prompt",
        json!({"sessionId":session_id,"prompt":[{"type":"text","text":prompt}]}),
    )?;
    let mut answer = String::new();
    read_response(
        &mut child.0,
        &mut stderr_reader,
        &mut input,
        &line_rx,
        prompt_id,
        true,
        is_xiaoli,
        &mut answer,
        start,
    )?;
    let _ = child.0.kill();
    let _ = child.0.wait();
    if let Some(stderr_reader) = stderr_reader {
        let _ = stderr_reader.join();
    }
    if answer.trim().is_empty() {
        Err("ACP agent returned no commit message.".into())
    } else {
        Ok(answer)
    }
}
