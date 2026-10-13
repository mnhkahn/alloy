//! Minimal ACP client for Xiaoli's stdio JSON-RPC interface.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(180);

fn executable() -> Result<PathBuf, String> {
    let settings = crate::projects::Projects::read(&crate::projects::store_path()?)?;
    let agent = settings
        .acp_agents
        .iter()
        .find(|agent| agent.name == settings.selected_acp)
        .ok_or("No ACP agent is configured in Alloy settings.")?;
    let path = PathBuf::from(agent.path.trim());
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "ACP executable for '{}' not found at {}. Open ACP settings to update it.",
            agent.name,
            path.display(),
        ))
    }
}

/// Start Xiaoli in ACP mode, send one stateless staged-diff request and return
/// its final assistant text. The caller retains ownership of the git commit.
pub fn generate(cwd: &Path, system: &str, prompt: &str) -> Result<String, String> {
    let binary = executable()?;
    let acp_system_prompt = format!(
        "你正在为 Alloy 生成 Git 提交信息。不要调用工具或运行命令，只返回提交信息纯文本。\n\n{system}"
    );
    let mut child = Command::new(&binary)
        .arg("acp")
        .arg("-prompt")
        .arg(acp_system_prompt)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start {} ACP: {e}", binary.display()))?;
    let mut input = child
        .stdin
        .take()
        .ok_or("Could not open Xiaoli ACP input.")?;
    let output = child
        .stdout
        .take()
        .ok_or("Could not open Xiaoli ACP output.")?;
    let mut reader = BufReader::new(output);
    let mut request_id = 0_u64;
    let mut send = |method: &str, params: Value| -> Result<u64, String> {
        request_id += 1;
        let id = request_id;
        serde_json::to_writer(
            &mut input,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )
        .map_err(|e| e.to_string())?;
        input
            .write_all(b"\n")
            .and_then(|_| input.flush())
            .map_err(|e| format!("Could not write Xiaoli ACP request: {e}"))?;
        Ok(id)
    };
    let read_response = |reader: &mut BufReader<_>,
                         id: u64,
                         text: &mut String,
                         started: Instant|
     -> Result<Value, String> {
        loop {
            if started.elapsed() > TIMEOUT {
                return Err("Xiaoli ACP timed out.".into());
            }
            let mut line = String::new();
            let n = reader
                .read_line(&mut line)
                .map_err(|e| format!("Could not read Xiaoli ACP response: {e}"))?;
            if n == 0 {
                return Err("Xiaoli ACP closed the connection before replying.".into());
            }
            let msg: Value = serde_json::from_str(&line)
                .map_err(|e| format!("Invalid Xiaoli ACP response: {e}"))?;
            if msg["method"] == "session/update"
                && msg["params"]["update"]["sessionUpdate"] == "agent_message_chunk"
            {
                if let Some(chunk) = msg["params"]["update"]["content"]["text"].as_str() {
                    text.push_str(chunk);
                }
            }
            if msg["id"].as_u64() == Some(id) {
                if let Some(error) = msg.get("error") {
                    return Err(format!(
                        "Xiaoli ACP error: {}",
                        error["message"].as_str().unwrap_or("unknown error")
                    ));
                }
                return Ok(msg["result"].clone());
            }
        }
    };
    let start = Instant::now();
    let init = send(
        "initialize",
        json!({"protocolVersion":1,"clientInfo":{"name":"Alloy","version":env!("CARGO_PKG_VERSION")},"clientCapabilities":{}}),
    )?;
    read_response(&mut reader, init, &mut String::new(), start)?;
    let session = send("session/new", json!({"cwd":cwd,"mcpServers":[]}))?;
    let session_id = read_response(&mut reader, session, &mut String::new(), start)?["sessionId"]
        .as_str()
        .ok_or("Xiaoli ACP did not return a session ID.")?
        .to_owned();
    let prompt_id = send(
        "session/prompt",
        json!({"sessionId":session_id,"prompt":[{"type":"text","text":prompt}]}),
    )?;
    let mut answer = String::new();
    read_response(&mut reader, prompt_id, &mut answer, start)?;
    drop(send);
    let _ = child.kill();
    let _ = child.wait();
    if answer.trim().is_empty() {
        Err("Xiaoli ACP returned no commit message.".into())
    } else {
        Ok(answer)
    }
}
