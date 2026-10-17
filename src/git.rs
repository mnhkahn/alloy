//! Git runs off the UI thread. Arguments never pass through a shell.
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::{
    fs,
    io::{Read, Seek, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, String>;
const MAX_TEXT: usize = 750_000;

pub fn run(dir: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
    run_index(dir, args, input, None, 120)
}
fn run_index(
    dir: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    index: Option<&Path>,
    timeout_secs: u64,
) -> Result<Vec<u8>> {
    let mut stdout = tempfile::tempfile().map_err(err)?;
    let mut stderr = tempfile::tempfile().map_err(err)?;
    let mut stdin = tempfile::tempfile().map_err(err)?;
    if let Some(bytes) = input {
        stdin.write_all(bytes).map_err(err)?;
    }
    stdin.rewind().map_err(err)?;
    let mut command = Command::new("git");
    // Status is a read for Alloy. Git normally writes refreshed stat/racy-clean
    // cache entries, which can change the real index before a commit is locked.
    if args.first() == Some(&"status") {
        command.env("GIT_OPTIONAL_LOCKS", "0");
    }
    crate::git_proxy::apply(&mut command, args);
    let mut child = command
        .current_dir(dir)
        .args([
            "--no-pager",
            "--literal-pathspecs",
            "-c",
            "core.quotepath=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .envs(index.map(|path| ("GIT_INDEX_FILE", path)))
        .stdin(Stdio::from(stdin))
        .stdout(stdout.try_clone().map_err(err)?)
        .stderr(stderr.try_clone().map_err(err)?)
        .spawn()
        .map_err(err)?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(err)? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "Git operation timed out after {timeout_secs} seconds; refresh repository state before retrying."
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    stdout.rewind().map_err(err)?;
    stderr.rewind().map_err(err)?;
    let mut out = vec![];
    let mut errors = String::new();
    stdout.read_to_end(&mut out).map_err(err)?;
    stderr.read_to_string(&mut errors).map_err(err)?;
    if !status.success() {
        return Err(format!(
            "git {}: {}{}",
            args.first().unwrap_or(&""),
            errors.trim(),
            String::from_utf8_lossy(&out)
        ));
    }
    Ok(out)
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn text(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| "Non-UTF-8 content cannot be edited.".into())
}
fn command(dir: &Path, args: &[&str]) -> Result<String> {
    text(run(dir, args, None)?)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub old_path: String,
    pub index: String,
    pub work: String,
    pub conflict: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub root: String,
    pub branch: String,
    pub upstream: String,
    pub ahead: usize,
    pub behind: usize,
    pub files: Vec<FileEntry>,
    pub merging: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: usize,
    pub start: usize,
    pub end: usize,
    pub replacement: String,
    pub label: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub path: String,
    pub added: bool,
    pub images: Option<crate::preview::ImageComparison>,
    pub left: String,
    pub right: String,
    pub base: String,
    pub ours: String,
    pub theirs: String,
    pub exists: bool,
    pub editable: bool,
    pub note: String,
    pub conflict: bool,
    pub staged: bool,
    pub hunks: Vec<Hunk>,
    pub index_source: Option<String>,
    pub staged_hunks: Option<Vec<usize>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartialSelection {
    pub left: String,
    pub right: String,
    pub indices: Vec<usize>,
}
struct IndexLock(PathBuf);
impl Drop for IndexLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Clone, Debug)]
pub struct Repository {
    pub root: PathBuf,
}
impl Repository {
    pub fn open(path: &str) -> Result<Self> {
        let path = Path::new(path);
        let root = command(path, &["rev-parse", "--show-toplevel"])?;
        Ok(Self {
            root: fs::canonicalize(root.trim_end_matches('\n')).map_err(err)?,
        })
    }
    fn cmd(&self, args: &[&str]) -> Result<String> {
        command(&self.root, args)
    }
    fn bytes(&self, args: &[&str]) -> Result<Vec<u8>> {
        run(&self.root, args, None)
    }
    fn safe_path(&self, path: &str) -> Result<PathBuf> {
        if path.is_empty()
            || Path::new(path)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || Path::new(path)
                .components()
                .any(|c| c.as_os_str() == ".git")
        {
            return Err("Invalid repository-relative path".into());
        }
        let full = self.root.join(path);
        // Refuse symlinks at every level; never follow a tracked link into another file.
        let mut check = self.root.clone();
        for c in Path::new(path).components() {
            check.push(c);
            if fs::symlink_metadata(&check).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err("Symlink content is read-only in Alloy.".into());
            }
        }
        Ok(full)
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let bytes = self.bytes(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        let files = parse_status(&bytes)?;
        let branch = self
            .cmd(&["symbolic-ref", "--short", "HEAD"])
            .unwrap_or_else(|_| {
                self.cmd(&["rev-parse", "--short", "HEAD"])
                    .unwrap_or_else(|_| "unborn".into())
            });
        let upstream = self
            .cmd(&["rev-parse", "--abbrev-ref", "@{upstream}"])
            .unwrap_or_default();
        let counts = self
            .cmd(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
            .unwrap_or_default();
        let mut counts = counts.split_whitespace();
        let merging = self.cmd(&["rev-parse", "--verify", "MERGE_HEAD"]).is_ok();
        Ok(Snapshot {
            root: self.root.to_string_lossy().into(),
            branch: branch.trim().into(),
            upstream: upstream.trim().into(),
            ahead: counts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            behind: counts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            files,
            merging,
        })
    }
    fn blob(&self, spec: &str) -> Result<Vec<u8>> {
        self.bytes(&["show", spec])
    }
    fn optional_blob(&self, spec: &str) -> Result<Vec<u8>> {
        if self.cmd(&["cat-file", "-e", spec]).is_err() {
            return Ok(vec![]);
        }
        self.blob(spec)
    }
    pub fn document(&self, path: &str, staged: bool) -> Result<Document> {
        let entries = parse_status(&self.bytes(&[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ])?)?;
        let entry = entries
            .iter()
            .find(|f| f.path == path)
            .ok_or("File is no longer in the change list; refresh.")?;
        self.document_for_entry(entry, staged)
    }
    pub fn selected_document(&self, path: &str, old_path: &str, staged: bool) -> Result<Document> {
        self.selected_comparison(path, old_path, staged, false)
    }
    pub fn selected_comparison(
        &self,
        path: &str,
        old_path: &str,
        staged: bool,
        head: bool,
    ) -> Result<Document> {
        let mut args = vec![
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            path,
        ];
        if !old_path.is_empty() {
            args.push(old_path);
        }
        let entries = parse_status(&self.bytes(&args)?)?;
        let entry = entries
            .iter()
            .find(|f| f.path == path)
            .ok_or("File is no longer in the change list; refresh.")?;
        self.comparison_for_entry(entry, staged, head)
    }
    pub fn document_for_entry(&self, entry: &FileEntry, staged: bool) -> Result<Document> {
        self.comparison_for_entry(entry, staged, false)
    }
    pub fn comparison_for_entry(
        &self,
        entry: &FileEntry,
        staged: bool,
        head: bool,
    ) -> Result<Document> {
        let path = entry.path.as_str();
        let staged = staged && !entry.conflict;
        let safe = self.safe_path(path);
        let conflict = entry.conflict;
        let old_path = if entry.old_path.is_empty() {
            path
        } else {
            &entry.old_path
        };
        let left = if staged || head {
            self.optional_blob(&format!("HEAD:{old_path}"))?
        } else {
            self.optional_blob(&format!(":{path}"))?
        };
        let full = self.root.join(path);
        let exists = fs::symlink_metadata(&full).is_ok();
        let special = safe.is_err() || full.is_dir();
        let right = if staged {
            self.optional_blob(&format!(":{path}"))?
        } else if special || !exists {
            vec![]
        } else {
            let meta = fs::metadata(&full).map_err(err)?;
            let mut prefix = [0u8; 32];
            let count = fs::File::open(&full)
                .map_err(err)?
                .read(&mut prefix)
                .map_err(err)?;
            let is_image = crate::preview::supported_path(path)
                || crate::preview::has_image_signature(&prefix[..count]);
            let limit = if is_image {
                crate::preview::MAX_IMAGE_BYTES
            } else {
                MAX_TEXT
            };
            if meta.len() > limit as u64 {
                return Ok(uneditable(
                    path,
                    exists,
                    conflict,
                    staged,
                    if is_image {
                        "Image exceeds the 32 MiB preview limit."
                    } else {
                        "Text file exceeds the 750 KB editor limit; content is not truncated. Use an external editor."
                    },
                ));
            }
            fs::read(&full).map_err(err)?
        };
        if !special
            && !conflict
            && (crate::preview::supported_path(path)
                || crate::preview::has_image_signature(&left)
                || crate::preview::has_image_signature(&right))
        {
            let mut document = uneditable(path, exists, false, staged, "");
            document.added = entry.index == "?" || ((head || staged) && entry.index == "A");
            document.images = Some(crate::preview::ImageComparison {
                left: if left.is_empty() {
                    None
                } else {
                    Some(crate::preview::prepare(&left))
                },
                right: if right.is_empty() && (if staged { entry.index == "D" } else { !exists }) {
                    None
                } else {
                    Some(crate::preview::prepare(&right))
                },
            });
            return Ok(document);
        }
        let base = if conflict {
            self.optional_blob(&format!(":1:{path}"))?
        } else {
            vec![]
        };
        let ours = if conflict {
            self.optional_blob(&format!(":2:{path}"))?
        } else {
            vec![]
        };
        let theirs = if conflict {
            self.optional_blob(&format!(":3:{path}"))?
        } else {
            vec![]
        };
        if special
            || [&left, &right, &base, &ours, &theirs]
                .iter()
                .any(|b| b.len() > MAX_TEXT || b.contains(&0) || std::str::from_utf8(b).is_err())
        {
            return Ok(uneditable(
                path,
                exists,
                conflict,
                staged,
                "Binary, symlink, submodule, non-UTF-8 or oversized file. Text editing is unavailable; file staging remains available.",
            ));
        }
        let left = text(left)?;
        let right = text(right)?;
        let hunks = if conflict {
            vec![]
        } else {
            hunks(&left, &right)
        };
        let index_source = if head && !staged && !conflict && !entry.old_path.is_empty() {
            None
        } else if head && !staged && !conflict {
            text(self.optional_blob(&format!(":{path}"))?).ok()
        } else {
            None
        };
        let staged_hunks = index_source
            .as_ref()
            .and_then(|index| staged_hunk_indices(&left, &right, index));
        Ok(Document {
            index_source,
            staged_hunks,
            path: path.into(),
            added: entry.index == "?" || ((head || staged) && entry.index == "A"),
            images: None,
            left,
            right,
            base: text(base)?,
            ours: text(ours)?,
            theirs: text(theirs)?,
            exists,
            editable: !staged,
            note: String::new(),
            conflict,
            staged,
            hunks,
        })
    }
    pub fn save(&self, path: &str, expected: &str, existed: bool, content: &str) -> Result<()> {
        let full = self.safe_path(path)?;
        if content.len() > MAX_TEXT {
            return Err("File exceeds the 750 KB editor limit.".into());
        }
        let current = match fs::read(&full) {
            Ok(v) => Some(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(err(e)),
        };
        if current.is_some() != existed
            || current.as_deref().unwrap_or_default() != expected.as_bytes()
        {
            return Err("File changed on disk. Your draft was kept; reload before overwriting external changes.".into());
        }
        let parent = full.parent().ok_or("Missing parent")?;
        fs::create_dir_all(parent).map_err(err)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
        if let Ok(meta) = fs::metadata(&full) {
            temp.as_file()
                .set_permissions(meta.permissions())
                .map_err(err)?;
        }
        temp.write_all(content.as_bytes()).map_err(err)?;
        temp.as_file().sync_all().map_err(err)?;
        temp.persist(&full).map_err(err)?;
        Ok(())
    }
    /// Publish an alternate index atomically while respecting Git's index lock.
    fn edit_index(&self, edit: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
        let real = self
            .root
            .join(self.cmd(&["rev-parse", "--git-path", "index"])?.trim());
        let mut name = real.as_os_str().to_os_string();
        name.push(".lock");
        let lock = PathBuf::from(name);
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .map_err(|e| format!("Cannot lock Git index: {e}"))?;
        let _guard = IndexLock(lock.clone());
        drop(file);
        let dir = tempfile::tempdir_in(real.parent().ok_or("Index has no parent")?).map_err(err)?;
        let index = dir.path().join("index");
        if real.exists() {
            fs::copy(&real, &index).map_err(err)?;
        } else {
            run_index(
                &self.root,
                &["read-tree", "--empty"],
                None,
                Some(&index),
                120,
            )?;
        }
        edit(&index)?;
        fs::rename(&index, &lock).map_err(err)?;
        fs::rename(&lock, &real).map_err(err)?;
        Ok(())
    }
    pub fn stage_paths(&self, paths: &[String], stage: bool) -> Result<()> {
        if paths.is_empty() {
            return Err("No files selected".into());
        }
        self.edit_index(|index| {
            let snapshot = self.snapshot()?;
            let mut names = std::collections::BTreeSet::new();
            for path in paths {
                let entry = snapshot
                    .files
                    .iter()
                    .find(|f| &f.path == path)
                    .ok_or("File is no longer changed; refresh.")?;
                if entry.conflict {
                    return Err("Use Mark resolved after reviewing the conflict.".into());
                }
                names.insert(path.clone());
                if !entry.old_path.is_empty() {
                    names.insert(entry.old_path.clone());
                }
            }
            if stage {
                let mut add = vec!["add", "-A", "--"];
                for path in &names {
                    if fs::symlink_metadata(self.root.join(path)).is_ok()
                        || self
                            .cmd(&["ls-files", "--error-unmatch", "--", path])
                            .is_ok()
                    {
                        add.push(path);
                    }
                }
                if add.len() > 3 {
                    run_index(&self.root, &add, None, Some(index), 120)?;
                }
            } else if self.cmd(&["rev-parse", "--verify", "HEAD"]).is_ok() {
                let mut reset = vec!["reset", "-q", "HEAD", "--"];
                reset.extend(names.iter().map(String::as_str));
                run_index(&self.root, &reset, None, Some(index), 120)?;
            } else {
                for path in &names {
                    run_index(
                        &self.root,
                        &["update-index", "--force-remove", "--", path],
                        None,
                        Some(index),
                        120,
                    )?;
                }
            }
            Ok(())
        })
    }
    /// Paths present in the latest commit; staged additions are deliberately excluded.
    pub fn committed_paths(&self) -> Result<std::collections::BTreeSet<String>> {
        if self.cmd(&["rev-parse", "--verify", "HEAD"]).is_err() {
            return Ok(Default::default());
        }
        self.bytes(&["ls-tree", "-r", "--name-only", "-z", "HEAD"])?
            .split(|b| *b == 0)
            .filter(|path| !path.is_empty())
            .map(|path| String::from_utf8(path.to_vec()).map_err(err))
            .collect()
    }
    pub fn stage_committed_changes(&self) -> Result<()> {
        let snapshot = self.snapshot()?;
        if snapshot.merging || snapshot.files.iter().any(|f| f.conflict) {
            return Ok(());
        }
        let committed = self.committed_paths()?;
        let paths: Vec<_> = snapshot
            .files
            .iter()
            .filter(|f| committed.contains(&f.path) || committed.contains(&f.old_path))
            .map(|f| f.path.clone())
            .collect();
        if !paths.is_empty() {
            self.stage_paths(&paths, true)?;
        }
        Ok(())
    }
    /// Delete a changed working-tree file, preserving the index for explicit staging.
    pub fn delete_file(&self, path: &str) -> Result<()> {
        let snapshot = self.snapshot()?;
        if snapshot.merging || snapshot.files.iter().any(|f| f.conflict) {
            return Err("Finish the merge before deleting files.".into());
        }
        if !snapshot.files.iter().any(|f| f.path == path) {
            return Err("File is no longer in the change list; refresh before deleting.".into());
        }
        let full = self.safe_path(path)?;
        let metadata = fs::symlink_metadata(&full).map_err(err)?;
        if !metadata.is_file() {
            return Err("Delete is available only for regular files.".into());
        }
        fs::remove_file(full).map_err(err)
    }

    /// Restore committed files only. New files, including staged additions, survive.
    pub fn rollback(&self, path: Option<&str>) -> Result<()> {
        let snapshot = self.snapshot()?;
        if snapshot.merging || snapshot.files.iter().any(|f| f.conflict) {
            return Err("Finish the merge before rolling back changes.".into());
        }
        let committed = self.committed_paths()?;
        let files: Vec<_> = snapshot
            .files
            .iter()
            .filter(|f| path.is_none_or(|path| f.path == path))
            .filter(|f| committed.contains(&f.path) || committed.contains(&f.old_path))
            .collect();
        if path.is_some() && files.is_empty() {
            return Err("Rollback is available only for files in the latest commit.".into());
        }
        let mut restore = std::collections::BTreeSet::new();
        let mut renamed = Vec::new();
        for file in files {
            if committed.contains(&file.path) {
                restore.insert(file.path.as_str());
            }
            if committed.contains(&file.old_path) {
                restore.insert(file.old_path.as_str());
                if !committed.contains(&file.path) {
                    // Validate before any writes; never recursively remove a directory.
                    let full = self.safe_path(&file.path)?;
                    if fs::symlink_metadata(&full).is_ok_and(|m| m.is_dir()) {
                        return Err("Cannot roll back a renamed directory or submodule.".into());
                    }
                    renamed.push((file.path.as_str(), full));
                }
            }
        }
        if restore.is_empty() {
            return Ok(());
        }
        self.edit_index(|index| {
            let mut args = vec!["restore", "--source=HEAD", "--staged", "--worktree", "--"];
            args.extend(restore.iter().copied());
            run_index(&self.root, &args, None, Some(index), 120)?;
            for (name, full) in &renamed {
                run_index(
                    &self.root,
                    &["update-index", "--force-remove", "--", name],
                    None,
                    Some(index),
                    120,
                )?;
                match fs::remove_file(full) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(err(e)),
                }
            }
            Ok(())
        })
    }
    pub fn stage(&self, path: &str) -> Result<()> {
        self.stage_paths(&[path.into()], true)
    }
    pub fn unstage(&self, path: &str) -> Result<()> {
        self.stage_paths(&[path.into()], false)
    }
    pub fn stage_hunks(
        &self,
        path: &str,
        selection: &PartialSelection,
        expected_index: &str,
    ) -> Result<()> {
        self.edit_index(|index| {
            let doc = self.selected_comparison(path,"",false,true)?;
            if !doc.editable || !doc.exists || doc.added || doc.conflict || doc.staged_hunks.is_none() {return Err("Use the file checkbox for this change; its staged content cannot be represented by these blocks.".into());}
            if doc.left != selection.left || doc.right != selection.right || doc.index_source.as_deref() != Some(expected_index) {return Err("File or index changed; refresh before selecting blocks.".into());}
            let content = if selection.indices.is_empty() {selection.left.clone()} else {selected_hunks(&selection.left,&selection.right,&selection.indices)?};
            let entry = self.cmd(&["ls-files", "--stage", "--", path])?;
            let mode = entry.split_whitespace().next().ok_or("Missing index entry")?;
            if !["100644","100755"].contains(&mode) {return Err("Partial staging requires a regular file.".into());}
            let oid = text(run_index(&self.root,&["hash-object","-w","--stdin"],Some(content.as_bytes()),Some(index),120)?)?;
            run_index(&self.root,&["update-index","--cacheinfo",mode,oid.trim(),path],None,Some(index),120)?; Ok(())
        })
    }
    pub fn resolve(&self, path: &str, expected: &str, existed: bool, content: &str) -> Result<()> {
        if !self
            .snapshot()?
            .files
            .iter()
            .any(|f| f.path == path && f.conflict)
        {
            return Err("File is no longer conflicted; refresh.".into());
        }
        if has_markers(content) {
            return Err(
                "Conflict markers remain. Resolve every block before marking resolved.".into(),
            );
        }
        self.save(path, expected, existed, content)?;
        self.cmd(&["add", "--", path])?;
        Ok(())
    }
    pub fn resolve_delete(&self, path: &str, expected: &str, existed: bool) -> Result<()> {
        if !self
            .snapshot()?
            .files
            .iter()
            .any(|f| f.path == path && f.conflict)
        {
            return Err("File is no longer conflicted".into());
        }
        let full = self.safe_path(path)?;
        let current = fs::read(&full).ok();
        if current.is_some() != existed
            || current.as_deref().unwrap_or_default() != expected.as_bytes()
        {
            return Err("File changed on disk; refresh before deleting.".into());
        }
        self.cmd(&["rm", "-f", "--", path])?;
        Ok(())
    }
    pub fn merge_message(&self) -> Result<String> {
        let path = self.cmd(&["rev-parse", "--git-path", "MERGE_MSG"])?;
        let content = fs::read_to_string(self.root.join(path.trim())).map_err(err)?;
        Ok(content
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string())
    }
    pub fn abort_merge(&self) -> Result<()> {
        if !self.snapshot()?.merging {
            return Err("No merge is in progress.".into());
        }
        self.cmd(&["merge", "--abort"])?;
        Ok(())
    }
    pub fn commit(&self, message: &str) -> Result<String> {
        if message.trim().is_empty() {
            return Err("Enter a commit message.".into());
        }
        if self.snapshot()?.files.iter().any(|f| f.conflict) {
            return Err("Resolve all conflicts before committing.".into());
        }
        self.cmd(&["commit", "-m", message])
    }
    /// `git diff --cached` against HEAD, or the empty tree before the first commit.
    fn diff_cached(&self, extra: &[&str]) -> Result<Vec<u8>> {
        let mut args = vec!["diff", "--cached"];
        if self.cmd(&["rev-parse", "--verify", "HEAD"]).is_err() {
            args.push("4b825dc642cb6eb9a060e54bf8d69288fbee4904");
        }
        args.extend_from_slice(extra);
        self.bytes(&args)
    }
    pub fn staged_names(&self) -> Result<Vec<String>> {
        Ok(self
            .diff_cached(&["--name-only", "-z"])?
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect())
    }
    pub fn stage_all(&self) -> Result<()> {
        self.cmd(&["add", "."])?;
        Ok(())
    }
    pub fn staged_numstat(&self) -> Result<String> {
        text(self.diff_cached(&["--numstat"])?)
    }
    pub fn staged_diff(&self) -> Result<String> {
        text(self.diff_cached(&[])?)
    }
    /// True when the working tree and index are clean, including untracked files.
    pub fn clean(&self) -> Result<bool> {
        Ok(self
            .bytes(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?
            .is_empty())
    }
    pub fn remotes(&self) -> Result<Vec<String>> {
        Ok(self
            .cmd(&["remote"])?
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect())
    }
    /// The single push URL of a remote; multiple push URLs are an error.
    pub fn push_url(&self, remote: &str) -> Result<String> {
        let out = self.cmd(&["remote", "get-url", "--push", "--all", remote])?;
        let mut lines = out.lines().map(str::trim).filter(|l| !l.is_empty());
        let url = lines
            .next()
            .ok_or_else(|| format!("Remote {remote} has no push URL."))?;
        if lines.next().is_some() {
            return Err(format!("Remote {remote} has multiple push URLs."));
        }
        Ok(url.to_string())
    }
    pub fn local_tags(&self) -> Result<Vec<String>> {
        Ok(self
            .cmd(&["tag", "--list"])?
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect())
    }
    /// Tag names advertised by a remote URL, queried with ls-remote (no fetch).
    pub fn remote_tags(&self, url: &str) -> Result<Vec<String>> {
        let out = text(run_index(
            &self.root,
            &["ls-remote", "--tags", url],
            None,
            None,
            30,
        ).map_err(|error| format!("Cannot read remote tags (git ls-remote). Check network/proxy and Git authentication, then retry Tag. No tag was created. {error}"))?)?;
        let mut tags = std::collections::BTreeSet::new();
        for line in out.lines() {
            if let Some((_, reference)) = line.split_once('\t')
                && let Some(name) = reference.strip_prefix("refs/tags/")
            {
                tags.insert(name.strip_suffix("^{}").unwrap_or(name).to_string());
            }
        }
        Ok(tags.into_iter().collect())
    }
    /// Full HEAD sha and its subject line.
    pub fn head_info(&self) -> Result<(String, String)> {
        let out = self.cmd(&["log", "-1", "--format=%H%x00%s"])?;
        let (sha, subject) = out
            .trim_end()
            .split_once('\0')
            .ok_or("Cannot read HEAD commit.")?;
        Ok((sha.to_string(), subject.to_string()))
    }
    pub fn head_sha(&self) -> Result<String> {
        Ok(self.cmd(&["rev-parse", "HEAD"])?.trim().to_string())
    }
    pub fn branch(&self) -> Result<String> {
        Ok(self
            .cmd(&["symbolic-ref", "--short", "HEAD"])
            .unwrap_or_else(|_| "detached".into())
            .trim()
            .to_string())
    }
    /// Annotated tag: `git tag -a <version> <commit> -m "Release <version>"`.
    pub fn create_tag(&self, version: &str, commit: &str) -> Result<()> {
        self.cmd(&[
            "tag",
            "-a",
            version,
            commit,
            "-m",
            &format!("Release {version}"),
        ])?;
        Ok(())
    }
    /// Commit a tag points at.
    pub fn tag_commit(&self, version: &str) -> Result<String> {
        Ok(self
            .cmd(&["rev-parse", &format!("{version}^{{commit}}")])?
            .trim()
            .to_string())
    }
    /// `git push <remote> refs/tags/<v>:refs/tags/<v>`; never forced.
    pub fn push_tag(&self, remote: &str, version: &str) -> Result<()> {
        self.cmd(&[
            "push",
            remote,
            &format!("refs/tags/{version}:refs/tags/{version}"),
        ])?;
        Ok(())
    }
    /// Commit checked files from the working tree, preserving unchecked index entries.
    /// Legacy selected-file commit API; the GUI now stages explicitly and uses `commit`.
    pub fn commit_selected(&self, message: &str, paths: &[String]) -> Result<String> {
        if message.trim().is_empty() {
            return Err("Enter a commit message.".into());
        }
        if paths.is_empty() {
            return Err("Select files to include in this commit.".into());
        }
        let snapshot = self.snapshot()?;
        if snapshot.merging {
            return Err("A merge must be committed as a whole. Use Commit merge (staged).".into());
        }
        if snapshot.files.iter().any(|f| f.conflict) {
            return Err("Resolve all conflicts before committing.".into());
        }
        let mut selected = std::collections::BTreeSet::new();
        let mut to_stage = std::collections::BTreeSet::new();
        for path in paths {
            let file = snapshot
                .files
                .iter()
                .find(|f| &f.path == path)
                .ok_or("A selected file is no longer changed; refresh before committing.")?;
            selected.insert(file.path.as_str());
            if file.index != "D" || fs::symlink_metadata(self.root.join(&file.path)).is_ok() {
                to_stage.insert(file.path.as_str());
            }
            if !file.old_path.is_empty() {
                selected.insert(file.old_path.as_str());
                if self
                    .cmd(&["ls-files", "--error-unmatch", "--", &file.old_path])
                    .is_ok()
                {
                    to_stage.insert(file.old_path.as_str());
                }
            }
        }
        let mut add = vec!["add", "-A", "--"];
        add.extend(to_stage.iter().copied());
        if !to_stage.is_empty() {
            self.cmd(&add)?;
        }
        let mut commit = vec!["commit", "--only", "-m", message, "--"];
        commit.extend(selected.iter().copied());
        self.cmd(&commit)
    }
    /// Commit a mixture of whole files and selected blocks without replacing working files.
    /// Both indices are prepared before committing; the real index stays locked until publication.
    pub fn commit_parts(
        &self,
        message: &str,
        paths: &[String],
        partial: &std::collections::BTreeMap<String, PartialSelection>,
    ) -> Result<String> {
        if partial.is_empty() {
            return self.commit_selected(message, paths);
        }
        if message.trim().is_empty() {
            return Err("Enter a commit message.".into());
        }
        let snapshot = self.snapshot()?;
        if snapshot.merging || snapshot.files.iter().any(|f| f.conflict) {
            return Err(
                "Partial commits are unavailable during a merge or unresolved conflict.".into(),
            );
        }
        let index_name = self.cmd(&["rev-parse", "--git-path", "index"])?;
        let real_index = self.root.join(index_name.trim());
        let mut lock_name = real_index.as_os_str().to_os_string();
        lock_name.push(".lock");
        let lock_path = PathBuf::from(lock_name);
        let lock_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|e| format!("Cannot lock Git index: {e}"))?;
        let _lock = IndexLock(lock_path.clone());
        drop(lock_file);
        let temp =
            tempfile::tempdir_in(real_index.parent().ok_or("Index has no parent")?).map_err(err)?;
        let commit_index = temp.path().join("commit-index");
        let next_index = temp.path().join("next-index");
        let cmd = |index: &Path, args: &[&str]| {
            text(run_index(&self.root, args, None, Some(index), 120)?)
        };
        cmd(&commit_index, &["read-tree", "HEAD"])?;
        if real_index.exists() {
            fs::copy(&real_index, &next_index).map_err(err)?;
        } else {
            cmd(&next_index, &["read-tree", "HEAD"])?;
        }
        for path in paths {
            if partial.contains_key(path) {
                return Err("File selected both wholly and partially.".into());
            }
            let entry = snapshot
                .files
                .iter()
                .find(|f| &f.path == path)
                .ok_or("Selected file disappeared; refresh.")?;
            // Starting from HEAD makes both sides of a rename valid pathspecs.
            let mut add = vec!["add", "-A", "--", path.as_str()];
            if !entry.old_path.is_empty() {
                add.push(&entry.old_path);
            }
            cmd(&commit_index, &add)?;
            // Reconcile using the exact selected tree entries, including staged deletions.
            let mut reset = vec!["reset", "-q", "HEAD", "--", path.as_str()];
            if !entry.old_path.is_empty() {
                reset.push(&entry.old_path);
            }
            cmd(&next_index, &reset)?;
            cmd(&next_index, &add)?;
        }
        for (path, selection) in partial {
            let entry = snapshot
                .files
                .iter()
                .find(|f| &f.path == path)
                .ok_or("Selected file disappeared; refresh.")?;
            let document = self.comparison_for_entry(entry, false, true)?;
            if !document.editable
                || !document.exists
                || document.added
                || !entry.old_path.is_empty()
            {
                return Err("Use whole-file selection for additions, deletions, renames, or non-text files.".into());
            }
            if selection.left != document.left || selection.right != document.right {
                return Err(format!(
                    "{path} changed since its blocks were selected. Review and select again."
                ));
            }
            let content = selected_hunks(&selection.left, &selection.right, &selection.indices)?;
            let staged = text(self.optional_blob(&format!(":{path}"))?)?;
            let reconciled = if staged == selection.left {
                content.clone()
            } else if staged == selection.right || staged == content {
                staged
            } else {
                let ours = temp.path().join("staged");
                let base = temp.path().join("base");
                let theirs = temp.path().join("selected");
                fs::write(&ours, staged).map_err(err)?;
                fs::write(&base, &selection.left).map_err(err)?;
                fs::write(&theirs, &content).map_err(err)?;
                self.cmd(&["merge-file", "-p", ours.to_str().ok_or("Invalid temporary path")?, base.to_str().ok_or("Invalid temporary path")?, theirs.to_str().ok_or("Invalid temporary path")?])
                        .map_err(|_| format!("Selected blocks overlap different staged edits in {path}; unstage this file and select again. Nothing was committed."))?
            };
            let tree = self.cmd(&["ls-tree", "HEAD", "--", path])?;
            let mode = tree.split_whitespace().next().ok_or("Missing HEAD mode")?;
            if mode != "100644" && mode != "100755" {
                return Err("Partial commits require regular text files.".into());
            }
            for (index, value) in [(&commit_index, content), (&next_index, reconciled)] {
                let oid = text(run_index(
                    &self.root,
                    &["hash-object", "-w", "--stdin"],
                    Some(value.as_bytes()),
                    Some(index),
                    120,
                )?)?;
                cmd(
                    index,
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        mode,
                        oid.trim(),
                        path,
                    ],
                )?;
            }
        }
        let output = cmd(&commit_index, &["commit", "-m", message])?;
        // No working-tree writes. Unchecked entries are copied intact from the original index.
        fs::rename(&next_index, &lock_path)
            .map_err(|e| format!("Commit succeeded, but preparing the index failed: {e}"))?;
        fs::rename(&lock_path, &real_index)
            .map_err(|e| format!("Commit succeeded, but publishing the index failed: {e}"))?;
        Ok(output)
    }
    pub fn has_upstream(&self) -> Result<bool> {
        let branch = self
            .cmd(&["symbolic-ref", "--quiet", "--short", "HEAD"])
            .map_err(|_| "Cannot push from detached HEAD. Check out a branch first.".to_string())?;
        let reference = format!("refs/heads/{}", branch.trim());
        // Configuration is valid even before the tracking ref has been fetched.
        Ok(!self
            .cmd(&["for-each-ref", "--format=%(upstream)", &reference])?
            .trim()
            .is_empty())
    }
    fn push(&self) -> Result<String> {
        let branch = self
            .cmd(&["symbolic-ref", "--quiet", "--short", "HEAD"])
            .map_err(|_| "Cannot push from detached HEAD. Check out a branch first.".to_string())?;
        let branch = branch.trim();
        self.head_sha()
            .map_err(|_| "Create a commit before pushing.".to_string())?;
        let reference = format!("refs/heads/{branch}");
        // Read configured tracking information even if its remote-tracking ref is missing.
        let upstream = self.cmd(&["for-each-ref", "--format=%(upstream)", &reference])?;
        if !upstream.trim().is_empty() {
            return self.cmd(&["push"]);
        }
        let remotes = self.remotes()?;
        let configured = [
            format!("branch.{branch}.pushRemote"),
            "remote.pushDefault".into(),
            format!("branch.{branch}.remote"),
        ]
        .iter()
        .find_map(|key| {
            self.cmd(&["config", "--get", key])
                .ok()
                .map(|s| s.trim().to_string())
        });
        let remote = if let Some(remote) = configured {
            if !remotes.contains(&remote) {
                return Err(format!(
                    "Configured push remote {remote:?} is not a named remote. Configure a push remote first."
                ));
            }
            remote
        } else if remotes.iter().any(|r| r == "origin") {
            "origin".into()
        } else if remotes.len() == 1 {
            remotes[0].clone()
        } else if remotes.is_empty() {
            return Err("No remote configured. Add a remote before pushing.".into());
        } else {
            return Err(
                "Multiple remotes and no origin. Set remote.pushDefault to choose a push remote."
                    .into(),
            );
        };
        self.push_url(&remote)?;
        let refspec = format!("{reference}:{reference}");
        let output = self.cmd(&["push", "--set-upstream", "--", &remote, &refspec])?;
        Ok(format!(
            "Pushed {branch} and set upstream to {remote}/{branch}.\n{output}"
        ))
    }
    pub fn network(&self, action: &str) -> Result<String> {
        match action {
            "fetch" => self.cmd(&["fetch", "--prune"]),
            "pull" => {
                if !self.snapshot()?.files.is_empty() {
                    return Err("Commit or stash local changes before pulling.".into());
                }
                self.cmd(&["pull", "--no-rebase", "--no-edit"])
            }
            "push" => self.push(),
            _ => Err("Unknown network action".into()),
        }
    }
}
fn uneditable(path: &str, exists: bool, conflict: bool, staged: bool, note: &str) -> Document {
    Document {
        path: path.into(),
        added: false,
        images: None,
        left: String::new(),
        right: String::new(),
        base: String::new(),
        ours: String::new(),
        theirs: String::new(),
        exists,
        editable: false,
        note: note.into(),
        conflict,
        staged,
        hunks: vec![],
        index_source: None,
        staged_hunks: None,
    }
}
pub fn parse_status(bytes: &[u8]) -> Result<Vec<FileEntry>> {
    let mut records = bytes.split(|b| *b == 0).filter(|r| !r.is_empty());
    let mut result = vec![];
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            return Err("Malformed Git status".into());
        }
        let index = record[0] as char;
        let work = record[1] as char;
        let path = std::str::from_utf8(&record[3..])
            .map_err(|_| "Non-UTF-8 filenames are not supported")?
            .to_string();
        let old_path = if index == 'R' || index == 'C' || work == 'R' || work == 'C' {
            text(records.next().ok_or("Missing rename source")?.to_vec())?
        } else {
            String::new()
        };
        let conflict = index == 'U'
            || work == 'U'
            || (index == 'A' && work == 'A')
            || (index == 'D' && work == 'D');
        result.push(FileEntry {
            path,
            old_path,
            index: index.to_string(),
            work: work.to_string(),
            conflict,
        });
    }
    Ok(result)
}
pub fn hunks(left: &str, right: &str) -> Vec<Hunk> {
    let diff = TextDiff::configure()
        .algorithm(similar::Algorithm::Patience)
        .diff_lines(left, right);
    let mut result = vec![];
    for group in diff.grouped_ops(0) {
        let first = &group[0];
        let last = group.last().unwrap();
        let start = first.new_range().start;
        let end = last.new_range().end;
        let replacement: String = group
            .iter()
            .flat_map(|op| diff.iter_changes(op))
            .filter(|c| c.tag() != ChangeTag::Insert)
            .map(|c| c.value().to_owned())
            .collect();
        result.push(Hunk {
            old_start: first.old_range().start,
            start,
            end,
            replacement,
            label: format!(
                "Lines {}–{}  ·  −{} +{}",
                start + 1,
                end.max(start + 1),
                last.old_range().end - first.old_range().start,
                end - start
            ),
        });
    }
    result
}
/// Match index changes to the exact visible HEAD-to-working-tree blocks.
/// Refuse ambiguous/overlapping external staging rather than dropping staged edits.
pub fn staged_hunk_indices(left: &str, right: &str, index: &str) -> Option<Vec<usize>> {
    let visible = hunks(left, right);
    let staged = hunks(left, index);
    let right_lines: Vec<_> = right.split_inclusive('\n').collect();
    let index_lines: Vec<_> = index.split_inclusive('\n').collect();
    let indices: Vec<_> = visible
        .iter()
        .enumerate()
        .filter_map(|(i, h)| {
            staged
                .iter()
                .any(|s| {
                    s.old_start == h.old_start
                        && s.replacement == h.replacement
                        && index_lines[s.start..s.end] == right_lines[h.start..h.end]
                })
                .then_some(i)
        })
        .collect();
    let reconstructed = if indices.is_empty() {
        left.to_owned()
    } else {
        selected_hunks(left, right, &indices).ok()?
    };
    (reconstructed == index).then_some(indices)
}
pub fn selected_hunks(left: &str, right: &str, indices: &[usize]) -> Result<String> {
    let blocks = hunks(left, right);
    let selected: std::collections::BTreeSet<_> = indices.iter().copied().collect();
    if selected.is_empty() || selected.iter().any(|i| *i >= blocks.len()) {
        return Err("Invalid or empty block selection; review again.".into());
    }
    let mut lines: Vec<String> = right.split_inclusive('\n').map(str::to_owned).collect();
    for (i, hunk) in blocks.iter().enumerate().rev() {
        if !selected.contains(&i) {
            lines.splice(
                hunk.start..hunk.end,
                hunk.replacement.split_inclusive('\n').map(str::to_owned),
            );
        }
    }
    Ok(lines.concat())
}
pub fn restore_hunk(left: &str, right: &str, index: usize) -> Result<String> {
    let h = hunks(left, right)
        .into_iter()
        .nth(index)
        .ok_or("Hunk no longer exists")?;
    let lines: Vec<&str> = right.split_inclusive('\n').collect();
    Ok(format!(
        "{}{}{}",
        lines[..h.start].concat(),
        h.replacement,
        lines[h.end..].concat()
    ))
}
pub fn has_markers(s: &str) -> bool {
    s.lines().any(|l| {
        ["<<<<<<<", "|||||||", "=======", ">>>>>>>"]
            .iter()
            .any(|prefix| l.starts_with(prefix))
    })
}
/// Resolve one numbered conflict, preserving all nonconflicting text and diff3 bases.
pub fn choose_conflict(s: &str, index: usize, choice: &str) -> Result<String> {
    if !["ours", "theirs", "both"].contains(&choice) {
        return Err("Unknown conflict choice".into());
    }
    let lines: Vec<&str> = s.split_inclusive('\n').collect();
    let mut blocks = vec![];
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].starts_with("<<<<<<<") {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        let ours_start = i;
        while i < lines.len()
            && !lines[i].starts_with("|||||||")
            && !lines[i].starts_with("=======")
        {
            i += 1;
        }
        let ours_end = i;
        while i < lines.len() && !lines[i].starts_with("=======") {
            i += 1;
        }
        if i == lines.len() {
            return Err("Incomplete conflict markers; edit manually.".into());
        }
        i += 1;
        let theirs_start = i;
        while i < lines.len() && !lines[i].starts_with(">>>>>>>") {
            i += 1;
        }
        if i == lines.len() {
            return Err("Incomplete conflict markers; edit manually.".into());
        }
        let theirs_end = i;
        i += 1;
        blocks.push((start, i, ours_start, ours_end, theirs_start, theirs_end));
    }
    let &(start, end, os, oe, ts, te) = blocks.get(index).ok_or("Conflict no longer exists")?;
    let mut result = lines[..start].concat();
    if choice != "theirs" {
        result.push_str(&lines[os..oe].concat());
    }
    if choice != "ours" {
        result.push_str(&lines[ts..te].concat());
    }
    result.push_str(&lines[end..].concat());
    Ok(result)
}
pub fn clone_repository(url: &str, destination: &str) -> Result<Repository> {
    if url.trim().is_empty() || url.starts_with('-') {
        return Err("Enter a repository URL".into());
    }
    let path = Path::new(destination);
    if !path.is_absolute() || path.exists() {
        return Err("Choose a new absolute destination path.".into());
    }
    let parent = path.parent().ok_or("Invalid destination")?;
    run(parent, &["clone", "--", url, destination], None)?;
    Repository::open(destination)
}
