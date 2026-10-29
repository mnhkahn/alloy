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

## Release build cache

The `Warm macOS release cache` workflow builds optimized dependencies for Apple Silicon and Intel on each default-branch push, or when run manually on that branch. It uses the same sibling checkout layout, pinned `gpui-rhai` revision, patches, Rust toolchain, macOS deployment target and architecture-specific shared cache keys as the Release workflow. Release tag builds restore this cache without saving another tag-scoped copy.

After enabling this workflow, let both architecture jobs finish on the default branch before pushing the next release tag. The first warm-up is a full build and adds default-branch CI usage. Later releases can reuse compiled dependencies; Alloy itself still recompiles, and dependency or toolchain changes can require rebuilding dependencies. Release optimization settings remain unchanged. Actual release timings depend on cache hits and runner performance.

## Automatic updates

Packaged Alloy apps embed Sparkle 2.10.0, a free macOS updater. The macOS **Alloy → 检查更新…** menu item checks manually; automatic checks run hourly and download signed updates in the background. Sparkle installs to the existing app location when you quit, with macOS authorization if that location requires it. A requested update restart waits while a draft is unsaved, Git/ACP work is in flight, a confirmation is open, or a merge is unfinished. Native update dialogs provide installation and download errors. Development launches without an app bundle disable the update menu item. The menu item is also disabled while work would block an update restart.

The Release workflow builds architecture-specific ZIP archives plus `appcast-arm64.xml` and `appcast-x86_64.xml`, hosted at GitHub's latest Release asset URLs. The application embeds `assets/update-public-key.txt`. Update archives and appcasts both require Ed25519 signatures and verification before extraction; the private key is held in the repository's `SPARKLE_PRIVATE_KEY` Actions secret. Preserve that key across releases. Locally, the signing key is stored in the login Keychain under Sparkle account `com.mnhkahn.alloy`. Never commit an exported private key.

One initial installation of a version containing the updater is required. Older releases without an appcast cannot supply automatic updates; the next release containing these changes starts the feed. Manual checks report a missing release appcast explicitly instead of showing Sparkle's generic retrieval error. Release tags must increase the bundle version. Sparkle's SDK download is pinned by SHA-256 in `scripts/prepare-sparkle.sh`; its license is bundled in the app.

Sparkle does not require a paid Apple developer account. Public releases currently use an ad-hoc macOS code signature. Removing Apple's unidentified-developer/notarization warnings reliably requires Developer ID signing and Apple notarization; the updater does not disable Gatekeeper or remove quarantine attributes.

## Workflow

