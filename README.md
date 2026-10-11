# Alloy

A macOS Git workspace built with [gpui-rhai](https://github.com/eddix/gpui-rhai). Inspect complete diffs, edit the working tree, and resolve conflicts in one window.

## Run

Requires macOS, Git, Xcode Command Line Tools, and Rust 1.95 or newer. Keep Alloy and gpui-rhai as sibling directories:

```sh
git clone https://github.com/eddix/gpui-rhai.git
git -C gpui-rhai checkout d87ebb9f781ecaaa53e265d39b9cacc7ded07f25
git clone https://github.com/mnhkahn/alloy.git
cd alloy
cargo run -- /absolute/path/to/a/repository
```

Omit the path for the welcome screen. `Browse…` opens the macOS directory picker. To clone, enter a URL and a new absolute destination path. Git credentials use your existing Git configuration; terminal password prompts are disabled. Configure an HTTPS credential helper or SSH agent first.

```sh
cargo test
cargo run -- --check
scripts/package.sh             # dist/Alloy.app (local ad-hoc signature)
scripts/package.sh debug       # faster local development bundle
```

## Workflow

- **Review:** browse changed files, filter filenames, switch working-tree/index and index/HEAD comparisons, or switch split/unified views. All context is visible. The native DiffViewer supports syntax highlighting, line numbers, selection, copy, search (`⌘F`), and previous/next difference (`⌥↑`/`⌥↓`).
- **Edit:** open **Edit result**, add/delete/change text, and see the comparison update. `⌘S` writes the draft atomically. Each `↶ Lines…` button restores one change block from the index into a draft. Save it after reviewing. **Discard draft** never changes disk.
- **Commit:** stage or unstage individual files; enter a message and commit the staged index. Existing staged changes are included, as in Git. Merge commits become available once conflicts are resolved.
- **Sync:** fetch, pull (merge strategy), and push using the current branch/upstream. Pull requires a clean working tree. Push never force-pushes. A remote/upstream must already be configured for normal push/pull; clone sets it up.
- **Resolve:** compare ours and theirs, inspect the common ancestor, and edit the merge result. Choose ours/theirs/both per conflict block, preserving surrounding text. Whole-file choices explicitly replace the entire draft. **Mark resolved** saves and stages the result after checking for remaining markers. Modify/delete conflicts can explicitly resolve as deletion.

`⌘O` opens a repository, `⌘R` refreshes, `⌘S` saves. Unsaved drafts block file/repository switches, refreshes, staging and network operations. Closing with a draft asks for confirmation. Saving checks whether the disk contents changed since loading; conflicts with another editor keep your draft rather than overwriting it.

## Current boundaries

This is an initial desktop implementation, not a full IntelliJ editor. The comparison surface is read-only; an adjacent native multiline editor edits the result with live diff feedback. It supports selection, clipboard, undo/redo and IME, but wraps lines and uses Tab for focus traversal rather than indentation. Hunk restore updates a draft; staging is currently per file.

UTF-8 text up to 750 KB is editable. Binary/non-UTF-8 files, symbolic links, submodules and larger files are explicitly identified rather than showing truncated content; they can still be staged. Unusual non-UTF-8 filenames are reported as unsupported. Conflict marker detection is intentionally conservative. Auto-refresh/file watching, branch management, stash/rebase controls, signed/notarized distribution and GUI credential prompts are not yet implemented. Git operations time out after 120 seconds; refresh after a timeout to inspect actual repository state.

## Structure

- `src/git.rs`: Git adapter, file guards, change blocks and conflict resolution; no UI dependencies.
- `src/bridge.rs`: serialized background capabilities connecting Git to Rhai.
- `ui/main.rhai`: application state, layout, draft lifecycle and actions.
- `src/main.rs`: embedded components, host setup and shortcuts.
- `tests/git_workflows.rs`: real temporary repositories, including local remotes and merge conflicts.

The sibling path dependencies intentionally use your local gpui-rhai checkout. CI pins the revision above; application builds embed all Rhai sources and do not need that checkout at runtime.
