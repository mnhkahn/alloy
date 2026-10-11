//! Native filesystem events are coalesced before requesting a repository read.
use crate::git::{self, Repository};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct Events {
    revision: u64,
    first: Instant,
    last: Instant,
    error: Option<String>,
}
pub struct RepositoryWatch {
    pub root: PathBuf,
    _watcher: RecommendedWatcher,
    events: Arc<Mutex<Events>>,
}
impl RepositoryWatch {
    pub fn new(repo: &Repository) -> Result<Self, String> {
        let output = git::run(
            &repo.root,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
            ],
            None,
        )?;
        let metadata: Vec<PathBuf> = String::from_utf8_lossy(&output)
            .lines()
            .map(PathBuf::from)
            .collect();
        let events = Arc::new(Mutex::new(Events {
            revision: 1,
            first: Instant::now(),
            last: Instant::now(),
            error: None,
        }));
        let captured = events.clone();
        let filters = metadata.clone();
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            let relevant = match &result {
                Ok(event) => {
                    !matches!(event.kind, EventKind::Access(_))
                        && (event.need_rescan()
                            || event.paths.iter().any(|path| {
                                if let Some(relative) =
                                    filters.iter().find_map(|dir| path.strip_prefix(dir).ok())
                                {
                                    let first = relative
                                        .components()
                                        .next()
                                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                    return matches!(
                                        first.as_str(),
                                        "" | "HEAD"
                                            | "index"
                                            | "config"
                                            | "config.worktree"
                                            | "packed-refs"
                                            | "refs"
                                            | "MERGE_HEAD"
                                            | "MERGE_MSG"
                                            | "rebase-merge"
                                            | "rebase-apply"
                                    );
                                }
                                true
                            }))
                }
                Err(_) => true,
            };
            if relevant && let Ok(mut state) = captured.lock() {
                state.revision += 1;
                state.last = Instant::now();
                if let Err(error) = result {
                    state.error = Some(error.to_string());
                }
            }
        })
        .map_err(|e| e.to_string())?;
        let mut watched = vec![repo.root.clone()];
        watcher
            .watch(&repo.root, RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;
        for dir in metadata {
            if !watched.iter().any(|parent| dir.starts_with(parent)) {
                watcher
                    .watch(&dir, RecursiveMode::Recursive)
                    .map_err(|e| e.to_string())?;
                watched.push(dir);
            }
        }
        Ok(Self {
            root: repo.root.clone(),
            _watcher: watcher,
            events,
        })
    }
    /// The caller acknowledges only results actually applied to the UI.
    pub fn revision(&self, seen: u64) -> Result<Option<u64>, String> {
        let mut state = self.events.lock().map_err(|e| e.to_string())?;
        if let Some(error) = state.error.take() {
            return Err(error);
        }
        if seen == state.revision {
            state.first = Instant::now();
            return Ok(None);
        }
        if state.last.elapsed() < Duration::from_millis(350)
            && state.first.elapsed() < Duration::from_secs(2)
        {
            return Ok(None);
        }
        state.first = Instant::now();
        Ok(Some(state.revision))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observes_file_and_index_changes_without_consuming_unacknowledged_events() {
        let dir = tempfile::tempdir().unwrap();
        git::run(dir.path(), &["init", "-b", "main"], None).unwrap();
        let repo = Repository::open(dir.path().to_str().unwrap()).unwrap();
        let watch = RepositoryWatch::new(&repo).unwrap();
        fn wait(watch: &RepositoryWatch, seen: u64) -> u64 {
            let end = Instant::now() + Duration::from_secs(8);
            loop {
                if let Some(revision) = watch.revision(seen).unwrap() {
                    return revision;
                }
                assert!(
                    Instant::now() < end,
                    "filesystem notification not delivered"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        let baseline = wait(&watch, 0);
        std::fs::write(dir.path().join("new.txt"), "hello").unwrap();
        let changed = wait(&watch, baseline);
        assert_eq!(watch.revision(baseline).unwrap(), Some(changed));
        assert_eq!(watch.revision(changed).unwrap(), None);
        git::run(dir.path(), &["add", "new.txt"], None).unwrap();
        let staged = wait(&watch, changed);
        std::fs::rename(dir.path().join("new.txt"), dir.path().join("renamed.txt")).unwrap();
        let renamed = wait(&watch, staged);
        std::fs::remove_file(dir.path().join("renamed.txt")).unwrap();
        assert!(wait(&watch, renamed) > renamed);
    }
}
