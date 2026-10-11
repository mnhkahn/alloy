//! AI commit messages from the staged diff, mirroring the xiaoli git-cmsg flow:
//! prepare the staged diff, compact it for the model, generate, sanitize, commit.
use crate::git::Repository;
use crate::llm;

const TOTAL_BUDGET: usize = 12 * 1024;
const FILE_LIMIT: usize = 24 * 1024;
const FILE_LINE_LIMIT: usize = 1000;
const FILE_HEAD_LINES: usize = 80;
const FILE_HEAD_BUDGET: usize = 4 * 1024;

pub const SYSTEM_PROMPT: &str = "你是 Git 提交信息助手。只输出中文 Conventional Commits 提交信息，不要解释，不要 Markdown。必须使用以下结构：第一行是 type(scope): 简短中文描述；随后空一行；再用 `- ` 开头的列表逐条说明主要变更。即使变更较小，也至少给出一条列表项。";

#[derive(Debug)]
pub struct PreparedDiff {
    pub files: Vec<String>,
    pub stat: String,
    pub diff: String,
}

/// Read only explicitly staged changes. Never add working-tree files implicitly.
pub fn prepare(repo: &Repository) -> Result<PreparedDiff, String> {
    let files = repo.staged_names()?;
    if files.is_empty() {
        return Err(
            "Nothing staged to commit. Check files or change blocks to stage them first.".into(),
        );
    }
    let blocked: Vec<&str> = files
        .iter()
        .filter(|f| sensitive(f))
        .map(String::as_str)
        .collect();
    if !blocked.is_empty() {
        return Err(format!(
            "Refusing to auto-commit sensitive files: {}. Unstage them first.",
            blocked.join(", ")
        ));
    }
    Ok(PreparedDiff {
        files,
        stat: repo.staged_numstat()?,
        diff: repo.staged_diff()?,
    })
}

/// Filename rules only; file contents are never scanned.
pub fn sensitive(path: &str) -> bool {
    let p = path.to_lowercase();
    p.contains(".env")
        || p.contains("secret")
        || p.contains("/logs/")
        || p.starts_with("logs/")
        || p.ends_with(".pem")
        || p.ends_with(".key")
        || p.ends_with(".log")
}

/// Shrink only what the model sees; the index and commit content stay untouched.
pub fn compact(diff: &str) -> String {
    if diff.len() <= TOTAL_BUDGET {
        return diff.to_string();
    }
    let mut out = String::new();
    for chunk in file_chunks(diff) {
        let chunk = if chunk.len() > FILE_LIMIT || chunk.lines().count() > FILE_LINE_LIMIT {
            let head = chunk
                .lines()
                .take(FILE_HEAD_LINES)
                .collect::<Vec<_>>()
                .join("\n");
            let end = head.floor_char_boundary(FILE_HEAD_BUDGET);
            format!("{}\n… (file diff truncated)\n", &head[..end])
        } else {
            chunk
        };
        if !out.is_empty() && out.len() + chunk.len() > TOTAL_BUDGET {
            out.push_str("… (remaining file diffs omitted)\n");
            break;
        }
        out.push_str(&chunk);
    }
    out
}

fn file_chunks(diff: &str) -> Vec<String> {
    let mut chunks: Vec<String> = vec![];
    for line in diff.split_inclusive('\n') {
        if line.starts_with("diff --git ") || chunks.is_empty() {
            chunks.push(String::new());
        }
        if let Some(chunk) = chunks.last_mut() {
            chunk.push_str(line);
        }
    }
    chunks
}

/// Drop code fences, trim trailing spaces and outer blank lines.
pub fn sanitize(text: &str) -> Result<String, String> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .map(str::trim_end)
        .collect();
    let message = lines.join("\n").trim().to_string();
    if message.is_empty() {
        return Err("Model produced no commit message.".into());
    }
    Ok(message)
}

/// Full Cmd+K flow: staged diff → compact → model → sanitize → commit. No push.
pub fn generate_and_commit(repo: &Repository) -> Result<String, String> {
    let prepared = prepare(repo)?;
    let diff = compact(&prepared.diff);
    let config = llm::load()?;
    let user = format!(
        "请为以下暂存的变更生成提交信息。\n\n文件列表：\n{}\n\n统计：\n{}\n\nDiff：\n{}",
        prepared.files.join("\n"),
        prepared.stat.trim(),
        diff
    );
    let raw = llm::generate(&config, SYSTEM_PROMPT, &user)?;
    let message = sanitize(&raw)?;
    let output = repo.commit(&message)?;
    Ok(format!(
        "{}\n{}",
        message.lines().next().unwrap_or_default(),
        output.trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_names_are_blocked() {
        for path in [
            ".env",
            "app/.env.local",
            "config/SECRET_token.txt",
            "app/logs/today.txt",
            "logs/today.txt",
            "keys/server.pem",
            "id.key",
            "debug.LOG",
        ] {
            assert!(sensitive(path), "{path} should be blocked");
        }
        for path in ["src/main.rs", "docs/environment.md", "logger.rs"] {
            assert!(!sensitive(path), "{path} should pass");
        }
    }

    #[test]
    fn compact_keeps_small_diff() {
        let diff = "diff --git a/a b/a\n+hello\n";
        assert_eq!(compact(diff), diff);
    }

    #[test]
    fn compact_truncates_large_file() {
        let mut diff = String::from("diff --git a/big b/big\n");
        for i in 0..2500 {
            diff.push_str(&format!("+line {i}\n"));
        }
        let out = compact(&diff);
        assert!(out.contains("file diff truncated"));
        assert!(out.len() <= FILE_HEAD_BUDGET + 100);
    }

    #[test]
    fn compact_omits_overflowing_files() {
        let mut diff = String::new();
        for f in 0..5 {
            diff.push_str(&format!("diff --git a/f{f} b/f{f}\n"));
            diff.push_str(&"+".repeat(5 * 1024));
            diff.push('\n');
        }
        let out = compact(&diff);
        assert!(out.contains("remaining file diffs omitted"));
        assert!(out.len() <= TOTAL_BUDGET + 100);
    }

    #[test]
    fn compact_respects_utf8_boundaries() {
        let mut diff = String::from("diff --git a/u b/u\n");
        diff.push_str(&"汉".repeat(10 * 1024));
        let out = compact(&diff);
        assert!(out.contains("file diff truncated"));
    }

    #[test]
    fn sanitize_strips_fences_and_whitespace() {
        let raw = "```\nfeat(ui): 添加快捷键  \n\n- 支持 Cmd+K\n```\n";
        assert_eq!(
            sanitize(raw).unwrap(),
            "feat(ui): 添加快捷键\n\n- 支持 Cmd+K"
        );
        assert!(sanitize("```\n\n```").is_err());
    }
}
