use alloy::git::{self, Repository};
use gpui_rhai::{
    AsyncCapabilityHandler, CapabilityDescriptor, CapabilityHandler, CapabilityId,
    CapabilityMethod, ScriptViewExtension, TaskWork, UiRuntimeState, UiValue, ValueSchema,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub struct GitExtension {
    pub initial: String,
    pub fixture: Option<Value>,
    pub gate: Arc<Mutex<()>>,
}
struct Handler {
    initial: String,
    gate: Arc<Mutex<()>>,
}
impl ScriptViewExtension for GitExtension {
    fn configure_runtime(&self, runtime: &mut UiRuntimeState) -> Result<(), String> {
        runtime
            .capabilities
            .register(
                CapabilityDescriptor {
                    id: CapabilityId::parse("alloy.boot").map_err(|e| e.to_string())?,
                    version: semver::Version::new(1, 0, 0),
                    methods: BTreeMap::from([(
                        "state".into(),
                        CapabilityMethod {
                            input: ValueSchema::Null,
                            output: ValueSchema::UiValue,
                        },
                    )]),
                },
                Boot(self.fixture.clone().unwrap_or(Value::Null)),
            )
            .map_err(|e| e.to_string())?;

        runtime
            .capabilities
            .register_async(
                CapabilityDescriptor {
                    id: CapabilityId::parse("alloy.git").map_err(|e| e.to_string())?,
                    version: semver::Version::new(1, 0, 0),
                    methods: BTreeMap::from([(
                        "execute".into(),
                        CapabilityMethod {
                            input: ValueSchema::UiValue,
                            output: ValueSchema::UiValue,
                        },
                    )]),
                },
                Handler {
                    initial: self.initial.clone(),
                    gate: self.gate.clone(),
                },
            )
            .map_err(|e| e.to_string())
    }
}
impl AsyncCapabilityHandler for Handler {
    fn start(&mut self, _: &str, input: UiValue) -> Result<TaskWork, String> {
        let data = from_ui(input);
        let initial = self.initial.clone();
        let gate = self.gate.clone();
        Ok(TaskWork::new(move || {
            let _guard = gate.lock().map_err(|e| e.to_string())?;
            let output = match execute(&data, &initial) {
                Ok(value) => value,
                Err(error) => {
                    let repo = Repository::open(string(&data, "root")).ok();
                    let snapshot = repo.as_ref().and_then(|r| r.snapshot().ok());
                    let document = repo.as_ref().zip(snapshot.as_ref()).and_then(|(r, s)| {
                        s.files
                            .iter()
                            .find(|f| f.conflict)
                            .or_else(|| s.files.iter().find(|f| f.path == string(&data, "path")))
                            .or_else(|| s.files.first())
                            .and_then(|f| r.document(&f.path, false).ok())
                    });
                    json!({ "ok": false, "message": error, "snapshot": snapshot, "document": document })
                }
            };
            Ok(to_ui(output))
        }))
    }
}
fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn flag(v: &Value, key: &str) -> bool {
    v[key].as_bool().unwrap_or(false)
}
fn execute(data: &Value, initial: &str) -> Result<Value, String> {
    let op = string(data, "op");
    let path = string(data, "path");
    if op == "bootstrap" && initial.is_empty() {
        return Ok(
            json!({"ok":true,"message":"Open a local repository or clone one to begin.","snapshot":null,"document":null}),
        );
    }
    if op == "hunk" || op == "choose" {
        let draft = if op == "hunk" {
            git::restore_hunk(
                string(data, "left"),
                string(data, "content"),
                data["number"].as_u64().unwrap_or(0) as usize,
            )?
        } else {
            git::choose_conflict(
                string(data, "content"),
                data["number"].as_u64().unwrap_or(0) as usize,
                string(data, "choice"),
            )?
        };
        return Ok(
            json!({"ok":true,"message":"Draft updated. Review and save when ready.","draft":draft,"snapshot":null,"document":null}),
        );
    }
    let repo = match op {
        "bootstrap" => Repository::open(initial)?,
        "open" => Repository::open(string(data, "root"))?,
        "clone" => git::clone_repository(string(data, "url"), string(data, "root"))?,
        "browse" => {
            let out = std::process::Command::new("osascript")
                .args([
                    "-e",
                    "POSIX path of (choose folder with prompt \"Open a Git repository in Alloy\")",
                ])
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err("Folder selection cancelled.".into());
            }
            Repository::open(String::from_utf8_lossy(&out.stdout).trim())?
        }
        _ => Repository::open(string(data, "root"))?,
    };
    let mut message = "Repository refreshed".to_string();
    match op {
        "bootstrap" | "open" | "clone" | "browse" | "refresh" | "select" => (),
        "save" => {
            repo.save(
                path,
                string(data, "expected"),
                flag(data, "exists"),
                string(data, "content"),
            )?;
            message = "Saved to working tree".into();
        }
        "stage" => {
            repo.stage(path)?;
            message = "File staged".into();
        }
        "unstage" => {
            repo.unstage(path)?;
            message = "File unstaged".into();
        }
        "resolve" => {
            repo.resolve(
                path,
                string(data, "expected"),
                flag(data, "exists"),
                string(data, "content"),
            )?;
            message = "Resolution saved and staged".into();
        }
        "delete" => {
            repo.resolve_delete(path, string(data, "expected"), flag(data, "exists"))?;
            message = "Deletion staged as conflict resolution".into();
        }
        "commit" => {
            message = repo.commit(string(data, "message"))?;
        }
        "fetch" | "pull" | "push" => {
            message = repo.network(op)?;
            if message.trim().is_empty() {
                message = format!("{op} completed");
            }
        }
        _ => return Err("Unknown operation".into()),
    }
    let snapshot = repo.snapshot()?;
    let selected = snapshot
        .files
        .iter()
        .find(|f| f.path == path)
        .or_else(|| snapshot.files.first());
    let document = match selected {
        Some(file) => Some(repo.document(&file.path, flag(data, "staged") && !file.conflict)?),
        None => None,
    };
    Ok(json!({"ok":true,"message":message,"snapshot":snapshot,"document":document}))
}
fn to_ui(value: Value) -> UiValue {
    match value {
        Value::Null => UiValue::Null,
        Value::Bool(v) => UiValue::Bool(v),
        Value::Number(v) => UiValue::Integer(v.as_i64().unwrap_or(0)),
        Value::String(v) => UiValue::String(v),
        Value::Array(v) => UiValue::Array(v.into_iter().map(to_ui).collect()),
        Value::Object(v) => UiValue::Map(v.into_iter().map(|(k, v)| (k, to_ui(v))).collect()),
    }
}
fn from_ui(value: UiValue) -> Value {
    match value {
        UiValue::Null => Value::Null,
        UiValue::Bool(v) => json!(v),
        UiValue::Integer(v) => json!(v),
        UiValue::Float(v) => json!(v),
        UiValue::String(v) => json!(v),
        UiValue::Array(v) => Value::Array(v.into_iter().map(from_ui).collect()),
        UiValue::Map(v) => Value::Object(v.into_iter().map(|(k, v)| (k, from_ui(v))).collect()),
        _ => Value::Null,
    }
}

struct Boot(Value);
impl CapabilityHandler for Boot {
    fn call(&mut self, _: &str, _: UiValue) -> Result<UiValue, String> {
        Ok(to_ui(self.0.clone()))
    }
}
