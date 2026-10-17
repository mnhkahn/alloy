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
    r.network("push").unwrap();
    assert_eq!(r.snapshot().unwrap().upstream, "origin/main");
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
fn ai_commit_prepare_never_stages_implicitly() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "changed\n").unwrap();
    fs::write(r.root.join("new.txt"), "new\n").unwrap();
    assert!(
        alloy::cmsg::prepare(&r)
            .unwrap_err()
            .contains("Nothing staged")
    );
    assert!(r.staged_names().unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(r.root.join("file.txt")).unwrap(),
        "changed\n"
    );
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
    r.stage("app.log").unwrap();
    let error = alloy::cmsg::prepare(&r).unwrap_err();
    assert!(error.contains("sensitive"), "{error}");
}
#[test]
fn ai_commit_prepare_rejects_clean_tree() {
    let (_dir, r) = fixture();
    let error = alloy::cmsg::prepare(&r).unwrap_err();
    assert!(error.contains("Nothing staged"), "{error}");
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
    assert_eq!(p.base, "v0.0.0");
    assert_eq!(p.versions["s"], "v0.0.1");
    assert_eq!(p.versions["m"], "v0.1.0");
    assert_eq!(p.versions["l"], "v1.0.0");
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
fn tag_custom_prefix_prepare_create_and_push() {
    let (_dir, r) = fixture();
    let bare = TempDir::new().unwrap();
    git(bare.path(), &["init", "--bare", "-b", "main"]);
    git(
        &r.root,
        &["remote", "add", "origin", bare.path().to_str().unwrap()],
    );
    // The base may exist only on the remote.
    git(&r.root, &["tag", "ease-v0.2.1"]);
    git(&r.root, &["push", "origin", "refs/tags/ease-v0.2.1"]);
    git(&r.root, &["tag", "-d", "ease-v0.2.1"]);
    let p = alloy::tag::prepare(&r).unwrap();
    assert_eq!(p.base, "ease-v0.2.1");
    assert_eq!(p.versions["s"], "ease-v0.2.2");
    assert_eq!(p.versions["m"], "ease-v0.3.0");
    assert_eq!(p.versions["l"], "ease-v1.0.0");
    alloy::tag::create(&r, &p.versions["s"], &p.head, true, &p.remote, &p.push_url).unwrap();
    assert_eq!(r.tag_commit("ease-v0.2.2").unwrap(), p.head);
    assert!(
        r.remote_tags(&p.push_url)
            .unwrap()
            .contains(&"ease-v0.2.2".into())
    );
    assert_eq!(alloy::tag::prepare(&r).unwrap().base, "ease-v0.2.2");
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

#[test]
fn checkbox_staging_commits_only_staged_versions() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "staged\n").unwrap();
    fs::write(r.root.join("other.txt"), "excluded\n").unwrap();
    r.stage_paths(&["file.txt".into()], true).unwrap();
    fs::write(r.root.join("file.txt"), "later unstaged\n").unwrap();
    assert_eq!(alloy::cmsg::prepare(&r).unwrap().files, vec!["file.txt"]);
    r.commit("test staged only").unwrap();
    assert_eq!(
        String::from_utf8(run(&r.root, &["show", "HEAD:file.txt"], None).unwrap()).unwrap(),
        "staged\n"
    );
    assert_eq!(
        fs::read_to_string(r.root.join("file.txt")).unwrap(),
        "later unstaged\n"
    );
    assert!(run(&r.root, &["cat-file", "-e", "HEAD:other.txt"], None).is_err());
    r.stage_paths(&["file.txt".into(), "other.txt".into()], true)
        .unwrap();
    r.stage_paths(&["file.txt".into(), "other.txt".into()], false)
        .unwrap();
    assert!(r.staged_names().unwrap().is_empty());
    assert!(r.root.join("other.txt").exists());
}
#[test]
fn block_checkboxes_write_index_preserve_other_entries_and_detect_stale_index() {
    let (_dir, r) = fixture();
    let changed = "ONE\ntwo\nTHREE\n";
    fs::write(r.root.join("file.txt"), changed).unwrap();
    fs::write(r.root.join("other.txt"), "keep staged\n").unwrap();
    r.stage("other.txt").unwrap();
    let d = r.selected_comparison("file.txt", "", false, true).unwrap();
    assert_eq!(d.staged_hunks, Some(vec![]));
    let mut selection = PartialSelection {
        left: d.left.clone(),
        right: d.right.clone(),
        indices: vec![0],
    };
    r.stage_hunks("file.txt", &selection, d.index_source.as_ref().unwrap())
        .unwrap();
    let partial = r.selected_comparison("file.txt", "", false, true).unwrap();
    assert_eq!(partial.staged_hunks, Some(vec![0]));
    assert_eq!(partial.index_source.as_deref(), Some("ONE\ntwo\nthree\n"));
    assert!(
        r.stage_hunks("file.txt", &selection, d.index_source.as_ref().unwrap())
            .is_err()
    );
    selection.indices = vec![0, 1];
    r.stage_hunks(
        "file.txt",
        &selection,
        partial.index_source.as_ref().unwrap(),
    )
    .unwrap();
    let full = r.selected_comparison("file.txt", "", false, true).unwrap();
    assert_eq!(full.staged_hunks, Some(vec![0, 1]));
    selection.indices = vec![];
    r.stage_hunks("file.txt", &selection, full.index_source.as_ref().unwrap())
        .unwrap();
    assert_eq!(r.staged_names().unwrap(), vec!["other.txt"]);
    assert_eq!(
        fs::read_to_string(r.root.join("file.txt")).unwrap(),
        changed
    );
}
#[test]
fn batch_staging_is_atomic_and_unborn_unstage_preserves_modified_file() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "change\n").unwrap();
    assert!(
        r.stage_paths(&["file.txt".into(), "missing".into()], true)
            .is_err()
    );
    assert!(r.staged_names().unwrap().is_empty());
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    let r = Repository::open(dir.path().to_str().unwrap()).unwrap();
    fs::write(r.root.join("new.txt"), "first\n").unwrap();
    r.stage("new.txt").unwrap();
    fs::write(r.root.join("new.txt"), "second\n").unwrap();
    r.unstage("new.txt").unwrap();
    assert!(r.staged_names().unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(r.root.join("new.txt")).unwrap(),
        "second\n"
    );
}
#[test]
fn staged_blocks_reject_unrepresentable_external_index() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "staged only\n").unwrap();
    r.stage("file.txt").unwrap();
    fs::write(r.root.join("file.txt"), "different work\n").unwrap();
    let d = r.selected_comparison("file.txt", "", false, true).unwrap();
    assert!(d.staged_hunks.is_none());
    let selection = PartialSelection {
        left: d.left,
        right: d.right,
        indices: vec![0],
    };
    assert!(
        r.stage_hunks("file.txt", &selection, d.index_source.as_ref().unwrap())
            .is_err()
    );
    assert!(r.staged_diff().unwrap().contains("+staged only"));
}

