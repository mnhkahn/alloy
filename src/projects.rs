//! Persistent recent repositories and lightweight, local-only overview data.
use crate::git::{Repository, Snapshot};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Default, Serialize, Deserialize)]
pub struct Projects {
    pub roots: Vec<String>,
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
    std::env::var_os("HOME")
        .map(|p| PathBuf::from(p).join("Library/Application Support/Alloy/projects.json"))
        .ok_or("Home directory unavailable".into())
}
impl Projects {
    pub fn read(path: &Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
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
