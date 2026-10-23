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
        runtime.capabilities.register(
            CapabilityDescriptor {
                id: CapabilityId::parse("alloy.awake").map_err(|e| e.to_string())?,
                version: semver::Version::new(1, 0, 0),
                methods: BTreeMap::from([
                    ("status".into(), CapabilityMethod { input: ValueSchema::Null, output: ValueSchema::UiValue }),
                    ("set_enabled".into(), CapabilityMethod { input: ValueSchema::UiValue, output: ValueSchema::UiValue }),
                ]),
            },
            AwakeCapabilities(alloy::awake::Monitor::new(self.fixture.is_none())),
        ).map_err(|e| e.to_string())?;
        runtime.capabilities.register(
            CapabilityDescriptor {
                id: CapabilityId::parse("alloy.update").map_err(|e| e.to_string())?,
                version: semver::Version::new(1, 0, 0),
                methods: BTreeMap::from([
                    ("status".into(), CapabilityMethod {input:ValueSchema::Null, output:ValueSchema::UiValue}),
                    ("check".into(), CapabilityMethod {input:ValueSchema::Null, output:ValueSchema::UiValue}),
                    ("guard".into(), CapabilityMethod {input:ValueSchema::UiValue, output:ValueSchema::UiValue}),
                ]),
            },
            UpdateCapabilities {disabled:self.fixture.is_some()},
        ).map_err(|e| e.to_string())?;
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
            // File browsing must remain responsive while ACP holds the mutation gate.
            if string(&data, "op") == "select" {
                return Ok(to_ui(execute(&data, &initial).unwrap_or_else(|error| {
                    json!({"ok":false,"message":error,"snapshot":null,"document":null,"selection_id":data["selection_id"]})
                })));
            }
            // Read-only summaries must not queue behind ACP or network operations.
            if matches!(
                string(&data, "op"),
                "projects" | "project_summaries" | "open_file" | "open_folder"
            ) {
                return execute(&data, &initial).map(to_ui);
            }
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
                    let merge_message = repo.as_ref().zip(snapshot.as_ref()).filter(|(_, s)| s.merging).and_then(|(r, _)| r.merge_message().ok());
                    json!({ "ok": false, "message": error, "snapshot": snapshot, "document": document, "merge_message": merge_message })
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
fn model_settings(data: &Value, path: &std::path::Path) -> Result<Value, String> {
    let mut settings = alloy::projects::Projects::read(path)?;
    let name = string(data, "name");
    match string(data, "op") {
        "register_model" => {
            let model = serde_json::from_value(data["model"].clone()).map_err(|_| "模型配置格式无效。")?;
            settings.register_model(model)?;
        }
        "select_model" => {
            if !settings.direct_models.iter().any(|m| m.name == name) {return Err("模型已不存在，请刷新后重试。".into());}
            settings.selected_model = name.into();
        }
        "remove_model" => {
            settings.direct_models.retain(|m| m.name != name);
            if settings.selected_model == name {settings.selected_model = settings.direct_models.first().map(|m| m.name.clone()).unwrap_or_default();}
        }
        _ => return Err("Unknown model settings operation".into()),
    }
    settings.save(path)?;
    Ok(settings.ai_settings())
}