#[test]
fn first_push_selects_remote_and_sets_tracking() {
    for (names, configured, expected) in [
        (vec!["backup"], None, "backup"),
        (vec!["backup", "origin"], None, "origin"),
        (
            vec!["backup", "origin"],
            Some(("remote.pushDefault", "backup")),
            "backup",
        ),
        (
            vec!["backup", "origin"],
            Some(("branch.main.pushRemote", "backup")),
            "backup",
        ),
    ] {
        let (_dir, r) = fixture();
        let targets: Vec<_> = names
            .iter()
            .map(|name| {
                let remote = TempDir::new().unwrap();
                git(remote.path(), &["init", "--bare", "-b", "main"]);
                git(
                    &r.root,
                    &["remote", "add", name, remote.path().to_str().unwrap()],
                );
                remote
            })
            .collect();
        if let Some((key, value)) = configured {
            git(&r.root, &["config", key, value]);
        }
        r.network("push").unwrap();
        assert_eq!(r.snapshot().unwrap().upstream, format!("{expected}/main"));
        for (name, target) in names.iter().zip(&targets) {
            assert_eq!(
                run(
                    target.path(),
                    &["rev-parse", "--verify", "refs/heads/main"],
                    None
                )
                .is_ok(),
                *name == expected
            );
        }
    }
}

