# Alloy

A macOS Git workspace built with [gpui-rhai](https://github.com/eddix/gpui-rhai). Inspect complete diffs, edit the working tree, and resolve conflicts in one window.

## Run

Requires macOS, Git, Xcode Command Line Tools, and Rust 1.95 or newer. Keep Alloy and gpui-rhai as sibling directories:

```sh
git clone https://github.com/eddix/gpui-rhai.git
git -C gpui-rhai checkout d87ebb9f781ecaaa53e265d39b9cacc7ded07f25
git clone https://github.com/mnhkahn/alloy.git
cd alloy
sh scripts/prepare-dependency.sh
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

- **Review:** browse changed files, filter filenames, and switch split/unified views. Local changes compares HEAD with the working tree, including staged and unstaged edits together. Use Staging… for the index/HEAD comparison. All context is visible. The native DiffViewer supports syntax highlighting, line numbers, selection, copy, search (`⌘F`), and previous/next difference (`⌥↑`/`⌥↓`).
- **Edit:** open **Edit result**, add/delete/change text, and see the comparison update. `⌘S` writes the draft atomically. Each `»` arrow in the diff gutter restores that block from HEAD into a draft. Save it after reviewing. **Discard draft** never changes disk.
- **Commit:** check files to include, enter a message, and click Commit. File selection and diff navigation are independent; checking a box never changes Git. Changes, Unversioned Files and Merge Conflicts have collapsible groups. Group checkboxes affect the visible, filtered files. A selected-file commit includes the current working-tree content of those files, including both paths of a rename; unchecked staged files remain staged and are excluded. A failed commit may leave checked files staged. Staging… exposes explicit stage/unstage and Commit staged index. Merge commits use Commit merge (staged) after resolving conflicts.
- **Sync:** fetch, pull (merge strategy), and push using the current branch/upstream. Pull requires a clean working tree. Push never force-pushes. A remote/upstream must already be configured for normal push/pull; clone sets it up.
- **Resolve:** compare ours and theirs, inspect the common ancestor, and edit the merge result. Choose ours/theirs/both per conflict block, preserving surrounding text. Whole-file choices explicitly replace the entire draft. **Mark resolved** saves and stages the result after checking for remaining markers. Modify/delete conflicts can explicitly resolve as deletion.

The project dropdown remembers the 30 most recently opened repositories and reopens the last project on launch. Each row shows the branch, changed files awaiting commit, added/modified/deleted files, staged files, conflicts, and commits ahead/behind the configured upstream. Opening the dropdown refreshes local summaries asynchronously; it does not fetch from remotes, so incoming counts reflect the last Fetch. Missing repositories remain visible with an unavailable label. Add project opens a path field or folder picker. Commit messages and filters are retained per project during the session. History is stored in `~/Library/Application Support/Alloy/projects.json`.

`⌥←` / `⌥→` reviews previous/next filtered files. `⌘O` opens a repository, `⌘R` refreshes, `⌘S` saves. Unsaved drafts block file/repository switches, refreshes, staging and network operations. Closing with a draft asks for confirmation. Saving checks whether the disk contents changed since loading; conflicts with another editor keep your draft rather than overwriting it.

## Current boundaries

This is an initial desktop implementation, not a full IntelliJ editor. The comparison surface is read-only; an adjacent native multiline editor edits the result with live diff feedback. It supports selection, clipboard, undo/redo and IME, but wraps lines and uses Tab for focus traversal rather than indentation. Hunk restore updates a draft; staging is per file, while commit inclusion supports individual change blocks.

PNG, JPEG, GIF, WebP, BMP and TIFF images have a dedicated preview with version comparison, dimensions, file size and a 100% view. New images use a single pane. Each image is limited to 32 MiB and 16 megapixels; animated images show a still frame. UTF-8 text up to 750 KB is editable. Binary/non-UTF-8 files, symbolic links, submodules and larger files are explicitly identified rather than showing truncated content; they can still be staged. Unusual non-UTF-8 filenames are reported as unsupported. Conflict marker detection is intentionally conservative. Auto-refresh/file watching, branch management, stash/rebase controls, signed/notarized distribution and GUI credential prompts are not yet implemented. Git operations time out after 120 seconds; refresh after a timeout to inspect actual repository state.

## Structure

- `src/git.rs`: Git adapter, file guards, change blocks and conflict resolution; no UI dependencies.
- `src/bridge.rs`: serialized background capabilities connecting Git to Rhai.
- `ui/main.rhai`: application state, layout, draft lifecycle and actions.
- `src/main.rs`: embedded components, host setup and shortcuts.
- `tests/git_workflows.rs`: real temporary repositories, including local remotes and merge conflicts.

The sibling path dependencies intentionally use your local gpui-rhai checkout. CI pins the revision above; application builds embed all Rhai sources and do not need that checkout at runtime.

## IntelliJ interaction alignment

Reference: [Commit workflow](https://www.jetbrains.com/help/idea/commit-and-push-changes.html), [Diff Viewer](https://www.jetbrains.com/help/idea/differences-viewer.html), [merge conflicts](https://www.jetbrains.com/help/idea/resolve-conflicts.html).

Implemented: native row-aligned restore arrows and per-block commit checkboxes, closable Commit file tabs, a compact diff toolbar with layout and whitespace dropdowns, single-pane green source view for added files, compact grouped file list, separate inclusion checkboxes and file preview, group inclusion/indeterminate state, HEAD versus local comparison by default, selected-file commits, optional explicit index controls, previous/next file navigation.

Remaining differences: editable text directly inside the diff pane; individual-line inclusion and append actions; double-click diff tabs and context menus; keyboard list selection; synchronized three-pane merge editor; automatic save with undo and file watching. The separate result editor remains an interim control. Restore arrows update a draft that must be saved; they do not immediately overwrite disk.

### Partial commits and native controls

In exact comparison mode, the middle gutter provides `»` (restore the HEAD block into the draft) and a checkbox (include that block in the next commit). File/group selection overrides block selection. Mixed file checkboxes indicate a partial selection. Hidden whitespace changes cannot be selected through a filtered comparison: choose **Do not ignore** first. Newly added files remain single-pane and use the whole-file checkbox.

Selected-block commits build a temporary index from HEAD and reconcile the original index under an index lock. Working-tree files are never overwritten. Unchecked staged content is retained; conflicting staged edits are rejected before committing. Partial selection is rejected if the file or HEAD content changed. Whole files may be committed alongside partial files. Merges, renames, deletions, additions and non-text files use whole-file selection. A new partial selection is required after editing its source.

`patches/gpui-rhai-hunk-controls.patch` extends the pinned gpui-rhai checkout with opt-in `hunk_controls`, `show_hunk_controls` and `on_hunk_action`. The controls share the native virtualized row layout and emit source-checked callbacks. `scripts/prepare-dependency.sh` applies this patch idempotently and refuses incompatible changes; the packaging script and CI run it automatically. The sibling gpui-rhai checkout therefore contains a small local modification.
