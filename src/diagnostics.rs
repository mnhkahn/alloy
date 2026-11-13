//! Best-effort, bounded local diagnostics. Never log request bodies or credentials.
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
static LOCK: Mutex<()> = Mutex::new(());

pub fn path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".alloy/logs/generation.jsonl"))
}

pub fn event(stage: &str, details: Value) {
    let Some(path) = path() else { return };
    let Ok(_lock) = LOCK.lock() else { return };
    let record = json!({
        "unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        "pid": std::process::id(),
        "thread": format!("{:?}", std::thread::current().id()),
        "stage": stage,
        "details": details,
    });
    if let Err(error) = append(&path, &record) {
        eprintln!("Alloy: could not write generation diagnostics: {error}");
    }
}

fn append(path: &Path, record: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    if fs::metadata(path).is_ok_and(|meta| meta.len() >= MAX_BYTES) {
        fs::rename(path, path.with_extension("jsonl.1"))?;
    }
    let mut file = OpenOptions::new().create(true).append(true).mode(0o600).open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    serde_json::to_writer(&mut file, record)?;
    file.write_all(b"\n")
}

/// Preserve UTF-8 and cap untrusted model output stored in diagnostics.
pub fn excerpt(text: &str) -> &str {
    &text[..text.floor_char_boundary(16 * 1024)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_are_private_valid_json_and_rotate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs/generation.jsonl");
        append(&path, &json!({"output":"异常\n内容"})).unwrap();
        let line = fs::read_to_string(&path).unwrap();
        assert_eq!(line.lines().count(), 1);
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["output"], "异常\n内容");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        OpenOptions::new().write(true).open(&path).unwrap().set_len(MAX_BYTES).unwrap();
        append(&path, &json!({"stage":"next"})).unwrap();
        assert_eq!(fs::metadata(path.with_extension("jsonl.1")).unwrap().len(), MAX_BYTES);
        assert!(fs::metadata(&path).unwrap().len() < 100);
        assert!(excerpt(&"汉".repeat(10000)).len() <= 16 * 1024);
    }
}