#[test]
fn first_push_rejects_ambiguous_targets_and_detached_head() {
    let (_dir, r) = fixture();
    assert!(r.network("push").unwrap_err().contains("No remote"));
    let target = TempDir::new().unwrap();
    git(target.path(), &["init", "--bare"]);
    for name in ["one", "two"] {
        git(
            &r.root,
            &["remote", "add", name, target.path().to_str().unwrap()],
        );
    }
    assert!(r.network("push").unwrap_err().contains("Multiple remotes"));
    git(&r.root, &["config", "remote.pushDefault", "missing"]);
    assert!(
        r.network("push")
            .unwrap_err()
            .contains("not a named remote")
    );
    git(&r.root, &["config", "remote.pushDefault", "one"]);
    for _ in 0..2 {
        git(
            &r.root,
            &[
                "config",
                "--add",
                "remote.one.pushurl",
                target.path().to_str().unwrap(),
            ],
        );
    }
    assert!(
        r.network("push")
            .unwrap_err()
            .contains("multiple push URLs")
    );
    git(&r.root, &["checkout", "--detach"]);
    assert!(r.network("push").unwrap_err().contains("detached HEAD"));
    assert!(
        run(
            target.path(),
            &["rev-parse", "--verify", "refs/heads/main"],
            None
        )
        .is_err()
    );
}

#[test]
fn first_push_does_not_overwrite_remote_history() {
    let (_dir, r) = fixture();
    let target = TempDir::new().unwrap();
    git(target.path(), &["init", "--bare", "-b", "main"]);
    git(
        &r.root,
        &["remote", "add", "origin", target.path().to_str().unwrap()],
    );
    git(&r.root, &["push", "origin", "main"]);
    let original = r.head_sha().unwrap();
    git(&r.root, &["commit", "--amend", "-m", "different history"]);
    assert!(r.network("push").is_err());
    assert!(r.snapshot().unwrap().upstream.is_empty());
    let tip =
        String::from_utf8(run(target.path(), &["rev-parse", "refs/heads/main"], None).unwrap())
            .unwrap();
    assert_eq!(tip.trim(), original.trim());
}

#[test]
fn status_reads_do_not_rewrite_index_stat_cache() {
    let (_dir, r) = fixture();
    let index_path = r.root.join(".git/index");
    let index = fs::read(&index_path).unwrap();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(r.root.join("file.txt"))
        .unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
        .unwrap();
    assert!(r.snapshot().unwrap().files.is_empty());
    assert_eq!(fs::read(&index_path).unwrap(), index);
    assert!(r.clean().unwrap());
    assert_eq!(fs::read(&index_path).unwrap(), index);
}

#[test]
fn committed_changes_are_selected_again_on_refresh() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "first edit\n").unwrap();
    fs::write(r.root.join("new.txt"), "new\n").unwrap();
    fs::write(r.root.join("staged-new.txt"), "staged new\n").unwrap();
    r.stage("staged-new.txt").unwrap();
    r.stage_committed_changes().unwrap();
    let files = r.snapshot().unwrap().files;
    assert!(
        files
            .iter()
            .any(|f| f.path == "file.txt" && f.index == "M" && f.work == " ")
    );
    assert!(files.iter().any(|f| f.path == "new.txt" && f.index == "?"));
    r.unstage("file.txt").unwrap();
    fs::write(r.root.join("file.txt"), "second edit\n").unwrap();
    r.stage_committed_changes().unwrap();
    assert_eq!(
        run(&r.root, &["show", ":file.txt"], None).unwrap(),
        b"second edit\n"
    );
    r.commit("defaults").unwrap();
    assert!(
        r.snapshot()
            .unwrap()
            .files
            .iter()
            .any(|f| f.path == "new.txt")
    );
}

#[test]
fn rollback_all_preserves_new_files_and_their_index_entries() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("other.txt"), "original\n").unwrap();
    git(&r.root, &["add", "other.txt"]);
    git(&r.root, &["commit", "-m", "other"]);
    fs::write(r.root.join("file.txt"), "staged edit\n").unwrap();
    r.stage("file.txt").unwrap();
    fs::write(r.root.join("file.txt"), "unstaged edit\n").unwrap();
    fs::remove_file(r.root.join("other.txt")).unwrap();
    fs::write(r.root.join("new.txt"), "keep\n").unwrap();
    fs::write(r.root.join("staged-new.txt"), "also keep\n").unwrap();
    r.stage("staged-new.txt").unwrap();
    r.rollback(None).unwrap();
    assert_eq!(
        fs::read(r.root.join("file.txt")).unwrap(),
        b"one\ntwo\nthree\n"
    );
    assert_eq!(fs::read(r.root.join("other.txt")).unwrap(), b"original\n");
    assert_eq!(fs::read(r.root.join("new.txt")).unwrap(), b"keep\n");
    assert_eq!(
        run(&r.root, &["show", ":staged-new.txt"], None).unwrap(),
        b"also keep\n"
    );
    assert_eq!(r.snapshot().unwrap().files.len(), 2);
    assert!(r.rollback(Some("new.txt")).is_err());
    assert!(r.rollback(Some("staged-new.txt")).is_err());
}

