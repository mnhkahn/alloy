//! OpenAI-compatible generation for models registered through Alloy's UI.
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

const TIMEOUT: Duration = Duration::from_secs(60);
const RETRIES: usize = 2;

#[derive(Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u64,
}
fn default_max_tokens() -> u64 {
    4096
}

impl ModelConfig {
    pub fn validate(&mut self) -> Result<(), String> {
        self.name = self.name.trim().to_owned();
        self.base_url = self.base_url.trim().trim_end_matches('/').to_owned();
        self.model = self.model.trim().to_owned();
        self.api_key = self
            .api_key
            .take()
            .map(|k| k.trim().to_owned())
            .filter(|k| !k.is_empty());
        if self.name.is_empty() || self.model.is_empty() {
            return Err("请填写名称和模型名。".into());
        }
        let url = reqwest::Url::parse(&self.base_url).map_err(|_| "请填写有效的 Base URL。")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "Base URL 必须为 HTTP/HTTPS 地址，不能包含用户名、密码、查询参数或片段。".into(),
            );
        }
        if self.max_tokens == 0 {
            return Err("输出 token 上限必须大于零。".into());
        }
        Ok(())
    }
}

/// Dispatch through the provider selected beside Commit; never read Xiaoli config.
pub fn generate_selected(cwd: &Path, system: &str, user: &str) -> Result<String, String> {
    let settings = crate::projects::Projects::read(&crate::projects::store_path()?)?;
    generate_using(&settings, cwd, system, user)
}

fn generate_using(
    settings: &crate::projects::Projects,
    cwd: &Path,
    system: &str,
    user: &str,
) -> Result<String, String> {
    if settings.selected_model.is_empty() {
        crate::acp::generate(cwd, system, user)
    } else {
        let config = settings
            .direct_models
            .iter()
            .find(|m| m.name == settings.selected_model)
            .ok_or("所选直连模型已不存在，请重新选择。")?;
        generate(config, system, user)
    }
}

pub(crate) fn endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

/// Single non-streaming generation: 60s per request, up to 2 retries with
/// 1s/2s backoff for timeouts, rate limits, server errors, network failures
/// and empty responses. Auth and quota errors fail immediately.
pub fn generate(config: &ModelConfig, system: &str, user: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({
        "model": config.model,
        "temperature": 0.2,
        "max_tokens": config.max_tokens,
        "stream": false,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    });
    let mut attempt = 0;
    loop {
        let mut request = client.post(endpoint(&config.base_url)).json(&body);
        if let Some(key) = &config.api_key {
            request = request.bearer_auth(key);
        }
        match send(request) {
            Ok(text) => return Ok(text),
            Err((retryable, error)) => {
                if !retryable || attempt >= RETRIES {
                    return Err(error);
                }
                attempt += 1;
                std::thread::sleep(Duration::from_secs(attempt as u64));
            }
        }
    }
}

fn send(request: reqwest::blocking::RequestBuilder) -> Result<String, (bool, String)> {
    let response = request.send().map_err(|e| {
        let retryable = e.is_timeout() || e.is_connect() || e.status().is_none();
        (retryable, format!("Model request failed: {e}"))
    })?;
    let status = response.status();
    if !status.is_success() {
        let detail = response.text().unwrap_or_default();
        let retryable = status.as_u16() == 429 || status.is_server_error();
        return Err((
            retryable,
            format!("Model returned {status}: {}", detail.trim()),
        ));
    }
    let payload: serde_json::Value = response
        .json()
        .map_err(|e| (false, format!("Cannot parse model response: {e}")))?;
    let text = payload
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err((true, "Model returned an empty response.".into()));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    #[test]
    fn direct_selection_sends_registered_endpoint_model_key_and_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1/", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let (header_end, content_length) = loop {
                let mut buf = [0; 1024];
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let len: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    break (end + 4, len);
                }
            };
            while bytes.len() < header_end + content_length {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
            }
            let response = r#"{"choices":[{"message":{"content":"feat: 恢复直连模型"}}]}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            bytes
        });
        let mut settings = crate::projects::Projects::default();
        settings
            .register_model(ModelConfig {
                name: "local mock".into(),
                base_url: base,
                model: "test-model".into(),
                api_key: Some("test-key".into()),
                max_tokens: 4096,
            })
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            generate_using(&settings, dir.path(), "system prompt", "staged diff").unwrap(),
            "feat: 恢复直连模型"
        );
        let bytes = server.join().unwrap();
        let request = String::from_utf8(bytes).unwrap();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer test-key")
        );
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["messages"][0]["content"], "system prompt");
        assert_eq!(body["messages"][1]["content"], "staged diff");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn endpoints_and_invalid_registration() {
        assert_eq!(
            endpoint("https://example.test/v1/"),
            "https://example.test/v1/chat/completions"
        );
        assert_eq!(
            endpoint("https://example.test/v1/chat/completions"),
            "https://example.test/v1/chat/completions"
        );
        for url in [
            "",
            "ftp://example.test",
            "https://user:password@example.test",
            "https://example.test?api_key=secret",
        ] {
            let mut model = ModelConfig {
                name: "test".into(),
                base_url: url.into(),
                model: "m".into(),
                api_key: None,
                max_tokens: 4096,
            };
            assert!(model.validate().is_err());
        }
    }
}
