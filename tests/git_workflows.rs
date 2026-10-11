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
    assert_eq!(
        alloy::projects::Overview::from_snapshot(&r.snapshot().unwrap()).ahead,
        1
    );
    r.network("push").unwrap();
    other.network("fetch").unwrap();
    assert_eq!(
        alloy::projects::Overview::from_snapshot(&other.snapshot().unwrap()).behind,
        1
    );
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

#[test]
fn scoped_selection_preserves_renames_and_reads_fresh_content() {
    let (_dir, r) = fixture();
    git(&r.root, &["mv", "file.txt", "renamed.txt"]);
    let d = r
        .selected_document("renamed.txt", "file.txt", true)
        .unwrap();
    assert_eq!(d.left, d.right);
    fs::write(r.root.join("renamed.txt"), "changed again\n").unwrap();
    let d = r
        .selected_document("renamed.txt", "file.txt", false)
        .unwrap();
    assert_eq!(d.right, "changed again\n");
    assert_eq!(d.left, "one\ntwo\nthree\n");
    assert!(r.selected_document("missing.txt", "", false).is_err());
}

#[test]
fn local_review_includes_both_staged_and_unstaged_changes() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "staged\n").unwrap();
    r.stage("file.txt").unwrap();
    fs::write(r.root.join("file.txt"), "working\n").unwrap();
    let d = r.selected_comparison("file.txt", "", false, true).unwrap();
    assert_eq!(d.left, "one\ntwo\nthree\n");
    assert_eq!(d.right, "working\n");
    let d = r.selected_comparison("file.txt", "", true, true).unwrap();
    assert_eq!(d.right, "staged\n");
}

#[test]
fn checked_commit_excludes_unchecked_staged_changes() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "unchecked staged\n").unwrap();
    r.stage("file.txt").unwrap();
    let name = ":[selected] new\nfile.txt";
    fs::write(r.root.join(name), "selected\n").unwrap();
    r.commit_selected("selected only", &[name.into()]).unwrap();
    assert_eq!(
        run(&r.root, &["show", "HEAD:file.txt"], None).unwrap(),
        b"one\ntwo\nthree\n"
    );
    assert_eq!(
        run(&r.root, &["show", ":file.txt"], None).unwrap(),
        b"unchecked staged\n"
    );
    let files = r.snapshot().unwrap().files;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "file.txt");
    assert!(r.commit_selected("empty", &[]).is_err());
    assert!(r.commit_selected("missing", &["missing".into()]).is_err());
}

#[test]
fn checked_commit_handles_rename_and_deletion() {
    let (_dir, r) = fixture();
    git(&r.root, &["mv", "file.txt", "renamed.txt"]);
    r.commit_selected("rename", &["renamed.txt".into()])
        .unwrap();
    assert!(run(&r.root, &["show", "HEAD:file.txt"], None).is_err());
    assert!(r.snapshot().unwrap().files.is_empty());
    fs::remove_file(r.root.join("renamed.txt")).unwrap();
    r.commit_selected("delete", &["renamed.txt".into()])
        .unwrap();
    assert!(r.snapshot().unwrap().files.is_empty());
}

#[test]
fn checked_initial_commit_keeps_other_files_staged() {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(dir.path(), &["config", "core.hooksPath", "/dev/null"]);
    git(dir.path(), &["config", "user.name", "Alloy Test"]);
    git(
        dir.path(),
        &["config", "user.email", "alloy@example.invalid"],
    );
    fs::write(dir.path().join("checked"), "yes").unwrap();
    fs::write(dir.path().join("unchecked"), "later").unwrap();
    git(dir.path(), &["add", "unchecked"]);
    let r = Repository::open(dir.path().to_str().unwrap()).unwrap();
    r.commit_selected("initial selection", &["checked".into()])
        .unwrap();
    assert!(run(&r.root, &["show", "HEAD:unchecked"], None).is_err());
    assert_eq!(r.snapshot().unwrap().files[0].path, "unchecked");
}

