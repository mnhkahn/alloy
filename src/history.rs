//! Read-only history for the current branch, independent of the index/worktree.
use crate::git::{Document, Repository, hunks, run};
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, String>;
const PAGE_SIZE: usize = 100;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Commit {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub subject: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryPage {
    pub commits: Vec<Commit>,
    pub has_more: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    pub old_path: String,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Details {
    pub commit: Commit,
    pub message: String,
    pub files: Vec<Change>,
}
fn utf8(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
fn parse_commits(bytes: Vec<u8>) -> Result<Vec<Commit>> {
    let source = utf8(bytes)?;
    let fields: Vec<_> = source
        .strip_suffix('\0')
        .unwrap_or(&source)
        .split('\0')
        .collect();
    if source.is_empty() {
        return Ok(vec![]);
    }
    if fields.len() % 5 != 0 {
        return Err("Invalid Git log output".into());
    }
    Ok(fields
        .chunks_exact(5)
        .map(|f| Commit {
            id: f[0].into(),
            parents: f[1].split_whitespace().map(str::to_owned).collect(),
            author: f[2].into(),
            date: f[3].into(),
            subject: f[4].into(),
        })
        .collect())
}
fn commit_id(repo: &Repository, id: &str) -> Result<String> {
    if ![40, 64].contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid commit ID".into());
    }
    Ok(utf8(run(
        &repo.root,
        &["rev-parse", "--verify", &format!("{id}^{{commit}}")],
        None,
    )?)?
    .trim()
    .into())
}
pub fn list(repo: &Repository, skip: usize) -> Result<HistoryPage> {
    if run(&repo.root, &["rev-parse", "--verify", "HEAD"], None).is_err() {
        return Ok(HistoryPage {
            commits: vec![],
            has_more: false,
        });
    }
    let mut commits = parse_commits(run(
        &repo.root,
        &[
            "log",
            "--topo-order",
            "-z",
            "--format=%H%x00%P%x00%an%x00%aI%x00%s",
            &format!("--max-count={}", PAGE_SIZE + 1),
            &format!("--skip={skip}"),
            "HEAD",
            "--",
        ],
        None,
    )?)?;
    let has_more = commits.len() > PAGE_SIZE;
    commits.truncate(PAGE_SIZE);
    Ok(HistoryPage { commits, has_more })
}
pub fn details(repo: &Repository, id: &str) -> Result<Details> {
    let id = commit_id(repo, id)?;
    let commit = parse_commits(run(
        &repo.root,
        &[
            "show",
            "-s",
            "-z",
            "--format=%H%x00%P%x00%an%x00%aI%x00%s",
            &id,
            "--",
        ],
        None,
    )?)?
    .into_iter()
    .next()
    .ok_or("Missing commit")?;
    let message = utf8(run(
        &repo.root,
        &["show", "-s", "--format=%B", &id, "--"],
        None,
    )?)?;
    let mut args = vec![
        "diff-tree",
        "--root",
        "--no-commit-id",
        "-r",
        "--name-status",
        "-z",
        "-M",
    ];
    if let Some(parent) = commit.parents.first() {
        args.push(parent);
    }
    args.extend([id.as_str(), "--"]);
    let source = utf8(run(&repo.root, &args, None)?)?;
    let mut fields = source.split('\0').filter(|s| !s.is_empty());
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let first = fields.next().ok_or("Missing changed filename")?;
        let renamed = status.starts_with('R') || status.starts_with('C');
        let path = if renamed {
            fields.next().ok_or("Missing renamed filename")?
        } else {
            first
        };
        files.push(Change {
            path: path.into(),
            old_path: if renamed { first.into() } else { String::new() },
            status: status.chars().next().unwrap().to_string(),
        });
    }
    Ok(Details {
        commit,
        message,
        files,
    })
}
fn blob(repo: &Repository, revision: &str, path: &str) -> Result<(Vec<u8>, String)> {
    // ls-tree provides the object ID without using filenames as revision expressions.
    let entry = utf8(run(
        &repo.root,
        &["ls-tree", "-z", revision, "--", path],
        None,
    )?)?;
    if entry.is_empty() {
        return Err("Historical file is missing".into());
    }
    let metadata = entry.split('\t').next().ok_or("Missing tree entry")?;
    let fields: Vec<_> = metadata.split_whitespace().collect();
    if fields.len() != 3 {
        return Err("Invalid tree entry".into());
    }
    if fields[0] == "160000" {
        return Ok((vec![], format!("Submodule commit: {}", fields[2])));
    }
    let size: usize = utf8(run(&repo.root, &["cat-file", "-s", fields[2]], None)?)?
        .trim()
        .parse::<usize>()
        .map_err(|e| e.to_string())?;
    if size > crate::preview::MAX_IMAGE_BYTES {
        return Ok((vec![], "File exceeds the 32 MiB preview limit.".into()));
    }
    let bytes = run(&repo.root, &["cat-file", "blob", fields[2]], None)?;
    Ok((
        bytes,
        if fields[0] == "120000" {
            "Symbolic link target (read-only).".into()
        } else {
            String::new()
        },
    ))
}
pub fn diff(repo: &Repository, id: &str, path: &str) -> Result<Document> {
    let details = details(repo, id)?;
    let change = details
        .files
        .iter()
        .find(|f| f.path == path)
        .ok_or("File is not part of this commit")?;
    let previous = if change.old_path.is_empty() {
        path
    } else {
        &change.old_path
    };
    let (left, left_note) = match details.commit.parents.first() {
        Some(parent) if change.status != "A" => blob(repo, parent, previous)?,
        _ => (vec![], String::new()),
    };
    let (right, right_note) = if change.status == "D" {
        (vec![], String::new())
    } else {
        blob(repo, &details.commit.id, path)?
    };
    let mut doc = Document {
        path: path.into(),
        added: change.status == "A",
        exists: change.status != "D",
        editable: false,
        images: None,
        left: String::new(),
        right: String::new(),
        base: String::new(),
        ours: String::new(),
        theirs: String::new(),
        note: [left_note, right_note]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        conflict: false,
        staged: false,
        hunks: vec![],
        index_source: None,
        staged_hunks: None,
    };
    if doc.note.is_empty()
        && (crate::preview::supported_path(path)
            || crate::preview::has_image_signature(&left)
            || crate::preview::has_image_signature(&right))
    {
        doc.images = Some(crate::preview::ImageComparison {
            left: if change.status == "A" {
                None
            } else {
                Some(crate::preview::prepare(&left))
            },
            right: if change.status == "D" {
                None
            } else {
                Some(crate::preview::prepare(&right))
            },
        });
    } else if left.len() > 750_000
        || right.len() > 750_000
        || left.contains(&0)
        || right.contains(&0)
        || std::str::from_utf8(&left).is_err()
        || std::str::from_utf8(&right).is_err()
    {
        doc.note = "Binary, non-UTF-8 or oversized file; text diff unavailable.".into();
    } else {
        doc.left = utf8(left)?;
        doc.right = utf8(right)?;
        doc.hunks = hunks(&doc.left, &doc.right);
    }
    Ok(doc)
}