- **Review:** diff views show the full file path above the content, with a hover tooltip for truncated paths and old → new paths for renames. Browse changed files, filter filenames, and switch split/unified views. Local changes compares HEAD with the working tree, including staged and unstaged edits together. Use Staging… for the index/HEAD comparison. All context is visible. The native DiffViewer supports syntax highlighting, line numbers, selection, copy, search (`⌘F`), and previous/next difference (`⌥↑`/`⌥↓`).
- **Edit:** open **Edit result**, add/delete/change text, and see the comparison update. `⌘S` writes the draft atomically. Each `»` arrow in the diff gutter restores that block from HEAD into a draft. Save it after reviewing. **Discard draft** never changes disk.
- **Commit:** opening a repository and manual Refresh automatically stage changes to files present in HEAD, so they are checked by default. New files remain unselected unless explicitly staged; automatic filesystem refresh preserves manual staging and partial selections. Checking a file stages it in Git immediately; unchecking unstages it without changing its working-tree contents. Checked means fully staged, a dash means staged and unstaged changes coexist. Group checkboxes stage/unstage the visible filtered files atomically. Ordinary Commit and ⌘K both commit only the current index. ⌘K generates its message through the selected ACP agent or directly registered model. The **Commit ⌘K** button and agent dropdown sit below the commit controls. Clicking it or pressing ⌘K generates a message with the selected provider and commits the staged index. While the AI commit runs, files and diff tabs remain browsable; staging and other write actions stay disabled. The read-only Commit message box displays the complete generated subject and body, retaining it after completion; the bottom bar shows only execution status. The top-right repository buttons display their keyboard shortcuts. Choose **手动注册** at the bottom of the dropdown to register a name and launch command, such as `opencode acp` or `"/path with spaces/agent" acp`. Command arguments and quoted paths are supported; existing executable-path configurations remain compatible. The default entry is `/Users/mnhkahn/go/bin/xiaoli`. Each Xiaoli agent continues to use its own model configuration. The same registration page also offers **直连模型**: enter a display name, an OpenAI-compatible Base URL (including `/v1`, or the full `/chat/completions` endpoint), model ID and optional API key, then register. Direct models appear alongside ACP agents in the Commit dropdown. Existing models can be edited or removed; leaving the key blank during an update keeps the saved key. All configuration is entered in the UI and saved automatically; Alloy no longer reads Xiaoli model configuration files for direct generation. Keys stay in local backend preferences and are omitted from settings responses to the UI. An empty index stops with a prompt to stage files, never `git add .`. Staging… exposes explicit index controls. Merge commits use Commit merge (staged).
- **Keep awake:** the compact checkbox immediately left of Log remembers its last checked state across launches (off by default), saved in `~/.alloy/preferences.json`. The adjacent count shows working agents, including multiple agents in one project; click the label/count to open a live list with exactly three labeled fields: Agent, task title, and project path. Missing fields stay blank; terminal executable names are never substituted for task titles. The checkbox only controls sleep prevention, so the count/list also work when unchecked. Monitoring reads live local Codex logs and thread titles plus Herdr agent states across workspaces and named sessions. A Codex CLI hosted by Herdr is counted once when its session ID is available. While checked, working agents hold both macOS system and display idle-sleep assertions independently of the UI, preventing idle display sleep and the automatic lock that follows it. Manual screen lock remains available. Finishing all tasks, unchecking the box, or quitting Alloy releases it. Detection runs about every three seconds; this does not prevent lid-close or manual sleep.
- **Log:** the Log button to the left of the top-right Refresh opens current-branch local history in the same window. Select a commit to see its full message, author, timestamp, hash and changed files; double-click a file (or select it and choose Show Diff / press Enter) to open a read-only parent-versus-commit diff above the Log panel. Merge commits compare with the first parent, and initial commits compare with an empty tree. Added/deleted/renamed files and image previews are supported. Load more commits pages through the complete history. Refresh reloads the Log while it is open. Local Changes (or Log again) returns to the preserved working-tree view.
- **Open / Open Folder:** right-click a changed file to open its working-tree copy in the system default app, or open its containing folder in Finder.
- **Delete:** right-click a changed file and choose **Delete**, then confirm to remove it from disk. The index is preserved; stage tracked deletions with the checkbox. Deleting an untracked file removes it from the change list. Delete is unavailable during a merge and does not remove directories or submodules.

- **Rollback:** right-click a committed file and choose **Rollback**, or use **Rollback all**, then confirm to restore working-tree and index changes to HEAD. Rollback all includes files hidden by the filter and preserves new files, including staged additions. Rollback is unavailable during a merge.
- **Sync:** fetch, pull (merge strategy), and push using the current branch/upstream. Pull requires a clean working tree. Push never force-pushes. With an upstream configured, Push runs immediately without confirmation. Only a branch with no configured upstream prompts before its first push and upstream setup; this checks Git configuration rather than cached UI state. On the first push, Alloy sets the upstream automatically: explicit branch push remote / default push remote / branch remote first, otherwise origin or the only remote. Ambiguous remotes and multiple push URLs are rejected. Pull still requires an upstream; clone sets it up.
- **Resolve:** Pull uses Git merge. Conflicts appear first in the file list and open a three-column view: local (index stage 2), editable final result, incoming (stage 3). Git supplies the auto-merged result and conflict blocks using the common ancestor; Base opens that ancestor for reference. Choose ours/theirs/both per conflict block, preserving surrounding text. Whole-file choices explicitly replace the entire draft. **Mark resolved** saves and stages the result after checking for remaining markers. Modify/delete conflicts can explicitly resolve as deletion. Mark resolved advances to the next conflict. Once all conflicts are resolved, review the editable Git merge message and choose Commit merge to finish the two-parent merge. Abort merge requires confirmation and runs git merge --abort.