#[test]
fn new_file_view_does_not_misclassify_existing_empty_files() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("empty.txt"), "").unwrap();
    r.stage("empty.txt").unwrap();
    r.commit("add empty file").unwrap();
    fs::write(r.root.join("empty.txt"), "now has content\n").unwrap();
    fs::write(r.root.join("new.txt"), "brand new\n").unwrap();
    assert!(
        !r.selected_comparison("empty.txt", "", false, true)
            .unwrap()
            .added
    );
    assert!(
        r.selected_comparison("new.txt", "", false, true)
            .unwrap()
            .added
    );
    r.stage("new.txt").unwrap();
    assert!(
        r.selected_comparison("new.txt", "", true, true)
            .unwrap()
            .added
    );
}

fn block_fixture() -> (TempDir, Repository, String, String) {
    let (dir, r) = fixture();
    let base = "alpha\nkeep 1\nkeep 2\nkeep 3\nomega\n".to_string();
    let working = "ALPHA\nkeep 1\nkeep 2\nkeep 3\nOMEGA\n".to_string();
    fs::write(r.root.join("file.txt"), &base).unwrap();
    r.stage("file.txt").unwrap();
    r.commit("block baseline").unwrap();
    fs::write(r.root.join("file.txt"), &working).unwrap();
    (dir, r, base, working)
}
#[test]
fn partial_commit_keeps_unchecked_working_and_staged_edits() {
    let (_dir, r, base, working) = block_fixture();
    r.stage("file.txt").unwrap();
    fs::write(r.root.join("other.txt"), "unchecked").unwrap();
    r.stage("other.txt").unwrap();
    let partial = std::collections::BTreeMap::from([(
        "file.txt".into(),
        PartialSelection {
            left: base,
            right: working.clone(),
            indices: vec![0],
        },
    )]);
    r.commit_parts("first block", &[], &partial).unwrap();
    assert_eq!(
        run(&r.root, &["show", "HEAD:file.txt"], None).unwrap(),
        b"ALPHA\nkeep 1\nkeep 2\nkeep 3\nomega\n"
    );
    assert_eq!(
        fs::read_to_string(r.root.join("file.txt")).unwrap(),
        working
    );
    assert_eq!(
        run(&r.root, &["show", ":file.txt"], None).unwrap(),
        working.as_bytes()
    );
    assert!(run(&r.root, &["show", "HEAD:other.txt"], None).is_err());
    assert_eq!(
        run(&r.root, &["show", ":other.txt"], None).unwrap(),
        b"unchecked"
    );
    assert!(!r.root.join(".git/index.lock").exists());
}
#[test]
fn partial_commit_mixes_whole_files_without_staging_excluded_blocks() {
    let (_dir, r, base, working) = block_fixture();
    fs::write(r.root.join("whole.txt"), "whole").unwrap();
    let partial = std::collections::BTreeMap::from([(
        "file.txt".into(),
        PartialSelection {
            left: base,
            right: working.clone(),
            indices: vec![1],
        },
    )]);
    r.commit_parts(
        "second block and whole file",
        &["whole.txt".into()],
        &partial,
    )
    .unwrap();
    assert_eq!(
        run(&r.root, &["show", "HEAD:file.txt"], None).unwrap(),
        b"alpha\nkeep 1\nkeep 2\nkeep 3\nOMEGA\n"
    );
    assert_eq!(
        run(&r.root, &["show", "HEAD:whole.txt"], None).unwrap(),
        b"whole"
    );
    assert_eq!(
        fs::read_to_string(r.root.join("file.txt")).unwrap(),
        working
    );
    assert!(
        run(&r.root, &["diff", "--cached"], None)
            .unwrap()
            .is_empty()
    );
}
#[test]
fn stale_partial_selection_does_not_touch_head_index_or_working_tree() {
    let (_dir, r, base, working) = block_fixture();
    let head = run(&r.root, &["rev-parse", "HEAD"], None).unwrap();
    let index = fs::read(r.root.join(".git/index")).unwrap();
    fs::write(r.root.join("file.txt"), "external edit").unwrap();
    let partial = std::collections::BTreeMap::from([(
        "file.txt".into(),
        PartialSelection {
            left: base,
            right: working,
            indices: vec![0],
        },
    )]);
    assert!(r.commit_parts("stale", &[], &partial).is_err());
    assert_eq!(run(&r.root, &["rev-parse", "HEAD"], None).unwrap(), head);
    assert_eq!(fs::read(r.root.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(r.root.join("file.txt")).unwrap(),
        "external edit"
    );
    assert!(!r.root.join(".git/index.lock").exists());
}
#[test]
fn selected_blocks_preserve_crlf_and_missing_final_newline() {
    let base = "first\r\nkeep\r\nlast";
    let working = "FIRST\r\nkeep\r\nLAST";
    assert_eq!(
        selected_hunks(base, working, &[0]).unwrap(),
        "FIRST\r\nkeep\r\nlast"
    );
    assert_eq!(
        selected_hunks(base, working, &[1]).unwrap(),
        "first\r\nkeep\r\nLAST"
    );
    assert!(selected_hunks(base, working, &[99]).is_err());
}

#[test]
fn partial_commit_preserves_disjoint_previously_staged_block() {
    let (_dir, r, base, working) = block_fixture();
    fs::write(
        r.root.join("file.txt"),
        "ALPHA\nkeep 1\nkeep 2\nkeep 3\nomega\n",
    )
    .unwrap();
    r.stage("file.txt").unwrap();
    fs::write(r.root.join("file.txt"), &working).unwrap();
    let partial = std::collections::BTreeMap::from([(
        "file.txt".into(),
        PartialSelection {
            left: base,
            right: working.clone(),
            indices: vec![1],
        },
    )]);
    r.commit_parts("second block", &[], &partial).unwrap();
    assert_eq!(
        run(&r.root, &["show", "HEAD:file.txt"], None).unwrap(),
        b"alpha\nkeep 1\nkeep 2\nkeep 3\nOMEGA\n"
    );
    assert_eq!(
        run(&r.root, &["show", ":file.txt"], None).unwrap(),
        working.as_bytes()
    );
}

#[test]
fn partial_commit_hook_failure_preserves_original_index_and_head() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, r, base, working) = block_fixture();
    let hooks = r.root.join("test-hooks");
    fs::create_dir(&hooks).unwrap();
    fs::write(hooks.join("pre-commit"), "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(hooks.join("pre-commit"), fs::Permissions::from_mode(0o755)).unwrap();
    git(
        &r.root,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    let head = run(&r.root, &["rev-parse", "HEAD"], None).unwrap();
    let index = fs::read(r.root.join(".git/index")).unwrap();
    let partial = std::collections::BTreeMap::from([(
        "file.txt".into(),
        PartialSelection {
            left: base,
            right: working.clone(),
            indices: vec![0],
        },
    )]);
    assert!(r.commit_parts("rejected", &[], &partial).is_err());
    assert_eq!(run(&r.root, &["rev-parse", "HEAD"], None).unwrap(), head);
    assert_eq!(fs::read(r.root.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(r.root.join("file.txt")).unwrap(),
        working
    );
    assert!(!r.root.join(".git/index.lock").exists());
}

#[test]
fn images_bypass_text_limit_and_preserve_version_bytes() {
    let (_dir, r) = fixture();
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(600, 600)
        .write_to(&mut bytes, image::ImageFormat::Bmp)
        .unwrap();
    let original = bytes.into_inner();
    assert!(original.len() > 750_000);
    fs::write(r.root.join("large.bmp"), &original).unwrap();
    let d = r.document("large.bmp", false).unwrap();
    assert!(!d.editable);
    assert!(d.note.is_empty());
    let images = d.images.unwrap();
    assert!(images.left.is_none());
    let preview = images.right.unwrap();
    assert_eq!((preview.width, preview.height), (600, 600));
    assert_eq!(
        alloy::preview::load_asset(preview.asset.strip_prefix("preview/").unwrap())
            .unwrap()
            .1,
        original
    );
    git(&r.root, &["add", "large.bmp"]);
    git(&r.root, &["commit", "-m", "image"]);
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(20, 30)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    fs::write(r.root.join("large.bmp"), bytes.get_ref()).unwrap();
    let images = r.document("large.bmp", false).unwrap().images.unwrap();
    assert_eq!(images.left.unwrap().width, 600);
    assert_eq!(images.right.unwrap().width, 20);
    git(&r.root, &["add", "large.bmp"]);
    fs::write(r.root.join("large.bmp"), b"corrupt").unwrap();
    assert_eq!(
        r.document("large.bmp", true)
            .unwrap()
            .images
            .unwrap()
            .right
            .unwrap()
            .width,
        20
    );
    assert!(
        !r.document("large.bmp", false)
            .unwrap()
            .images
            .unwrap()
            .right
            .unwrap()
            .error
            .is_empty()
    );
    git(&r.root, &["rm", "-f", "large.bmp"]);
    fs::write(r.root.join("large.bmp"), &original).unwrap();
    assert!(
        r.document("large.bmp", true)
            .unwrap()
            .images
            .unwrap()
            .right
            .is_none()
    );
    fs::write(r.root.join("no-extension"), &original).unwrap();
    assert!(r.document("no-extension", false).unwrap().images.is_some());
    fs::write(r.root.join("large.txt"), vec![b'x'; 750_001]).unwrap();
    assert!(
        r.document("large.txt", false)
            .unwrap()
            .note
            .contains("750 KB")
    );
}

#[test]
fn project_overview_counts_files_once_across_index_and_worktree() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "staged\n").unwrap();
    r.stage("file.txt").unwrap();
    fs::write(r.root.join("file.txt"), "unstaged too\n").unwrap();
    fs::write(r.root.join("new.txt"), "new\n").unwrap();
    let s = alloy::projects::Overview::from_snapshot(&r.snapshot().unwrap());
    assert_eq!(
        (s.files, s.staged, s.added, s.modified, s.deleted),
        (2, 1, 1, 1, 0)
    );
    assert!(s.upstream.is_empty());
    fs::remove_file(r.root.join("file.txt")).unwrap();
    let s = alloy::projects::Overview::from_snapshot(&r.snapshot().unwrap());
    assert_eq!(
        (s.files, s.staged, s.added, s.modified, s.deleted),
        (2, 1, 1, 0, 1)
    );
}