fn execute(data: &Value, initial: &str) -> Result<Value, String> {
    let op = string(data, "op");
    let path = string(data, "path");
    if op == "projects" || op == "project_summaries" {
        let projects = alloy::projects::Projects::read(&alloy::projects::store_path()?)?;
        if op == "project_summaries" { return Ok(json!({"projects":projects.overview()})); }
        let mut result = projects.ai_settings();
        result["projects"] = json!(projects.overview());
        return Ok(result);
    }
    if matches!(op, "register_model" | "select_model" | "remove_model") {
        return model_settings(data, &alloy::projects::store_path()?);
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
        if data["activate_acp"] != json!(false) {settings.selected_model.clear();}
        if !selected_path.is_empty() {
            settings.xiaoli_path = selected_path;
        }
        settings.save(&settings_path)?;
        return Ok(settings.ai_settings());
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
    if matches!(op, "open_file" | "open_folder") {
        let target = repo.external_open_path(path, op == "open_folder")?;
        let output = std::process::Command::new("/usr/bin/open")
            .arg(&target)
            .output()
            .map_err(|e| format!("Cannot open {}: {e}", target.display()))?;
        if !output.status.success() {
            return Err(format!(
                "Cannot open {}: {}",
                target.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        return Ok(json!({"ok":true,"message":format!("Opened {}", target.display())}));
    }
    if op == "push_prepare" {
        repo.head_sha()?;
        return Ok(json!({"ok":true,"needs_upstream":!repo.has_upstream()?}));
    }
    if matches!(op, "history" | "history_commit" | "history_diff") {
        let mut result = json!({"ok":true,"history_op":op,"history_root":repo.root.to_string_lossy(),"history_id":data["commit_id"],"history_skip":data["skip"]});
        match op {
            "history" => result["history"] = serde_json::to_value(alloy::history::list(&repo, data["skip"].as_u64().unwrap_or(0) as usize)?).map_err(|e| e.to_string())?,
            "history_commit" => result["details"] = serde_json::to_value(alloy::history::details(&repo, string(data, "commit_id"))?).map_err(|e| e.to_string())?,
            _ => result["document"] = serde_json::to_value(alloy::history::diff(&repo, string(data, "commit_id"), path)?).map_err(|e| e.to_string())?,
        }
        return Ok(result);
    }
    if op == "select" {
        let document =
            repo.selected_comparison(path, string(data, "old_path"), flag(data, "staged"), true)?;
        return Ok(
            json!({"ok":true,"message":path,"snapshot":null,"document":document,"selection_id":data["selection_id"]}),
        );
    }
    let mut message = "Repository refreshed".to_string();
    let mut commit_message = None;
    let mut committed = false;
    let mut operation_ok = true;
    match op {
        "bootstrap" | "open" | "clone" | "browse" => repo.stage_committed_changes()?,
        "refresh" => {
            if flag(data, "default_selection") {
                repo.stage_committed_changes()?;
            }
        }
        "delete_file" => {
            repo.delete_file(path)?;
            message = "File deleted from working tree".into();
        }
        "rollback" => {
            repo.rollback(Some(path))?;
            message = "File rolled back to HEAD".into();
        }
        "rollback_all" => {
            repo.rollback(None)?;
            message = "Committed files rolled back; new files preserved".into();
        }
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
        "abort_merge" => {
            repo.abort_merge()?;
            message = "Merge aborted".into();
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
            repo.commit_parts(string(data, "message"), &paths, &partial)?;
            message = "Committed staged changes.".into();
        }
        "commit" => {
            repo.commit(string(data, "message"))?;
            message = "Committed staged changes.".into();
        }
        "ai_commit" => {
            let confirmed_tree = string(data, "confirmed_tree");
            if let Some((files, tree)) = alloy::cmsg::sensitive_review(&repo)? {
                if confirmed_tree != tree {
                    return Ok(
                        json!({"ok":false,"message":"AI commit blocked: sensitive filenames require your confirmation.","snapshot":repo.snapshot()?,"document":null,
                        "sensitive_review":{"files":files,"tree":tree,"root":repo.root.to_string_lossy()}}),
                    );
                }
            }
            let generated = alloy::cmsg::generate_and_commit_confirmed(
                &repo,
                if confirmed_tree.is_empty() {
                    None
                } else {
                    Some(confirmed_tree)
                },
            )?;
            commit_message = Some(generated.message);
            match generated.commit_result {
                Ok(_) => {
                    committed = true;
                    message = "Committed staged changes.".into();
                }
                Err(error) => {
                    operation_ok = false;
                    message = error;
                }
            }
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
            if op == "push" && !flag(data, "allow_upstream") && !repo.has_upstream()? {
                return Err("The upstream configuration changed. Click Push again to confirm its setup.".into());
            }
            message = repo.network(op)?;
            if message.trim().is_empty() {
                message = format!("{op} completed");
            }
        }
        _ => return Err("Unknown operation".into()),
    }
    let snapshot = repo.snapshot()?;
    let next_conflict = if matches!(op, "resolve" | "delete") {
        snapshot.files.iter().find(|f| f.conflict)
    } else { None };
    let selected = next_conflict.or_else(|| snapshot
        .files
        .iter()
        .find(|f| f.path == path))
        .or_else(|| snapshot.files.iter().find(|f| f.conflict))
        .or_else(|| snapshot.files.first());
    let document = match selected {
        Some(file) => Some(repo.comparison_for_entry(file, flag(data, "staged"), true)?),
        None => None,
    };
    let committed_paths = repo.committed_paths()?;
    let mut result = json!({"committed_paths":committed_paths,"ok":operation_ok,"message":message,"snapshot":snapshot,"document":document,"overview":alloy::projects::Overview::from_snapshot(&snapshot)});
    if snapshot.merging { result["merge_message"] = json!(repo.merge_message().unwrap_or_default()); }
    if op == "ai_commit" {
        result["committed"] = json!(committed);
        result["commit_message"] = json!(commit_message);
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
pub(crate) fn to_ui(value: Value) -> UiValue {
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
struct AwakeCapabilities(alloy::awake::Monitor);
impl CapabilityHandler for AwakeCapabilities {
    fn call(&mut self, method: &str, input: UiValue) -> Result<UiValue, String> {
        let status = match method {
            "status" => self.0.status(),
            "set_enabled" => match input {
                UiValue::Bool(enabled) => self.0.set_enabled(enabled),
                _ => return Err("Expected keep-awake checkbox value".into()),
            },
            _ => return Err("Unknown keep-awake method".into()),
        };
        Ok(to_ui(serde_json::to_value(status).map_err(|e| e.to_string())?))
    }
}
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

#[cfg(test)]
mod merge_tests {
    use super::*;
    #[test]
    fn resolving_a_file_advances_to_the_next_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| alloy::git::run(dir.path(), args, None).unwrap();
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "Merge Test"]);
        git(&["config", "user.email", "merge@example.invalid"]);
        git(&["config", "commit.gpgsign", "false"]);
        git(&["config", "core.hooksPath", "/dev/null"]);
        for path in ["a.txt", "b.txt"] {
            std::fs::write(dir.path().join(path), "base\n").unwrap();
        }
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        git(&["checkout", "-b", "incoming"]);
        for path in ["a.txt", "b.txt"] {
            std::fs::write(dir.path().join(path), "remote\n").unwrap();
        }
        git(&["commit", "-am", "incoming"]);
        git(&["checkout", "main"]);
        for path in ["a.txt", "b.txt"] {
            std::fs::write(dir.path().join(path), "local\n").unwrap();
        }
        git(&["commit", "-am", "local"]);
        assert!(alloy::git::run(dir.path(), &["merge", "incoming"], None).is_err());
        let repo = Repository::open(dir.path().to_str().unwrap()).unwrap();
        let doc = repo.document("a.txt", false).unwrap();
        let response = execute(&json!({"op":"resolve","root":repo.root,"path":doc.path,"expected":doc.right,"exists":true,"content":doc.ours}), "").unwrap();
        assert_eq!(response["document"]["path"], "b.txt");
        assert_eq!(response["document"]["conflict"], true);
        assert_eq!(response["snapshot"]["merging"], true);
        assert!(
            response["merge_message"]
                .as_str()
                .unwrap()
                .starts_with("Merge")
        );
        assert!(repo.commit("too early").is_err());
    }
}

struct UpdateCapabilities { disabled: bool }
impl CapabilityHandler for UpdateCapabilities {
    fn call(&mut self, method: &str, input: UiValue) -> Result<UiValue, String> {
        if self.disabled {return Ok(to_ui(json!({"available":false,"can_check":false})));}
        match method {
            "status" => Ok(to_ui(json!({"available":crate::updater::available(),"can_check":crate::updater::can_check()}))),
            "guard" => {crate::updater::set_blocked(flag(&from_ui(input), "blocked")); Ok(UiValue::Null)},
            "check" => {crate::updater::check()?; Ok(UiValue::Null)},
            _ => Err("Unknown update action".into()),
        }
    }
}

#[cfg(test)]
mod model_settings_tests {
    use super::*;
    #[test]
    fn register_select_update_remove_and_reload_models() {
        let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("preferences.json");
        let mut initial = alloy::projects::Projects::default();initial.remember("/existing/repo");initial.save(&path).unwrap();
        let register = |name: &str, key: &str| json!({"op":"register_model","model":{"name":name,"base_url":"http://localhost:8000/v1","model":"test-model","api_key":key}});
        let first = model_settings(&register("first", "test-secret"), &path).unwrap();
        assert_eq!(first["selected_model"],"first");assert!(!first.to_string().contains("test-secret"));
        model_settings(&register("second", ""),&path).unwrap();
        assert_eq!(model_settings(&json!({"op":"select_model","name":"first"}),&path).unwrap()["selected_model"],"first");
        model_settings(&register("first", ""),&path).unwrap();
        let saved = alloy::projects::Projects::read(&path).unwrap();
        assert_eq!(saved.direct_models[0].api_key.as_deref(),Some("test-secret"));
        assert_eq!(saved.roots,vec!["/existing/repo"]);assert_eq!(saved.acp_agents[0].name,"Xiaoli");
        assert!(model_settings(&json!({"op":"select_model","name":"missing"}),&path).is_err());
        assert_eq!(model_settings(&json!({"op":"remove_model","name":"first"}),&path).unwrap()["selected_model"],"second");
        assert_eq!(model_settings(&json!({"op":"remove_model","name":"second"}),&path).unwrap()["selected_model"],"");
        assert!(alloy::projects::Projects::read(&path).unwrap().direct_models.is_empty());
    }
}