#[test]
fn rollback_single_rename_and_literal_paths() {
    let (_dir, r) = fixture();
    let renamed = ":[renamed] file\n.txt";
    git(&r.root, &["mv", "file.txt", renamed]);
    fs::write(r.root.join("new.txt"), "keep").unwrap();
    r.rollback(Some(renamed)).unwrap();
    assert!(!r.root.join(renamed).exists());
    assert_eq!(
        fs::read(r.root.join("file.txt")).unwrap(),
        b"one\ntwo\nthree\n"
    );
    assert_eq!(fs::read(r.root.join("new.txt")).unwrap(), b"keep");
    assert_eq!(r.snapshot().unwrap().files.len(), 1);
}

#[test]
fn defaults_and_rollback_in_unborn_repository_preserve_everything() {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    let r = Repository::open(dir.path().to_str().unwrap()).unwrap();
    fs::write(r.root.join("new.txt"), "keep").unwrap();
    r.stage("new.txt").unwrap();
    r.stage_committed_changes().unwrap();
    r.rollback(None).unwrap();
    assert_eq!(fs::read(r.root.join("new.txt")).unwrap(), b"keep");
    assert_eq!(r.snapshot().unwrap().files[0].index, "A");
}

#[test]
fn single_file_rollback_leaves_other_modifications_alone() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("other.txt"), "original").unwrap();
    git(&r.root, &["add", "other.txt"]);
    git(&r.root, &["commit", "-m", "other"]);
    fs::write(r.root.join("other.txt"), "keep this edit").unwrap();
    fs::remove_file(r.root.join("file.txt")).unwrap();
    r.stage_committed_changes().unwrap();
    assert_eq!(
        r.snapshot()
            .unwrap()
            .files
            .iter()
            .find(|f| f.path == "file.txt")
            .unwrap()
            .index,
        "D"
    );
    r.rollback(Some("file.txt")).unwrap();
    assert_eq!(
        fs::read(r.root.join("file.txt")).unwrap(),
        b"one\ntwo\nthree\n"
    );
    assert_eq!(
        fs::read(r.root.join("other.txt")).unwrap(),
        b"keep this edit"
    );
    assert_eq!(
        run(&r.root, &["show", ":other.txt"], None).unwrap(),
        b"keep this edit"
    );
}

