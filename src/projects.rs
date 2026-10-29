//! Persistent recent repositories and lightweight, local-only overview data.
use crate::git::{Repository, Snapshot};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const DEFAULT_XIAOLI_PATH: &str = "/Users/mnhkahn/go/bin/xiaoli";

fn default_xiaoli_path() -> String {
    DEFAULT_XIAOLI_PATH.to_owned()
}

#[derive(Serialize, Deserialize)]
pub struct Projects {
    #[serde(default)]
    pub roots: Vec<String>,
    /// Kept for migration from early ACP builds that stored one path only.
    #[serde(default = "default_xiaoli_path")]
    pub xiaoli_path: String,
    #[serde(default)]
    pub acp_agents: Vec<AcpAgent>,
    #[serde(default)]
    pub selected_acp: String,
    #[serde(default)]
    pub direct_models: Vec<crate::llm::ModelConfig>,
    #[serde(default)]
    pub selected_model: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcpAgent {
    pub name: String,
    #[serde(
        default,
        deserialize_with = "null_string",
        skip_serializing_if = "String::is_empty"
    )]
    pub path: String,
    #[serde(default, deserialize_with = "null_string")]
    pub command: String,
}
// UI arrays can represent absent optional map fields as explicit nulls.
fn null_string<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}
impl Default for Projects {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            xiaoli_path: default_xiaoli_path(),
            acp_agents: vec![AcpAgent {
                name: "Xiaoli".into(),
                path: default_xiaoli_path(),
                command: String::new(),
            }],
            selected_acp: "Xiaoli".into(),
            direct_models: Vec::new(),
            selected_model: String::new(),
        }
    }
}
#[derive(Serialize)]
pub struct Overview {
    pub root: String,
    pub name: String,
    pub branch: String,
    pub upstream: String,
    pub files: usize,
    pub staged: usize,
    pub conflicts: usize,
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
    pub ahead: usize,
    pub behind: usize,
    pub error: String,
}
impl Overview {
    pub fn from_snapshot(s: &Snapshot) -> Self {
        Self {
            root: s.root.clone(),
            name: Path::new(&s.root)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            branch: s.branch.clone(),
            upstream: s.upstream.clone(),
            files: s.files.len(),
            staged: s
                .files
                .iter()
                .filter(|f| f.index != " " && f.index != "?" && !f.conflict)
                .count(),
            conflicts: s.files.iter().filter(|f| f.conflict).count(),
            added: s
                .files
                .iter()
                .filter(|f| (f.index == "?" || f.index == "A") && f.work != "D")
                .count(),
            deleted: s
                .files
                .iter()
                .filter(|f| f.index == "D" || f.work == "D")
                .count(),
            modified: s
                .files
                .iter()
                .filter(|f| f.index != "?" && f.index != "A" && f.index != "D" && f.work != "D")
                .count(),
            ahead: s.ahead,
            behind: s.behind,
            error: String::new(),
        }
    }
}
pub fn store_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("Home directory unavailable")?;
    let path = home.join(".alloy/projects.json");
    match fs::metadata(&path) {
        Ok(_) => return Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let legacy = home.join("Library/Application Support/Alloy/projects.json");
    let bytes = match fs::read(&legacy) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(path),
        Err(error) => return Err(format!("Could not read previous Alloy settings: {error}")),
    };
    // Keep all fields during migration and retain the original as a backup.
    serde_json::from_slice::<Projects>(&bytes)
        .map_err(|error| format!("Could not migrate previous Alloy settings: {error}"))?;
    let parent = path.parent().ok_or("Missing settings directory")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    match file.persist_noclobber(&path) {
        Ok(_) => {}
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("Could not migrate Alloy settings: {error}")),
    }
    Ok(path)
}
impl Projects {
    pub fn read(path: &Path) -> Result<Self, String> {
        let mut settings = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<Self>(&bytes).map_err(|e| e.to_string())?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.to_string()),
        };
        if settings.acp_agents.is_empty() {
            settings.acp_agents.push(AcpAgent {
                name: "Xiaoli".into(),
                path: settings.xiaoli_path.clone(),
                command: String::new(),
            });
        }
        if !settings
            .acp_agents
            .iter()
            .any(|agent| agent.name == settings.selected_acp)
        {
            settings.selected_acp = settings.acp_agents[0].name.clone();
        }
        settings.xiaoli_path = settings
            .acp_agents
            .iter()
            .find(|agent| agent.name == settings.selected_acp)
            .map(|agent| agent.path.clone())
            .filter(|path| !path.is_empty())
            .unwrap_or_else(default_xiaoli_path);
        if !settings.selected_model.is_empty()
            && !settings
                .direct_models
                .iter()
                .any(|m| m.name == settings.selected_model)
        {
            settings.selected_model.clear();
        }
        Ok(settings)
    }
    pub fn ai_settings(&self) -> serde_json::Value {
        let models: Vec<_> = self
            .direct_models
            .iter()
            .map(|m| {
                serde_json::json!({
                    "protocol":m.protocol,"name":m.name,"base_url":m.base_url,"model":m.model,"max_tokens":m.max_tokens,
                    "has_api_key":m.api_key.as_ref().is_some_and(|k| !k.is_empty())
                })
            })
            .collect();
        serde_json::json!({"ok":true,"acp_agents":self.acp_agents,"selected_acp":self.selected_acp,
            "direct_models":models,"selected_model":self.selected_model,"snapshot":null,"document":null})
    }
    /// Create or update a profile. A blank key preserves an existing credential.
    pub fn register_model(&mut self, mut model: crate::llm::ModelConfig) -> Result<(), String> {
        model.validate()?;
        if let Some(existing) = self.direct_models.iter_mut().find(|m| m.name == model.name) {
            if model.api_key.is_none() {
                model.api_key = existing.api_key.clone();
            }
            *existing = model.clone();
        } else {
            self.direct_models.push(model.clone());
        }
        self.selected_model = model.name;
        Ok(())
    }
    pub fn remember(&mut self, root: &str) {
        self.roots.retain(|r| r != root);
        self.roots.insert(0, root.into());
        self.roots.truncate(30);
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("Missing settings directory")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(&mut file, self).map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn overview(&self) -> Vec<Overview> {
        self.roots
            .iter()
            .map(
                |root| match Repository::open(root).and_then(|r| r.snapshot()) {
                    Ok(s) => Overview::from_snapshot(&s),
                    Err(error) => {
                        let mut s = Overview::from_snapshot(&Snapshot {
                            root: root.clone(),
                            ..Default::default()
                        });
                        s.error = error;
                        s
                    }
                },
            )
            .collect()
    }
}
/// Build large file-list data in Rust: repeated Rhai array pushes recursively
/// validate the growing array and turn a large repository into quadratic work.
/// This is a pure projection; it never touches repositories or the filesystem.
pub fn file_rows(input: &serde_json::Value) -> serde_json::Value {
    use serde_json::json;
    let mut rows = Vec::new();
    let filter = input["filter"].as_str().unwrap_or_default().to_lowercase();
    let Some(files) = input["files"].as_array() else {
        return json!(rows);
    };
    for group in ["Merge Conflicts", "Changes", "Unversioned Files"] {
        let files: Vec<_> = files
            .iter()
            .filter(|file| {
                let kind = if file["conflict"] == true {
                    "Merge Conflicts"
                } else if file["index"] == "?" {
                    "Unversioned Files"
                } else {
                    "Changes"
                };
                kind == group
                    && file["path"].as_str().is_some_and(|path| {
                        filter.is_empty() || path.to_lowercase().contains(&filter)
                    })
            })
            .collect();
        if files.is_empty() {
            continue;
        }
        if let Some(requested) = input["group"].as_str() {
            if requested == group {
                return json!(
                    files
                        .iter()
                        .filter_map(|file| file["path"].as_str())
                        .collect::<Vec<_>>()
                );
            }
            continue;
        }
        let paths: Vec<_> = files
            .iter()
            .filter_map(|file| file["path"].as_str())
            .collect();
        let count = paths
            .iter()
            .filter(|path| input["checked"][**path] == true)
            .count();
        let mixed = paths
            .iter()
            .any(|path| input["partial"].get(*path).is_some());
        let folded = input["collapsed"][group] == true;
        rows.push(
            json!({"key":format!("group-{group}"),"kind":"group","group":group,
            "count":count,"total":files.len(),"mixed":mixed,"folded":folded,"paths":null}),
        );
        if folded {
            continue;
        }
        for file in files {
            let path = file["path"].as_str().unwrap();
            rows.push(json!({"key":format!("file-{path}"),"kind":"file","file":file,
                "included":input["checked"][path] == true,"mixed":input["partial"].get(path).is_some()}));
        }
    }
    json!(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_file_rows_preserve_filtering_groups_and_staging() {
        use serde_json::json;
        let mut input = json!({"files":[
            {"path":"src/New.rs","index":"?","work":"?","conflict":false},
            {"path":"src/Modified.rs","index":"M","work":"M","conflict":false},
            {"path":"src/Conflict.rs","index":"U","work":"U","conflict":true},
            {"path":"README.md","index":"M","work":" ","conflict":false}
        ],"checked":{"src/Modified.rs":true},"partial":{"src/Modified.rs":true},
        "collapsed":{},"filter":"SRC/"});
        let rows = file_rows(&input);
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0]["group"], "Merge Conflicts");
        assert_eq!(rows[2]["group"], "Changes");
        assert_eq!(rows[2]["count"], 1);
        assert_eq!(rows[2]["mixed"], true);
        assert_eq!(rows[2]["paths"], serde_json::Value::Null);
        input["group"] = json!("Changes");
        assert_eq!(file_rows(&input), json!(["src/Modified.rs"]));
        input.as_object_mut().unwrap().remove("group");
        assert_eq!(rows[3]["included"], true);
        assert_eq!(rows[3]["mixed"], true);
        assert_eq!(rows[4]["group"], "Unversioned Files");
        input["collapsed"] = json!({"Changes":true});
        let rows = file_rows(&input);
        assert_eq!(rows.as_array().unwrap().len(), 5);
        assert_eq!(rows[2]["folded"], true);
        input["filter"] = json!("absent");
        assert_eq!(file_rows(&input), json!([]));
    }
    #[test]
    fn registered_models_persist_without_exposing_keys_to_ui() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        let mut settings = Projects::default();
        let legacy: Projects =
            serde_json::from_value(serde_json::json!({"roots":[],"selected_acp":"Xiaoli"}))
                .unwrap();
        assert!(legacy.direct_models.is_empty());
        assert!(legacy.selected_model.is_empty());
        let model = crate::llm::ModelConfig {
            protocol: crate::llm::Protocol::default(),
            name: " Direct ".into(),
            base_url: " https://example.test/v1/ ".into(),
            model: " test-model ".into(),
            api_key: Some("test-key".into()),
            max_tokens: 4096,
        };
        settings.register_model(model.clone()).unwrap();
        settings.save(&path).unwrap();
        let mut restored = Projects::read(&path).unwrap();
        assert_eq!(restored.selected_model, "Direct");
        assert_eq!(
            restored.direct_models[0].api_key.as_deref(),
            Some("test-key")
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let ui = restored.ai_settings();
        assert!(!ui.to_string().contains("test-key"));
        assert_eq!(ui["direct_models"][0]["has_api_key"], true);
        let mut updated = model;
        updated.api_key = None;
        updated.model = "updated".into();
        restored.register_model(updated).unwrap();
        assert_eq!(restored.direct_models.len(), 1);
        assert_eq!(
            restored.direct_models[0].api_key.as_deref(),
            Some("test-key")
        );
        assert_eq!(restored.direct_models[0].model, "updated");
        restored.selected_model.clear();
        restored.save(&path).unwrap();
        assert!(Projects::read(&path).unwrap().selected_model.is_empty());
    }
    #[test]
    fn summaries_read_changes_in_unopened_repositories_without_staging() {
        let dir = tempfile::tempdir().unwrap();
        let active = dir.path().join("active");
        let unopened = dir.path().join("unopened");
        fs::create_dir_all(&active).unwrap();
        fs::create_dir_all(&unopened).unwrap();
        for root in [&active, &unopened] {
            crate::git::run(root, &["init", "-b", "main"], None).unwrap();
        }
        let projects = Projects {
            roots: vec![
                active.to_string_lossy().into(),
                unopened.to_string_lossy().into(),
            ],
            ..Default::default()
        };
        assert_eq!(projects.overview()[1].files, 0);
        fs::write(unopened.join("new.txt"), "external edit").unwrap();
        let summaries = projects.overview();
        assert_eq!(summaries[0].files, 0);
        assert_eq!(summaries[1].files, 1);
        assert_eq!(summaries[1].added, 1);
        assert_eq!(summaries[1].staged, 0);
        fs::remove_file(unopened.join("new.txt")).unwrap();
        assert_eq!(projects.overview()[1].files, 0);
    }
    #[test]
    fn persistence_deduplicates_and_keeps_missing_repositories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings/projects.json");
        let mut p = Projects::read(&path).unwrap();
        p.remember("/missing/one");
        p.remember("/missing/two");
        p.remember("/missing/one");
        p.save(&path).unwrap();
        let p = Projects::read(&path).unwrap();
        assert_eq!(p.roots, vec!["/missing/one", "/missing/two"]);
        assert!(!p.overview()[0].error.is_empty());
    }
}