The bottom-right status bar shows one compact row: Δ uncommitted files, + added, ~ modified, − deleted, ✓ staged, ! conflicts, ↑ commits to push and ↓ commits to pull. The project dropdown uses the same metrics and highlighting: nonzero counts appear as bold colored badges, zero counts stay muted. Uncommitted totals/pull use yellow, added/staged use green, modified/push use blue, and deleted/conflicts use red. Hover over each metric for its meaning; ⇅ — indicates no upstream. It updates with repository refreshes, even when the project dropdown is closed. Remote counts use the locally known upstream state; Fetch updates them. Repositories without an upstream show that explicitly.

The project dropdown remembers the 30 most recently opened repositories and reopens the last project on launch. Each row shows the branch, changed files awaiting commit, added/modified/deleted files, staged files, conflicts, and commits ahead/behind the configured upstream. All recent repositories refresh their local summaries in the background about every 9 seconds, even when unopened, while editing drafts or running Git/ACP operations. Opening the dropdown displays cached summaries immediately without starting another scan. Compact project statistics use plain styled labels to keep scrolling responsive; the status bar retains detailed hover hints. Summary reads do not stage files or fetch remotes; incoming counts reflect the last Fetch. Missing repositories remain visible with an unavailable label. Repositories with more than 200 changed files render only visible file rows, so large change lists do not block project switching. Add project opens a path field or folder picker. Commit messages and filters are retained per project during the session. Project history and ACP agent settings are stored in `~/.alloy/projects.json`. On first launch, existing settings from `~/Library/Application Support/Alloy/projects.json` are copied into the new location if it does not exist; the original is retained as a backup.

`⌥←` / `⌥→` reviews previous/next filtered files. `⌘O` opens a repository, `⌘R` refreshes, `⌘S` saves. Unsaved drafts block file/repository switches, refreshes, staging and network operations. `⌘Q` quits Alloy; `⌘W` closes the current file tab. Quitting with a draft or an in-flight Git operation asks for confirmation. Saving checks whether the disk contents changed since loading; conflicts with another editor keep your draft rather than overwriting it.

## Current boundaries

This is an initial desktop implementation, not a full IntelliJ editor. The comparison surface is read-only; an adjacent native multiline editor edits the result with live diff feedback. It supports selection, clipboard, undo/redo and IME, but wraps lines and uses Tab for focus traversal rather than indentation. Hunk restore updates a draft; file and change-block checkboxes update the actual Git index.

PNG, JPEG, GIF, WebP, BMP, TIFF and ICO images have a dedicated preview with version comparison, dimensions, file size and a 100% view. New images use a single pane. Each image is limited to 32 MiB and 16 megapixels; animated images show a still frame. UTF-8 text up to 750 KB is editable. Binary/non-UTF-8 files, symbolic links, submodules and larger files are explicitly identified rather than showing truncated content; they can still be staged. Unusual non-UTF-8 filenames are reported as unsupported. Conflict marker detection is intentionally conservative. Auto-refresh/file watching, branch management, stash/rebase controls, signed/notarized distribution and GUI credential prompts are not yet implemented. Git operations time out after 120 seconds; refresh after a timeout to inspect actual repository state.

## Structure

- `src/git.rs`: Git adapter, file guards, change blocks and conflict resolution; no UI dependencies.
- `src/bridge.rs`: serialized background capabilities connecting Git to Rhai.
- `ui/main.rhai`: application state, layout, draft lifecycle and actions.
- `src/main.rs`: embedded components, host setup and shortcuts.
- `tests/git_workflows.rs`: real temporary repositories, including local remotes and merge conflicts.

The sibling path dependencies intentionally use your local gpui-rhai checkout. CI pins the revision above; application builds embed all Rhai sources and do not need that checkout at runtime.

## IntelliJ interaction alignment

