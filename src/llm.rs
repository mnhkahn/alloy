//! OpenAI-compatible text generation. Configuration comes from the existing
//! xiaoli model settings (~/.xiaoli/settings.json + secrets.json); Alloy adds
//! no model configuration of its own.
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path, path::PathBuf, time::Duration};

const TIMEOUT: Duration = Duration::from_secs(60);
const RETRIES: usize = 2;

#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub max_tokens: u64,
}

#[derive(Deserialize)]
struct Settings {
    models: Option<Models>,
}
#[derive(Deserialize)]
struct Models {
    default: Option<String>,
    options: Option<BTreeMap<String, ModelEntry>>,
}
#[derive(Deserialize)]
struct ModelEntry {
    name: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    api_key_env: Option<String>,
    max_tokens: Option<u64>,
}

fn config_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("Cannot locate the home directory.")?;
    Ok(PathBuf::from(home).join(".xiaoli"))
}

/// Resolve the default model: settings.json `models.default` → `options[id]`.
/// API key priority: entry `api_key` → env var named by `api_key_env` →
/// secrets.json entry of the same name.
pub fn load() -> Result<ModelConfig, String> {
    let dir = config_dir()?;
    let path = dir.join("settings.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| format!("No model configuration found at {}.", path.display()))?;
    let settings: Settings =
        serde_json::from_str(&raw).map_err(|e| format!("Cannot parse {}: {e}", path.display()))?;
    let models = settings
        .models
        .ok_or("settings.json has no \"models\" section.")?;
    let default = models
        .default
        .filter(|d| !d.is_empty())
        .ok_or("settings.json has no models.default entry.")?;
    let entry = models
        .options
        .unwrap_or_default()
        .remove(&default)
        .ok_or(format!(
            "Model \"{default}\" is not defined under models.options."
        ))?;
    let non_empty = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
    let base_url =
        non_empty(entry.base_url).ok_or(format!("Model \"{default}\" has no base_url."))?;
    let model = non_empty(entry.model).ok_or(format!("Model \"{default}\" has no model name."))?;
    let env_name = entry.api_key_env.clone().unwrap_or_default();
    let api_key = non_empty(entry.api_key)
        .or_else(|| std::env::var(&env_name).ok().filter(|k| !k.is_empty()))
        .or_else(|| secret(&dir, &env_name));
    Ok(ModelConfig {
        name: non_empty(entry.name).unwrap_or(default),
        base_url,
        model,
        api_key,
        max_tokens: entry.max_tokens.filter(|t| *t > 0).unwrap_or(4096),
    })
}

fn secret(dir: &Path, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    let raw = std::fs::read_to_string(dir.join("secrets.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn endpoint(base_url: &str) -> String {
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
