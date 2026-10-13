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
    watcher: Arc<Mutex<Option<alloy::watch::RepositoryWatch>>>,
    initial: String,
    gate: Arc<Mutex<()>>,
}
impl ScriptViewExtension for GitExtension {
    fn configure_runtime(&self, runtime: &mut UiRuntimeState) -> Result<(), String> {
        runtime
            .assets
            .register("preview", PreviewAssets)
            .map_err(|e| e.to_string())?;
        runtime
            .capabilities
            .register(
                CapabilityDescriptor {
                    id: CapabilityId::parse("alloy.boot").map_err(|e| e.to_string())?,
                    version: semver::Version::new(1, 0, 0),
                    methods: BTreeMap::from([
                        (
                            "shortcuts".into(),
                            CapabilityMethod {
                                input: ValueSchema::Null,
                                output: ValueSchema::UiValue,
                            },
                        ),
                        (
                            "state".into(),
                            CapabilityMethod {
                                input: ValueSchema::Null,
                                output: ValueSchema::UiValue,
                            },
                        ),
                    ]),
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
                    watcher: Arc::new(Mutex::new(None)),
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
        let watcher = self.watcher.clone();
        let initial = self.initial.clone();
        let gate = self.gate.clone();
        Ok(TaskWork::new(move || {
            let _guard = gate.lock().map_err(|e| e.to_string())?;
            if string(&data, "op") == "watch" {
                let mut current = watcher.lock().map_err(|e| e.to_string())?;
                let root = std::path::Path::new(string(&data, "root"));
                if current.as_ref().is_none_or(|w| w.root != root) {
                    *current = None;
                    *current = Some(alloy::watch::RepositoryWatch::new(&Repository::open(
                        string(&data, "root"),
                    )?)?);
                }
                let revision = current
                    .as_ref()
                    .unwrap()
                    .revision(data["seen"].as_u64().unwrap_or(0))?;
                let mut output = json!({"watch_root":data["root"],"watch_epoch":data["epoch"],"revision":revision,"update":null});
                if revision.is_some() {
                    let mut refresh = data.clone();
                    refresh["op"] = json!("refresh");
                    output["update"] = execute(&refresh, &initial)?;
                }
                return Ok(to_ui(output));
            }
            let output = match execute(&data, &initial) {
                Ok(value) => value,
                Err(error) if string(&data, "op") == "select" => {
                    json!({"ok":false,"message":error,"snapshot":null,"document":null,"selection_id":data["selection_id"]})
                }
                Err(error) => {
                    let repo = Repository::open(string(&data, "root")).ok();
                    let snapshot = repo.as_ref().and_then(|r| r.snapshot().ok());
                    let document = repo.as_ref().zip(snapshot.as_ref()).and_then(|(r, s)| {
                        s.files
                            .iter()
                            .find(|f| f.conflict)
                            .or_else(|| s.files.iter().find(|f| f.path == string(&data, "path")))
                            .or_else(|| s.files.first())
                            .and_then(|f| r.comparison_for_entry(f, false, true).ok())
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
    if op == "projects" {
        let projects = alloy::projects::Projects::read(&alloy::projects::store_path()?)?;
        return Ok(
            json!({"projects":projects.overview(),"acp_agents":projects.acp_agents,"selected_acp":projects.selected_acp}),
        );
    }
    if op == "save_acp_settings" {
        let agents: Vec<alloy::projects::AcpAgent> =
            serde_json::from_value(data["acp_agents"].clone()).map_err(|e| e.to_string())?;
        let selected = string(data, "selected_acp").trim();
        let mut agents = agents;
        if agents.is_empty() {
            return Err("Configure at least one ACP agent.".into());
        }
        for agent in &mut agents {
            agent.name = agent.name.trim().to_owned();
            agent.path = agent.path.trim().to_owned();
            agent.command = agent.command.trim().to_owned();
        }
        for (index, agent) in agents.iter().enumerate() {
            if agent.name.is_empty() {
                return Err("ACP agent names cannot be empty.".into());
            }
            if agents[..index].iter().any(|other| other.name == agent.name) {
                return Err(format!("ACP agent name '{}' is already used.", agent.name));
            }
            alloy::acp::command_args(agent).map_err(|error| format!("{}: {error}", agent.name))?;
        }
        let selected_agent = agents
            .iter()
            .find(|agent| agent.name == selected)
            .ok_or("Select a configured ACP agent.")?;
        let args = alloy::acp::command_args(selected_agent)?;
        alloy::acp::resolve_executable(&args[0])
            .map_err(|error| format!("{}: {error}", selected_agent.name))?;
        let selected_name = selected_agent.name.clone();
        let selected_path = selected_agent.path.clone();
        let settings_path = alloy::projects::store_path()?;
        let mut settings = alloy::projects::Projects::read(&settings_path)?;
        settings.acp_agents = agents;
        settings.selected_acp = selected_name;
        if !selected_path.is_empty() {
            settings.xiaoli_path = selected_path;
        }
        settings.save(&settings_path)?;
        return Ok(
            json!({"ok":true,"message":"ACP settings saved.","acp_agents":settings.acp_agents,"selected_acp":settings.selected_acp,"snapshot":null,"document":null}),
        );
    }
    let remembered = if op == "bootstrap" && initial.is_empty() {
        alloy::projects::Projects::read(&alloy::projects::store_path()?)?
            .roots
            .first()
            .cloned()
            .unwrap_or_default()
    } else {
        initial.to_owned()
    };
    let initial = remembered.as_str();
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
    if op == "select" {
        let document =
            repo.selected_comparison(path, string(data, "old_path"), flag(data, "staged"), true)?;
        return Ok(
            json!({"ok":true,"message":path,"snapshot":null,"document":document,"selection_id":data["selection_id"]}),
        );
    }
    let mut message = "Repository refreshed".to_string();
    match op {
        "bootstrap" | "open" | "clone" | "browse" | "refresh" => (),
        "save" => {
            repo.save(
                path,
                string(data, "expected"),
                flag(data, "exists"),
                string(data, "content"),
            )?;
            message = "Saved to working tree".into();
        }
        "stage_paths" => {
            let paths: Vec<String> =
                serde_json::from_value(data["paths"].clone()).map_err(|e| e.to_string())?;
            repo.stage_paths(&paths, flag(data, "checked"))?;
            message = if flag(data, "checked") {
                "Selected files staged"
            } else {
                "Selected files unstaged"
            }
            .into();
        }
        "stage_hunks" => {
            let selection =
                serde_json::from_value(data["selection"].clone()).map_err(|e| e.to_string())?;
            repo.stage_hunks(path, &selection, string(data, "expected_index"))?;
            message = "Staged blocks updated".into();
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
        "commit_selected" => {
            let paths: Vec<String> =
                serde_json::from_value(data["paths"].clone()).map_err(|e| e.to_string())?;
            let partial = if data["partial"].is_object() {
                serde_json::from_value(data["partial"].clone()).map_err(|e| e.to_string())?
            } else {
                BTreeMap::new()
            };
            message = repo.commit_parts(string(data, "message"), &paths, &partial)?;
        }
        "commit" => {
            message = repo.commit(string(data, "message"))?;
        }
        "ai_commit" => {
            message = alloy::cmsg::generate_and_commit(&repo)?;
        }
        "tag_prepare" => {
            let preview = alloy::tag::prepare(&repo)?;
            return Ok(
                json!({"ok":true,"message":format!("Tag base {}; pick s/m/l", preview.base),"tag_preview":preview,"snapshot":null,"document":null}),
            );
        }
        "tag_create" | "tag_push" => {
            let outcome = if op == "tag_create" {
                alloy::tag::create(
                    &repo,
                    string(data, "version"),
                    string(data, "head"),
                    flag(data, "push"),
                    string(data, "remote"),
                    string(data, "push_url"),
                )
            } else {
                alloy::tag::retry_push(
                    &repo,
                    string(data, "version"),
                    string(data, "commit"),
                    string(data, "remote"),
                    string(data, "push_url"),
                )
            };
            match outcome {
                Ok(done) => message = done,
                Err(alloy::tag::TagError::Plain(error)) => return Err(error),
                Err(alloy::tag::TagError::PushFailed {
                    message: error,
                    version,
                    commit,
                    remote,
                    push_url,
                }) => {
                    return Ok(
                        json!({"ok":false,"message":error,"snapshot":repo.snapshot()?,"document":null,"tag_retry":{"version":version,"commit":commit,"remote":remote,"push_url":push_url}}),
                    );
                }
            }
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
        Some(file) => Some(repo.comparison_for_entry(file, flag(data, "staged"), true)?),
        None => None,
    };
    let mut result = json!({"ok":true,"message":message,"snapshot":snapshot,"document":document,"overview":alloy::projects::Overview::from_snapshot(&snapshot)});
    if op == "ai_commit" {
        result["committed"] = json!(true);
    }
    if matches!(op, "tag_create" | "tag_push") {
        result["tagged"] = json!(true);
    }
    if matches!(op, "bootstrap" | "open" | "browse" | "clone") {
        let saved = (|| -> Result<(), String> {
            let path = alloy::projects::store_path()?;
            let mut projects = alloy::projects::Projects::read(&path)?;
            projects.remember(&snapshot.root);
            projects.save(&path)
        })();
        if let Err(error) = saved {
            result["message"] = json!(format!(
                "Opened repository; could not save project history: {error}"
            ));
        }
    }
    Ok(result)
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
    fn call(&mut self, method: &str, _: UiValue) -> Result<UiValue, String> {
        Ok(to_ui(if method == "shortcuts" {
            crate::shortcuts::catalog()
        } else {
            self.0.clone()
        }))
    }
}

struct PreviewAssets;
impl gpui_rhai::AssetProvider for PreviewAssets {
    fn load(&self, name: &str) -> Result<gpui_rhai::AssetData, String> {
        let (mime_type, bytes) = alloy::preview::load_asset(name)?;
        Ok(gpui_rhai::AssetData { mime_type, bytes })
    }
}