#[test]
fn ai_commit_prepare_stages_everything() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "one\nchanged\nthree\n").unwrap();
    fs::write(r.root.join("new.txt"), "fresh\n").unwrap();
    let prepared = alloy::cmsg::prepare(&r).unwrap();
    assert!(prepared.files.contains(&"file.txt".to_string()));
    assert!(prepared.files.contains(&"new.txt".to_string()));
    assert!(prepared.diff.contains("diff --git"));
    assert!(prepared.stat.contains("file.txt"));
    assert!(r.snapshot().unwrap().files.iter().all(|f| f.work == " "));
}
#[test]
fn ai_commit_prepare_uses_existing_index_only() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "staged\n").unwrap();
    r.stage("file.txt").unwrap();
    fs::write(r.root.join("other.txt"), "left out\n").unwrap();
    let prepared = alloy::cmsg::prepare(&r).unwrap();
    assert_eq!(prepared.files, vec!["file.txt"]);
    let s = r.snapshot().unwrap();
    assert!(
        s.files
            .iter()
            .any(|f| f.path == "other.txt" && f.index == "?")
    );
}
#[test]
fn ai_commit_prepare_blocks_sensitive_files() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("app.log"), "x\n").unwrap();
    let error = alloy::cmsg::prepare(&r).unwrap_err();
    assert!(error.contains("sensitive"), "{error}");
}
#[test]
fn ai_commit_prepare_rejects_clean_tree() {
    let (_dir, r) = fixture();
    let error = alloy::cmsg::prepare(&r).unwrap_err();
    assert!(error.contains("Nothing to commit"), "{error}");
}
#[test]
fn staged_diff_works_before_first_commit() {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    let repo = Repository::open(dir.path().to_str().unwrap()).unwrap();
    repo.stage_all().unwrap();
    assert_eq!(repo.staged_names().unwrap(), vec!["a.txt"]);
    assert!(repo.staged_diff().unwrap().contains("+hello"));
    assert!(repo.staged_numstat().unwrap().contains("a.txt"));
}

