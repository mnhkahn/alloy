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
        Ok(settings)
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
#[cfg(test)]
mod tests {
    use super::*;
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
