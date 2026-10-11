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
    let mut stdout = tempfile::tempfile().map_err(err)?;
    let mut stderr = tempfile::tempfile().map_err(err)?;
    let mut stdin = tempfile::tempfile().map_err(err)?;
    if let Some(bytes) = input {
        stdin.write_all(bytes).map_err(err)?;
    }
    stdin.rewind().map_err(err)?;
    let mut child = Command::new("git")
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
        .stdin(Stdio::from(stdin))
        .stdout(stdout.try_clone().map_err(err)?)
        .stderr(stderr.try_clone().map_err(err)?)
        .spawn()
        .map_err(err)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(err)? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Git operation timed out after 120 seconds; refresh repository state before retrying.".into());
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
    pub start: usize,
    pub end: usize,
    pub replacement: String,
    pub label: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub path: String,
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
        let snapshot = self.snapshot()?;
        let entry = snapshot
            .files
            .iter()
            .find(|f| f.path == path)
            .ok_or("File is no longer in the change list; refresh.")?;
        let safe = self.safe_path(path);
        let conflict = entry.conflict;
        let old_path = if entry.old_path.is_empty() {
            path
        } else {
            &entry.old_path
        };
        let left = if staged {
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
            if meta.len() > MAX_TEXT as u64 {
                return Ok(uneditable(
                    path,
                    exists,
                    conflict,
                    staged,
                    "File exceeds the 750 KB editor limit; content is not truncated. Use an external editor.",
                ));
            }
            fs::read(&full).map_err(err)?
        };
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
        Ok(Document {
            path: path.into(),
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
    pub fn stage(&self, path: &str) -> Result<()> {
        let s = self.snapshot()?;
        let entry = s
            .files
            .iter()
            .find(|f| f.path == path)
            .ok_or("File is no longer changed")?;
        if entry.conflict {
            return Err("Use Mark resolved after reviewing the result.".into());
        }
        if entry.old_path.is_empty() {
            self.cmd(&["add", "-A", "--", path])?;
        } else {
            self.cmd(&["add", "-A", "--", path, &entry.old_path])?;
        }
        Ok(())
    }
    pub fn unstage(&self, path: &str) -> Result<()> {
        let s = self.snapshot()?;
        let entry = s
            .files
            .iter()
            .find(|f| f.path == path)
            .ok_or("File is no longer changed")?;
        if entry.conflict {
            return Err("Resolve the conflict before unstaging.".into());
        }
        let mut args = if self.cmd(&["rev-parse", "--verify", "HEAD"]).is_ok() {
            vec!["reset", "-q", "HEAD", "--", path]
        } else {
            vec!["rm", "--cached", "-r", "--", path]
        };
        if !entry.old_path.is_empty() {
            args.push(&entry.old_path);
        }
        self.cmd(&args)?;
        Ok(())
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
    pub fn commit(&self, message: &str) -> Result<String> {
        if message.trim().is_empty() {
            return Err("Enter a commit message.".into());
        }
        if self.snapshot()?.files.iter().any(|f| f.conflict) {
            return Err("Resolve all conflicts before committing.".into());
        }
        self.cmd(&["commit", "-m", message])
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
            "push" => self.cmd(&["push"]),
            _ => Err("Unknown network action".into()),
        }
    }
}
fn uneditable(path: &str, exists: bool, conflict: bool, staged: bool, note: &str) -> Document {
    Document {
        path: path.into(),
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
    let diff = TextDiff::from_lines(left, right);
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
            .map(|c| c.to_string())
            .collect();
        result.push(Hunk {
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
