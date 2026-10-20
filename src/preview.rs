//! Repository images never travel through Rhai strings or the text editor.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Mutex, OnceLock},
};

pub const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_777_216;
const CACHE_BYTES: usize = 128 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImagePreview {
    pub asset: String,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
    pub format: String,
    pub error: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageComparison {
    pub left: Option<ImagePreview>,
    pub right: Option<ImagePreview>,
}
#[derive(Clone)]
struct Cached {
    key: String,
    mime: String,
    bytes: Vec<u8>,
}
static CACHE: OnceLock<Mutex<VecDeque<Cached>>> = OnceLock::new();
fn cache() -> &'static Mutex<VecDeque<Cached>> {
    CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub fn supported_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| {
            matches!(
                v.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico"
            )
        })
}
pub fn has_image_signature(bytes: &[u8]) -> bool {
    image::guess_format(bytes).is_ok()
}
pub fn load_asset(key: &str) -> Result<(String, Vec<u8>), String> {
    let mut cache = cache().lock().map_err(|e| e.to_string())?;
    let index = cache
        .iter()
        .position(|v| v.key == key)
        .ok_or("Image preview expired; select the file again.")?;
    let entry = cache.remove(index).ok_or("Image preview expired")?;
    let result = (entry.mime.clone(), entry.bytes.clone());
    cache.push_back(entry);
    Ok(result)
}
pub fn prepare(bytes: &[u8]) -> ImagePreview {
    match prepare_inner(bytes) {
        Ok(preview) => preview,
        Err(error) => ImagePreview {
            asset: String::new(),
            width: 0,
            height: 0,
            bytes: bytes.len(),
            format: String::new(),
            error,
        },
    }
}
fn prepare_inner(bytes: &[u8]) -> Result<ImagePreview, String> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("Image exceeds the 32 MiB preview limit.".into());
    }
    let format = image::guess_format(bytes).map_err(|_| "Invalid or unsupported image data.")?;
    let (name, mime) = match format {
        image::ImageFormat::Png => ("PNG", "image/png"),
        image::ImageFormat::Jpeg => ("JPEG", "image/jpeg"),
        image::ImageFormat::Gif => ("GIF", "image/gif"),
        image::ImageFormat::WebP => ("WebP", "image/webp"),
        image::ImageFormat::Bmp => ("BMP", "image/bmp"),
        image::ImageFormat::Tiff => ("TIFF", "image/tiff"),
        image::ImageFormat::Ico => ("ICO", "image/png"),
        _ => return Err("This image format is not supported for preview.".into()),
    };
    let reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| format!("Cannot read image: {e}"))?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("Image exceeds the 16 megapixel preview limit.".into());
    }
    // Validate in the Git worker, so a corrupt image never breaks the Rhai render.
    let decoded = image::ImageReader::with_format(Cursor::new(bytes), format)
        .decode()
        .map_err(|e| format!("Cannot decode image: {e}"))?;
    // The UI asset loader does not decode ICO; serve its selected frame as PNG.
    let mut png = Cursor::new(Vec::new());
    let asset_bytes = if format == image::ImageFormat::Ico {
        decoded
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| format!("Cannot convert icon for preview: {e}"))?;
        png.get_ref().as_slice()
    } else {
        bytes
    };
    let key = format!("{:x}", Sha256::digest(bytes));
    let mut cache = cache().lock().map_err(|e| e.to_string())?;
    if !cache.iter().any(|v| v.key == key) {
        while cache.iter().map(|v| v.bytes.len()).sum::<usize>() + asset_bytes.len() > CACHE_BYTES {
            cache.pop_front();
        }
        cache.push_back(Cached {
            key: key.clone(),
            mime: mime.into(),
            bytes: asset_bytes.to_vec(),
        });
    }
    Ok(ImagePreview {
        asset: format!("preview/{key}"),
        width,
        height,
        bytes: bytes.len(),
        format: name.into(),
        error: String::new(),
    })
}