#[test]
fn pull_conflicts_use_git_stages_and_finish_with_a_two_parent_commit() {
    let (_dir, local) = fixture();
    let remote_dir = TempDir::new().unwrap();
    let remote = remote_dir.path().join("remote.git");
    git(
        remote_dir.path(),
        &["init", "--bare", remote.to_str().unwrap()],
    );
    git(
        &local.root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&local.root, &["push", "-u", "origin", "main"]);
    let other = remote_dir.path().join("other");
    git(
        remote_dir.path(),
        &[
            "clone",
            "-b",
            "main",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.name", "Remote Test"]);
    git(&other, &["config", "user.email", "remote@example.invalid"]);
    git(&other, &["config", "commit.gpgsign", "false"]);
    git(&other, &["config", "core.hooksPath", "/dev/null"]);
    fs::write(other.join("file.txt"), "one\nremote\nthree\n").unwrap();
    git(&other, &["commit", "-am", "remote"]);
    git(&other, &["push"]);
    fs::write(local.root.join("file.txt"), "one\nlocal\nthree\n").unwrap();
    git(&local.root, &["commit", "-am", "local"]);
    assert!(local.network("pull").is_err());
    assert!(local.snapshot().unwrap().merging);
    let doc = local.document("file.txt", false).unwrap();
    assert_eq!(doc.base, "one\ntwo\nthree\n");
    assert_eq!(doc.ours, "one\nlocal\nthree\n");
    assert_eq!(doc.theirs, "one\nremote\nthree\n");
    let message = local.merge_message().unwrap();
    assert!(message.starts_with("Merge"));
    assert!(!message.contains("# Conflicts:"));
    let result = choose_conflict(&doc.right, 0, "both").unwrap();
    local.resolve(&doc.path, &doc.right, true, &result).unwrap();
    assert!(!local.snapshot().unwrap().files.iter().any(|f| f.conflict));
    assert!(local.snapshot().unwrap().merging);
    local.commit(&message).unwrap();
    assert!(!local.snapshot().unwrap().merging);
    let log = alloy::history::list(&local, 0).unwrap();
    assert_eq!(log.commits[0].parents.len(), 2);
    assert_eq!(
        fs::read_to_string(local.root.join("file.txt")).unwrap(),
        "one\nlocal\nremote\nthree\n"
    );
}

#[test]
fn abort_merge_restores_local_commit_and_keeps_new_files() {
    let (_dir, r) = fixture();
    git(&r.root, &["checkout", "-b", "incoming"]);
    fs::write(r.root.join("file.txt"), "remote\n").unwrap();
    git(&r.root, &["commit", "-am", "remote"]);
    git(&r.root, &["checkout", "main"]);
    fs::write(r.root.join("file.txt"), "local\n").unwrap();
    git(&r.root, &["commit", "-am", "local"]);
    assert!(run(&r.root, &["merge", "incoming"], None).is_err());
    fs::write(r.root.join("keep-new.txt"), "keep").unwrap();
    r.abort_merge().unwrap();
    assert!(!r.snapshot().unwrap().merging);
    assert_eq!(fs::read(r.root.join("file.txt")).unwrap(), b"local\n");
    assert_eq!(fs::read(r.root.join("keep-new.txt")).unwrap(), b"keep");
    assert!(r.abort_merge().is_err());
}

#[test]
fn upstream_detection_uses_configuration_even_without_a_tracking_ref() {
    let (_dir, r) = fixture();
    assert!(!r.has_upstream().unwrap());
    git(&r.root, &["remote", "add", "origin", "/alloy-test/missing-remote"]);
    git(&r.root, &["config", "branch.main.remote", "origin"]);
    git(&r.root, &["config", "branch.main.merge", "refs/heads/main"]);
    assert!(r.snapshot().unwrap().upstream.is_empty());
    assert!(r.has_upstream().unwrap());
    git(&r.root, &["config", "--unset", "branch.main.merge"]);
    assert!(!r.has_upstream().unwrap());
    git(&r.root, &["checkout", "--detach"]);
    assert!(r.has_upstream().is_err());
}

#[test]
fn delete_file_removes_working_copy_and_preserves_staged_content() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("file.txt"), "staged content\n").unwrap();
    r.stage("file.txt").unwrap();
    let staged = run(&r.root, &["show", ":file.txt"], None).unwrap();
    r.delete_file("file.txt").unwrap();
    assert!(!r.root.join("file.txt").exists());
    assert_eq!(run(&r.root, &["show", ":file.txt"], None).unwrap(), staged);
    let snapshot = r.snapshot().unwrap();
    assert_eq!(snapshot.files[0].work, "D");
    r.stage("file.txt").unwrap();
    assert_eq!(r.snapshot().unwrap().files[0].index, "D");
}

#[test]
fn delete_file_handles_untracked_binary_and_rejects_unsafe_paths() {
    let (_dir, r) = fixture();
    fs::write(r.root.join("new.bin"), [0, 255, 1]).unwrap();
    r.delete_file("new.bin").unwrap();
    assert!(!r.root.join("new.bin").exists());
    assert!(r.snapshot().unwrap().files.is_empty());
    assert!(r.delete_file("file.txt").is_err());
    assert!(r.delete_file("../file.txt").is_err());
    assert!(r.delete_file(".git/config").is_err());
    fs::create_dir(r.root.join("folder")).unwrap();
    fs::write(r.root.join("folder/keep.txt"), "keep").unwrap();
    assert!(r.delete_file("folder").is_err());
    assert!(r.root.join("folder/keep.txt").exists());
    #[cfg(unix)]
    {
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("keep.txt");
        fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, r.root.join("link")).unwrap();
        assert!(r.delete_file("link").is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "keep");
    }
}
