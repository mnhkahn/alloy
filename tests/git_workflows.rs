use alloy::git::*;
use std::{fs, path::Path};
use tempfile::TempDir;
fn git(root: &Path, args: &[&str]) {
    run(root, args, None).unwrap();
}
fn fixture() -> (TempDir, Repository) {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(dir.path(), &["config", "core.hooksPath", "/dev/null"]);
    git(dir.path(), &["config", "user.name", "Alloy Test"]);
    git(
        dir.path(),
        &["config", "user.email", "alloy@example.invalid"],
    );
    fs::write(dir.path().join("file.txt"), "one\ntwo\nthree\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);
    let repo = Repository::open(dir.path().to_str().unwrap()).unwrap();
    (dir, repo)
}
#[test]
fn edit_save_stage_unstage_commit() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "one\nchanged\nthree\n").unwrap();
    let d = r.document("file.txt", false).unwrap();
    assert_eq!(d.left, "one\ntwo\nthree\n");
    assert_eq!(d.hunks.len(), 1);
    assert_eq!(restore_hunk(&d.left, &d.right, 0).unwrap(), d.left);
    r.save(&d.path, &d.right, true, "one\ncustom\nthree\n")
        .unwrap();
    r.stage(&d.path).unwrap();
    assert_eq!(r.snapshot().unwrap().files[0].index, "M");
    let staged = r.document(&d.path, true).unwrap();
    assert!(staged.right.contains("custom"));
    assert!(!staged.editable);
    r.unstage(&d.path).unwrap();
    assert_eq!(r.snapshot().unwrap().files[0].index, " ");
    r.stage(&d.path).unwrap();
    r.commit("test edit").unwrap();
    assert!(r.snapshot().unwrap().files.is_empty());
}
#[test]
fn external_edits_and_missing_files_are_protected() {
    let (_dir, r) = fixture();
    let old = "one\ntwo\nthree\n";
    fs::write(r.root.join("file.txt"), "external").unwrap();
    assert!(r.save("file.txt", old, true, "overwrite").is_err());
    assert_eq!(
        fs::read_to_string(r.root.join("file.txt")).unwrap(),
        "external"
    );
    assert!(r.save("new.txt", "", true, "no").is_err());
    r.save("new.txt", "", false, "new").unwrap();
    assert!(r.save("new.txt", "", false, "overwrite").is_err());
}
#[test]
fn parse_renames_spaces_newlines_and_conflicts() {
    let entries =
        parse_status(b"R  new name\0old name\0 M line\nfile\0?? :[literal]\0UU conflict\0")
            .unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].old_path, "old name");
    assert_eq!(entries[1].path, "line\nfile");
    assert!(entries[3].conflict);
}
#[test]
fn untracked_deleted_renamed_and_literal_paths() {
    let (_dir, r) = fixture();
    fs::write(r.root.join(":[x]"), "literal").unwrap();
    r.stage(":[x]").unwrap();
    r.commit("literal").unwrap();
    git(&r.root, &["mv", "file.txt", "new name.txt"]);
    let d = r.document("new name.txt", true).unwrap();
    assert_eq!(d.left, d.right);
    r.unstage("new name.txt").unwrap();
    assert!(!r.root.join("file.txt").exists());
    assert!(r.root.join("new name.txt").exists());
    r.stage("file.txt").unwrap();
    r.stage("new name.txt").unwrap();
    r.commit("rename").unwrap();
    fs::remove_file(r.root.join("new name.txt")).unwrap();
    let d = r.document("new name.txt", false).unwrap();
    assert!(!d.exists);
    assert!(d.right.is_empty());
    r.save(&d.path, &d.right, d.exists, &d.left).unwrap();
    assert!(r.snapshot().unwrap().files.is_empty());
}
#[test]
fn first_commit_unstage_preserves_file() {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(dir.path(), &["config", "core.hooksPath", "/dev/null"]);
    fs::write(dir.path().join("new.txt"), "hello").unwrap();
    let r = Repository::open(dir.path().to_str().unwrap()).unwrap();
    r.stage("new.txt").unwrap();
    r.unstage("new.txt").unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("new.txt")).unwrap(),
        "hello"
    );
    assert_eq!(r.snapshot().unwrap().files[0].index, "?");
}
#[test]
fn conflict_block_resolution_preserves_context_and_diff3() {
    let s =
        "before\n<<<<<<< HEAD\nours\n||||||| base\nold\n=======\ntheirs\n>>>>>>> branch\nafter\n";
    assert_eq!(
        choose_conflict(s, 0, "ours").unwrap(),
        "before\nours\nafter\n"
    );
    assert_eq!(
        choose_conflict(s, 0, "theirs").unwrap(),
        "before\ntheirs\nafter\n"
    );
    assert_eq!(
        choose_conflict(s, 0, "both").unwrap(),
        "before\nours\ntheirs\nafter\n"
    );
    assert!(choose_conflict("<<<<<<< HEAD\nbroken", 0, "ours").is_err());
}
#[test]
fn real_three_way_conflict_and_merge_commit() {
    let (_dir, r) = fixture();
    git(&r.root, &["checkout", "-b", "other"]);
    fs::write(r.root.join("file.txt"), "one\ntheirs\nthree\n").unwrap();
    git(&r.root, &["commit", "-am", "theirs"]);
    git(&r.root, &["checkout", "main"]);
    fs::write(r.root.join("file.txt"), "one\nours\nthree\n").unwrap();
    git(&r.root, &["commit", "-am", "ours"]);
    assert!(run(&r.root, &["merge", "other"], None).is_err());
    let d = r.document("file.txt", false).unwrap();
    assert!(d.conflict);
    assert!(d.base.contains("two"));
    assert!(d.ours.contains("ours"));
    assert!(d.theirs.contains("theirs"));
    assert!(r.stage(&d.path).is_err());
    assert!(r.commit("premature").is_err());
    assert!(r.resolve(&d.path, &d.right, true, &d.right).is_err());
    let result = choose_conflict(&d.right, 0, "both").unwrap();
    r.resolve(&d.path, &d.right, true, &result).unwrap();
    assert!(!r.snapshot().unwrap().files.iter().any(|f| f.conflict));
    r.commit("resolve merge").unwrap();
    assert!(!r.snapshot().unwrap().merging);
}
#[test]
fn conflict_can_resolve_as_deletion() {
    let (_dir, r) = fixture();
    git(&r.root, &["checkout", "-b", "other"]);
    git(&r.root, &["rm", "file.txt"]);
    git(&r.root, &["commit", "-m", "delete"]);
    git(&r.root, &["checkout", "main"]);
    fs::write(r.root.join("file.txt"), "changed\n").unwrap();
    git(&r.root, &["commit", "-am", "edit"]);
    assert!(run(&r.root, &["merge", "other"], None).is_err());
    let d = r.document("file.txt", false).unwrap();
    assert!(d.conflict);
    r.resolve_delete(&d.path, &d.right, d.exists).unwrap();
    assert!(!r.root.join("file.txt").exists());
    r.commit("resolve deletion").unwrap();
}
#[test]
fn binary_and_symlinks_are_not_edited() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("binary"), b"abc\0def").unwrap();
    assert!(!r.document("binary", false).unwrap().editable);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("file.txt", r.root.join("link")).unwrap();
        assert!(!r.document("link", false).unwrap().editable);
        assert!(r.save("link", "", false, "oops").is_err());
    }
    assert!(r.save("../outside", "", false, "oops").is_err());
    assert!(r.save(".git/config", "", false, "oops").is_err());
}
#[test]
fn hunk_restore_handles_crlf_unicode_and_no_final_newline() {
    for (old, new) in [
        ("a\r\nb\r\n", "a\r\n中文🙂\r\n"),
        ("last\n", "last"),
        ("", "added\n"),
        ("removed\n", ""),
    ] {
        let restored = restore_hunk(old, new, 0).unwrap();
        assert_eq!(restored, old);
    }
}
#[test]
fn clone_fetch_pull_push_against_local_remote() {
    let (_dir, r) = fixture();
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare", "-b", "main"]);
    git(
        &r.root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(&r.root, &["push", "-u", "origin", "main"]);
    let clone_parent = TempDir::new().unwrap();
    let clone_path = clone_parent.path().join("clone");
    let other = clone_repository(
        remote.path().to_str().unwrap(),
        clone_path.to_str().unwrap(),
    )
    .unwrap();
    git(&other.root, &["config", "user.name", "Alloy Test"]);
    git(
        &other.root,
        &["config", "user.email", "alloy@example.invalid"],
    );
    fs::write(r.root.join("file.txt"), "updated\n").unwrap();
    r.stage("file.txt").unwrap();
    r.commit("update").unwrap();
    r.network("push").unwrap();
    other.network("fetch").unwrap();
    assert_eq!(other.snapshot().unwrap().behind, 1);
    other.network("pull").unwrap();
    assert_eq!(
        fs::read_to_string(other.root.join("file.txt")).unwrap(),
        "updated\n"
    );
    fs::write(other.root.join("file.txt"), "dirty").unwrap();
    assert!(other.network("pull").is_err());
}

#[test]
fn staged_and_unstaged_edits_remain_separate() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "staged\n").unwrap();
    r.stage("file.txt").unwrap();
    fs::write(r.root.join("file.txt"), "unstaged\n").unwrap();
    let work = r.document("file.txt", false).unwrap();
    let index = r.document("file.txt", true).unwrap();
    assert_eq!(work.left, "staged\n");
    assert_eq!(work.right, "unstaged\n");
    assert_eq!(index.left, "one\ntwo\nthree\n");
    assert_eq!(index.right, "staged\n");
    r.commit("only staged").unwrap();
    assert_eq!(r.document("file.txt", false).unwrap().left, "staged\n");
    assert_eq!(
        fs::read_to_string(r.root.join("file.txt")).unwrap(),
        "unstaged\n"
    );
}
#[cfg(unix)]
#[test]
fn saving_preserves_executable_bit() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, r) = fixture();
    let path = r.root.join("file.txt");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    r.save("file.txt", "one\ntwo\nthree\n", true, "new\n")
        .unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o755
    );
}