#[test]
fn tag_prepare_requires_clean_tree() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("untracked.txt"), "x\n").unwrap();
    let error = alloy::tag::prepare(&r).unwrap_err();
    assert!(error.contains("clean"), "{error}");
}
#[test]
fn tag_prepare_and_local_create() {
    let (_dir, r) = fixture();
    let p = alloy::tag::prepare(&r).unwrap();
    assert_eq!(p.base, "v0.0.0");
    assert_eq!(p.versions["s"], "v0.0.1");
    assert_eq!(p.versions["m"], "v0.1.0");
    assert_eq!(p.versions["l"], "v1.0.0");
    assert_eq!(p.branch, "main");
    assert_eq!(p.head, r.head_sha().unwrap());
    assert!(p.remote.is_empty());
    let out = alloy::tag::create(&r, &p.versions["m"], &p.head, false, "", "").unwrap();
    assert!(out.contains("v0.1.0"), "{out}");
    assert_eq!(r.tag_commit("v0.1.0").unwrap(), p.head);
    // Next round counts the tag left behind into the version base.
    let p2 = alloy::tag::prepare(&r).unwrap();
    assert_eq!(p2.base, "v0.1.0");
    assert_eq!(p2.versions["s"], "v0.1.1");
}
#[test]
fn tag_create_rejects_moved_head() {
    let (_dir, r) = fixture();
    let p = alloy::tag::prepare(&r).unwrap();
    fs::write(r.root.join("file.txt"), "new\n").unwrap();
    git(&r.root, &["add", "."]);
    git(&r.root, &["commit", "-m", "moved"]);
    let error = alloy::tag::create(&r, &p.versions["s"], &p.head, false, "", "");
    assert!(matches!(error, Err(alloy::tag::TagError::Plain(_))));
    assert!(r.local_tags().unwrap().is_empty());
}
#[test]
fn tag_create_and_push_roundtrip() {
    let (_dir, r) = fixture();
    let bare = TempDir::new().unwrap();
    git(bare.path(), &["init", "--bare", "-b", "main"]);
    git(
        &r.root,
        &["remote", "add", "origin", bare.path().to_str().unwrap()],
    );
    let p = alloy::tag::prepare(&r).unwrap();
    assert_eq!(p.remote, "origin");
    assert!(!p.push_url.is_empty());
    let out =
        alloy::tag::create(&r, &p.versions["l"], &p.head, true, "origin", &p.push_url).unwrap();
    assert!(out.contains("pushed"), "{out}");
    assert!(
        r.remote_tags(&p.push_url)
            .unwrap()
            .contains(&"v1.0.0".to_string())
    );
    // Retry on an already-pushed tag is a no-op success path.
    let again = alloy::tag::retry_push(&r, "v1.0.0", &p.head, "origin", &p.push_url).unwrap();
    assert!(again.contains("v1.0.0"), "{again}");
    // Push failure keeps the local tag and reports retry state.
    git(
        &r.root,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            "/nonexistent/remote.git",
        ],
    );
    let failed = alloy::tag::retry_push(&r, "v1.0.0", &p.head, "origin", "/nonexistent/remote.git");
    match failed {
        Err(alloy::tag::TagError::PushFailed {
            version, commit, ..
        }) => {
            assert_eq!(version, "v1.0.0");
            assert_eq!(commit, p.head);
        }
        other => panic!("expected PushFailed, got {other:?}"),
    }
    assert_eq!(r.tag_commit("v1.0.0").unwrap(), p.head);
}
#[test]
fn tag_prefix_of_highest_version_wins() {
    let (_dir, r) = fixture();
    git(&r.root, &["tag", "1.5.0"]);
    git(&r.root, &["tag", "V1.9.0"]);
    git(&r.root, &["tag", "v1.2.0"]);
    let p = alloy::tag::prepare(&r).unwrap();
    assert_eq!(p.base, "V1.9.0");
    assert_eq!(p.versions["l"], "V2.0.0");
}
