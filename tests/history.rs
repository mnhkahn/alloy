use alloy::{
    git::{Repository, run},
    history,
};
use std::{fs, path::Path};
use tempfile::TempDir;
fn git(root: &Path, args: &[&str]) {
    run(root, args, None).unwrap();
}
fn fixture() -> (TempDir, Repository) {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "History Author"]);
    git(
        dir.path(),
        &["config", "user.email", "history@example.invalid"],
    );
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(dir.path(), &["config", "core.hooksPath", "/dev/null"]);
    let repo = Repository::open(dir.path().to_str().unwrap()).unwrap();
    (dir, repo)
}
fn commit(repo: &Repository, message: &str) -> String {
    git(&repo.root, &["add", "-A"]);
    git(&repo.root, &["commit", "-m", message]);
    String::from_utf8(run(&repo.root, &["rev-parse", "HEAD"], None).unwrap())
        .unwrap()
        .trim()
        .into()
}
#[test]
fn history_reads_root_and_full_message_without_touching_local_changes() {
    let (_dir, repo) = fixture();
    assert!(history::list(&repo, 0).unwrap().commits.is_empty());
    fs::write(repo.root.join("file.txt"), "original\n").unwrap();
    let root = commit(
        &repo,
        "Initial subject\n\nFull body with 中文\nSecond paragraph.",
    );
    fs::write(repo.root.join("file.txt"), "working edit\n").unwrap();
    repo.stage("file.txt").unwrap();
    fs::write(repo.root.join("file.txt"), "unstaged edit\n").unwrap();
    let index = run(&repo.root, &["show", ":file.txt"], None).unwrap();
    let page = history::list(&repo, 0).unwrap();
    assert_eq!(page.commits[0].id, root);
    assert_eq!(page.commits[0].author, "History Author");
    assert!(!page.has_more);
    let details = history::details(&repo, &root).unwrap();
    assert!(
        details
            .message
            .contains("Full body with 中文\nSecond paragraph.")
    );
    assert_eq!(details.files[0].status, "A");
    let diff = history::diff(&repo, &root, "file.txt").unwrap();
    assert_eq!(diff.left, "");
    assert_eq!(diff.right, "original\n");
    assert!(!diff.editable);
    assert!(diff.staged_hunks.is_none());
    assert_eq!(
        run(&repo.root, &["show", ":file.txt"], None).unwrap(),
        index
    );
    assert_eq!(
        fs::read(repo.root.join("file.txt")).unwrap(),
        b"unstaged edit\n"
    );
    assert!(history::details(&repo, "--all").is_err());
    assert!(history::diff(&repo, &root, "missing").is_err());
}
#[test]
fn historical_renames_deletions_and_literal_filenames() {
    let (_dir, repo) = fixture();
    let old = ":[literal] old\n.txt";
    let new = ":[literal] new\n.txt";
    fs::write(repo.root.join(old), "original\n").unwrap();
    fs::write(repo.root.join("deleted.txt"), "deleted content\n").unwrap();
    let first = commit(&repo, "initial");
    git(&repo.root, &["mv", old, new]);
    fs::remove_file(repo.root.join("deleted.txt")).unwrap();
    let second = commit(&repo, "rename and delete");
    let details = history::details(&repo, &second).unwrap();
    assert!(
        details
            .files
            .iter()
            .any(|f| f.path == new && f.old_path == old && f.status == "R")
    );
    let diff = history::diff(&repo, &second, new).unwrap();
    assert_eq!(diff.left, "original\n");
    assert_eq!(diff.right, "original\n");
    let diff = history::diff(&repo, &second, "deleted.txt").unwrap();
    assert_eq!(diff.left, "deleted content\n");
    assert_eq!(diff.right, "");
    let page = history::list(&repo, 1).unwrap();
    assert_eq!(page.commits[0].id, first);
}
#[test]
fn history_merge_compares_first_parent() {
    let (_dir, repo) = fixture();
    fs::write(repo.root.join("base.txt"), "base\n").unwrap();
    commit(&repo, "base");
    git(&repo.root, &["checkout", "-b", "topic"]);
    fs::write(repo.root.join("topic.txt"), "topic\n").unwrap();
    commit(&repo, "topic");
    git(&repo.root, &["checkout", "main"]);
    fs::write(repo.root.join("main.txt"), "main\n").unwrap();
    let parent = commit(&repo, "main");
    git(
        &repo.root,
        &["merge", "--no-ff", "topic", "-m", "merge topic"],
    );
    let merge = history::list(&repo, 0).unwrap().commits[0].clone();
    assert_eq!(merge.parents.len(), 2);
    assert_eq!(merge.parents[0], parent);
    let details = history::details(&repo, &merge.id).unwrap();
    assert_eq!(details.files.len(), 1);
    assert_eq!(details.files[0].path, "topic.txt");
    let diff = history::diff(&repo, &merge.id, "topic.txt").unwrap();
    assert_eq!(diff.right, "topic\n");
}
#[test]
fn history_previews_images_and_reports_binary_files() {
    let (_dir, repo) = fixture();
    image::DynamicImage::new_rgb8(4, 4)
        .save(repo.root.join("image.png"))
        .unwrap();
    fs::write(repo.root.join("binary.bin"), [0, 1, 255]).unwrap();
    let first = commit(&repo, "assets");
    let diff = history::diff(&repo, &first, "image.png").unwrap();
    assert!(diff.images.unwrap().right.is_some());
    let diff = history::diff(&repo, &first, "binary.bin").unwrap();
    assert!(diff.note.contains("Binary"));
    assert!(diff.right.is_empty());
}

#[test]
fn paginates_history_and_accepts_empty_commit_messages() {
    let (_dir, repo) = fixture();
    let tree = String::from_utf8(run(&repo.root, &["mktree"], None).unwrap()).unwrap();
    let mut parent = String::new();
    for i in 0..101 {
        let message = format!("commit {i}");
        let mut args = vec!["commit-tree", tree.trim(), "-m", &message];
        if !parent.is_empty() {
            args.extend(["-p", &parent]);
        }
        parent = String::from_utf8(run(&repo.root, &args, None).unwrap())
            .unwrap()
            .trim()
            .into();
    }
    git(&repo.root, &["update-ref", "refs/heads/main", &parent]);
    let page = history::list(&repo, 0).unwrap();
    assert_eq!(page.commits.len(), 100);
    assert!(page.has_more);
    let last = history::list(&repo, 100).unwrap();
    assert_eq!(last.commits.len(), 1);
    assert!(!last.has_more);
    git(
        &repo.root,
        &["commit", "--allow-empty", "--allow-empty-message", "-m", ""],
    );
    let blank = history::list(&repo, 0).unwrap().commits[0].clone();
    assert_eq!(blank.subject, "");
    assert!(history::details(&repo, &blank.id).unwrap().files.is_empty());
}
