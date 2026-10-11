//! App-level macOS keys. Native text/diff controls own their editing keys.
// key, action, Rhai handler, visible label, group
pub const SHORTCUTS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "cmd-w",
        "alloy.close_tab",
        "close_active",
        "Close current file",
        "Files",
    ),
    (
        "cmd-shift-]",
        "alloy.next_tab",
        "next_tab",
        "Next editor tab",
        "Files",
    ),
    (
        "cmd-shift-[",
        "alloy.previous_tab",
        "previous_tab",
        "Previous editor tab",
        "Files",
    ),
    (
        "ctrl-tab",
        "alloy.next_tab",
        "next_tab",
        "Next editor tab",
        "Files",
    ),
    (
        "cmd-e",
        "alloy.recent",
        "recent_files",
        "Recent / open files",
        "Files",
    ),
    (
        "cmd-shift-o",
        "alloy.find_file",
        "find_file",
        "Go to changed file",
        "Files",
    ),
    ("cmd-s", "alloy.save", "save", "Save current draft", "Edit"),
    (
        "cmd-o",
        "alloy.open",
        "browse",
        "Open repository (Alloy)",
        "Projects",
    ),
    (
        "cmd-alt-y",
        "alloy.refresh",
        "refresh",
        "Synchronize / refresh repository",
        "VCS",
    ),
    (
        "cmd-k",
        "alloy.ai_commit",
        "ai_commit",
        "AI commit message & commit",
        "VCS",
    ),
    (
        "cmd-0",
        "alloy.commit_focus",
        "focus_commit",
        "Focus commit message",
        "VCS",
    ),
    (
        "cmd-enter",
        "alloy.commit",
        "commit_shortcut",
        "Commit checked files",
        "VCS",
    ),
    (
        "cmd-shift-k",
        "alloy.push_prompt",
        "push_prompt",
        "Push commits…",
        "VCS",
    ),
    (
        "cmd-t",
        "alloy.pull_prompt",
        "pull_prompt",
        "Update project / pull…",
        "VCS",
    ),
    ("ctrl-v", "alloy.vcs", "vcs_actions", "VCS actions", "VCS"),
    (
        "cmd-1",
        "alloy.files_focus",
        "focus_files",
        "Focus changed files",
        "Files",
    ),
    (
        "cmd-9",
        "alloy.files_focus",
        "focus_files",
        "Focus changed files",
        "VCS",
    ),
    (
        "cmd-shift-f12",
        "alloy.sidebar",
        "toggle_sidebar",
        "Show / hide sidebar",
        "View",
    ),
    (
        "shift-escape",
        "alloy.hide_sidebar",
        "hide_sidebar",
        "Hide sidebar",
        "View",
    ),
    (
        "escape",
        "alloy.escape",
        "escape_action",
        "Close popup / focus diff",
        "View",
    ),
    (
        "f4",
        "alloy.edit",
        "edit_focus",
        "Edit current file",
        "Edit",
    ),
    (
        "cmd-d",
        "alloy.diff",
        "focus_diff",
        "Show current diff",
        "View",
    ),
    (
        "cmd-shift-d",
        "alloy.diff_settings",
        "diff_settings",
        "Diff settings",
        "View",
    ),
    (
        "cmd-shift-a",
        "alloy.actions",
        "find_action",
        "Find action / shortcuts",
        "View",
    ),
    (
        "cmd-alt-right",
        "alloy.next_file",
        "next_file",
        "Next changed file (Alloy)",
        "Files",
    ),
    (
        "cmd-alt-left",
        "alloy.previous_file",
        "previous_file",
        "Previous changed file (Alloy)",
        "Files",
    ),
];
pub fn catalog() -> serde_json::Value {
    serde_json::Value::Array(SHORTCUTS.iter().map(|(key,action,handler,label,group)| serde_json::json!({"key":key,"action":action,"handler":handler,"label":label,"group":group})).collect())
}
#[cfg(test)]
mod tests {
    #[test]
    fn bindings_are_unique_and_leave_text_navigation_alone() {
        let mut keys = std::collections::BTreeSet::new();
        for (key, ..) in super::SHORTCUTS {
            assert!(keys.insert(key));
            assert!(!["alt-left", "alt-right", "cmd-r", "cmd-backspace"].contains(key));
        }
    }
}