Reference: [Commit workflow](https://www.jetbrains.com/help/idea/commit-and-push-changes.html), [Diff Viewer](https://www.jetbrains.com/help/idea/differences-viewer.html), [merge conflicts](https://www.jetbrains.com/help/idea/resolve-conflicts.html).

Implemented: native row-aligned restore arrows and per-block staging checkboxes, closable Commit file tabs, a compact diff toolbar with layout and whitespace dropdowns, single-pane green source view for added files, compact grouped file list, Git staging checkboxes and file preview, group inclusion/indeterminate state, HEAD versus local comparison by default, staged-only commits, optional explicit index controls, previous/next file navigation.

Remaining differences: editable text directly inside the diff pane; individual-line inclusion and append actions; double-click diff tabs; keyboard list selection; synchronized scrolling between merge panes; automatic save with undo and file watching. The separate result editor remains an interim control. Restore arrows update a draft that must be saved; they do not immediately overwrite disk.

### Partial commits and native controls

In exact comparison mode, the middle gutter provides `»` (restore the HEAD block into the draft) and a checkbox (stage/unstage that block immediately). File/group checkboxes stage complete working-tree changes; mixed file checkboxes indicate staged and unstaged changes. Hidden whitespace changes cannot be selected through a filtered comparison: choose **Do not ignore** first. Newly added files use the whole-file checkbox.

Block staging updates a temporary copy of the index under Git’s index lock, then publishes it atomically. Working-tree files and unrelated staged entries are preserved. Refreshes reconstruct block checkbox states from HEAD, index and working-tree contents. Stale selections and staged content not representable by the visible blocks are rejected; use the whole-file checkbox in those cases. Additions, deletions, renames and non-text files use whole-file staging.

`patches/gpui-rhai-hunk-controls.patch` extends the pinned gpui-rhai checkout with opt-in `hunk_controls`, `show_hunk_controls` and `on_hunk_action`. The controls share the native virtualized row layout and emit source-checked callbacks. `patches/gpui-rhai-menu-focus.patch` keeps virtual menu labels from sharing their container’s native focus handle, preventing an accessibility crash when selecting an ACP agent. The same document patch adds native mouse and keyboard text selection to diff/code viewers. `scripts/prepare-dependency.sh` applies these patches idempotently and refuses incompatible changes; the packaging script and CI run it automatically. The sibling gpui-rhai checkout therefore contains a small local modification.

Toolbar shortcuts (also printed on the buttons): Refresh `⌘⌥Y`, Fetch `⌘⌥F`, Pull `⌘T`, Push `⌘⇧K`, Tag `⌘⌥T`. Pull/Push open the same confirmation from either the button or keyboard; Tag opens the version-selection flow. It uses the highest stable numeric version across local and remote tags and preserves its prefix, including custom prefixes made of ASCII letters, digits, hyphens and underscores (for example `ease-v0.2.1` → `ease-v0.2.2` for s, `ease-v0.3.0` for m, or `ease-v1.0.0` for l). Equal versions prefer a custom prefix, then `v`, `V`, and no prefix; multiple custom prefixes use lexical order to break ties. Fetch and Tag use Alloy-specific bindings. Shortcut labels and bindings share one catalog; `⌘⇧A` lists the actions.

## Release packages

`.github/workflows/release.yml` builds optimized macOS apps for Apple Silicon (`arm64`) and Intel (`x86_64`), then attaches ZIP archives and SHA-256 checksum files to a GitHub Release. Both architecture packages must build successfully before publishing assets. The workflow only builds, packages and publishes; it does not run tests, formatting checks, Clippy or UI checks.

Commit and push the workflow and packaging scripts first. Pushing a stable version tag (`v1.2.3`, `V1.2.3` or `1.2.3`) triggers packaging and creates a Release with generated notes if needed. Ordinary branch pushes, pull requests and manual Release publication do not trigger any workflow. For retries, use **Re-run jobs** on the existing tag workflow run. Reruns replace that tag's assets. No extra token is needed: publishing uses the workflow's `GITHUB_TOKEN` with `contents: write`.

The app's bundle version comes from the tag. Local packaging defaults to the Cargo version, or accepts an override: `sh scripts/package.sh release v1.2.3`. Assets are named `Alloy-1.2.3-macOS-arm64.zip` and `Alloy-1.2.3-macOS-x86_64.zip`. The bundled app uses an ad-hoc signature; Developer ID signing and Apple notarization are not configured.

Git network operations use macOS manual HTTP/HTTPS system proxies when no explicit proxy environment variable is set. Git proxy configuration continues to take precedence. Automatic proxy scripts (PAC) are not evaluated. An empty remote tag list starts version selection at `v0.0.0`; a failed remote lookup blocks tagging and reports a network/authentication error.

Diff views hide unchanged sections by default, keeping three context lines around each change. Use **Show all lines / Hide unchanged** in the diff toolbar to toggle the full file; click an “unchanged lines” separator to expand one folded section. This changes only the display, not staging or file contents.

The active repository refreshes automatically from native filesystem events, including worktree files, the Git index, HEAD and refs (also with external Git directories/worktrees). Changes are coalesced before reading Git state; idle ticks only check the event counter. Refresh results are deferred while editing unsaved drafts or confirming an operation, and discarded after switching files/projects or starting another operation. Manual Refresh remains available. This does not automatically fetch from remotes.

### Diff text interactions

Click inside a pane to focus it. Selections and copies use that pane's original text, including tabs and line endings; diff markers and line numbers are excluded. These interactions also work in historical diffs and merge comparison panes.

| Interaction | Behavior |
| --- | --- |
| Drag | Select text across lines within one pane |
| Double-click / double-click and drag | Select a Unicode word / extend by words (including code identifiers such as `foo_bar`) |
| Triple-click / triple-click and drag | Select a complete source line including its newline / extend by lines |
| Click a line number | Select the complete source line |
| Shift-click | Extend the current selection to the clicked position in the same pane |
| ⌘A / ⌘C | Select the whole active pane (including folded text) / copy selection; before a pane is clicked, ⌘A defaults to the right pane |
| ← / →, ↑ / ↓ | Move the selection endpoint by character or source line; Shift extends the selection |
| ⌥← / ⌥→ | Move by word; Shift extends the selection |
| ⌘← / ⌘→ | Move to source line start/end; Shift extends the selection |
| ⌘↑ / ⌘↓, Home / End | Move to document start/end; Shift extends the selection |
| ⌘F | Open search |
| ⌘G / ⌘⇧G | Next / previous search match |
| Escape | Close search |
| ⌥↑ / ⌥↓ | Previous / next change block |
| ⌘⇧E / ⌘⇧C | Expand / collapse all unchanged sections |
| ⌘⌥C | Copy a unified diff patch |
| ⌘-click / Enter | Emit source-location activation for consumers that handle it |

While the diff has focus, ⌥←/→ moves within text; the app's previous/next file shortcut remains available outside the native viewer. Keyboard movement reveals folded target lines. Drag selection currently requires the pointer to remain over rendered text; dragging beyond the viewport does not auto-scroll. Rectangular and multiple disjoint selections are not implemented.

In split diffs, horizontal gestures work across the full content viewport, including blank space below short files. Both panes share the horizontal offset and use the wider document for their scroll range. Vertical scrolling remains owned by the virtual list.

The Commit message is cleared as soon as file/group/block staging is changed. Refresh and filesystem updates also clear it when the staged file selection changes; browsing diffs and changing the filename filter preserve it. Returning to a project restores its message only if its staged selection still matches the saved session. Merge messages retain their existing edit workflow.

### Direct model protocols

Model registration includes a protocol selector: OpenAI Chat Completions (the default for existing configurations), OpenAI Responses, or Anthropic Messages. Both a server root such as `http://127.0.0.1:8790` and an API base such as `http://127.0.0.1:8790/v1` are accepted; server roots receive `/v1` automatically. Full endpoints are also accepted. Custom API prefixes are preserved.

For the local 8790 service, choose Anthropic Messages or OpenAI Responses. Editing a saved model restores its protocol; an empty API Key preserves the saved key.
