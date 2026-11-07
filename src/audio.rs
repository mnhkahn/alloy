//! Original audio blobs for worktree, index and historical previews.
use serde::{Deserialize, Serialize};

pub const MAX_AUDIO_BYTES: usize = 32 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioPreview {
    pub asset: String,
    pub bytes: usize,
    pub format: String,
    pub error: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioComparison {
    pub left: Option<AudioPreview>,
    pub right: Option<AudioPreview>,
}
pub fn supported_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| {
            matches!(
                v.to_ascii_lowercase().as_str(),
                "mp3"
                    | "wav"
                    | "wave"
                    | "m4a"
                    | "aac"
                    | "aif"
                    | "aiff"
                    | "aifc"
                    | "caf"
                    | "flac"
                    | "mp4"
            )
        })
}
pub fn prepare(path: &str, bytes: &[u8]) -> AudioPreview {
    let result = if bytes.is_empty() {
        Err("Audio file is empty.".into())
    } else if bytes.len() > MAX_AUDIO_BYTES {
        Err("Audio exceeds the 32 MiB preview limit.".into())
    } else {
        crate::preview::cache_audio(bytes)
    };
    let (asset, error) = match result {
        Ok(key) => (key, String::new()),
        Err(error) => (String::new(), error),
    };
    AudioPreview {
        asset,
        error,
        bytes: bytes.len(),
        format: std::path::Path::new(path)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_uppercase(),
    }
}
