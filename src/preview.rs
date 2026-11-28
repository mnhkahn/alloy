//! Repository images never travel through Rhai strings or the text editor.
use resvg::{tiny_skia, usvg};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Arc, Mutex, OnceLock},
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
    #[serde(default)]
    pub source_available: bool,
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
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "svg"
            )
        })
}
pub(crate) fn is_svg_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}
pub fn has_image_signature(bytes: &[u8]) -> bool {
    image::guess_format(bytes).is_ok()
}
pub fn load_asset(key: &str) -> Result<(String, Vec<u8>), String> {
    let mut cache = cache().lock().map_err(|e| e.to_string())?;
    let index = cache
        .iter()
        .position(|v| v.key == key)
        .ok_or("Preview expired; select the file again.")?;
    let entry = cache.remove(index).ok_or("Preview expired")?;
    let result = (entry.mime.clone(), entry.bytes.clone());
    cache.push_back(entry);
    Ok(result)
}
pub fn prepare(bytes: &[u8]) -> ImagePreview {
    prepare_for_path("", bytes)
}
pub(crate) fn prepare_for_path(path: &str, bytes: &[u8]) -> ImagePreview {
    match prepare_inner(path, bytes) {
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
fn prepare_inner(path: &str, bytes: &[u8]) -> Result<ImagePreview, String> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("Image exceeds the 32 MiB preview limit.".into());
    }
    let format = match image::guess_format(bytes) {
        Ok(format) => format,
        Err(_) if is_svg_path(path) => {
            let (width, height, png) = render_svg(bytes)?;
            return cache_image(bytes, width, height, "SVG", "image/png", &png);
        }
        Err(_) => return Err("Invalid or unsupported image data.".into()),
    };
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
    check_dimensions(width, height)?;
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
    cache_image(bytes, width, height, name, mime, asset_bytes)
}

fn check_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("Image exceeds the 16 megapixel preview limit.".into());
    }
    Ok(())
}

fn render_svg(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let source = std::str::from_utf8(bytes).map_err(|e| format!("Cannot read SVG: {e}"))?;
    let fontdb = FONTS.get_or_init(|| {
        let mut database = usvg::fontdb::Database::new();
        database.load_system_fonts();
        Arc::new(database)
    });
    let resolve_data = usvg::ImageHrefResolver::default_data_resolver();
    let options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(move |mime, data, options| {
                // Check raster dimensions before decoding and skip compressed SVGs.
                if data.starts_with(&[0x1f, 0x8b]) {
                    return None;
                }
                if let Ok(format) = image::guess_format(&data) {
                    let (width, height) = image::ImageReader::with_format(Cursor::new(&*data), format)
                        .into_dimensions()
                        .ok()?;
                    check_dimensions(width, height).ok()?;
                }
                resolve_data(mime, data, options)
            }),
            // A repository SVG can be rendered from any revision without reading external files.
            resolve_string: Box::new(|_, _| None),
        },
        fontdb: fontdb.clone(),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(source, &options).map_err(|e| format!("Cannot read SVG: {e}"))?;
    let size = tree.size().to_int_size();
    let (width, height) = (size.width(), size.height());
    check_dimensions(width, height)?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or("Cannot allocate SVG preview.")?;
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    let png = pixmap
        .encode_png()
        .map_err(|e| format!("Cannot encode SVG preview: {e}"))?;
    Ok((width, height, png))
}

fn cache_image(
    bytes: &[u8],
    width: u32,
    height: u32,
    format: &str,
    mime: &str,
    asset_bytes: &[u8],
) -> Result<ImagePreview, String> {
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
        format: format.into(),
        error: String::new(),
    })
}

/// Store original audio bytes separately from image decoding.
pub(crate) fn cache_audio(bytes: &[u8]) -> Result<String, String> {
    let key = format!("audio-{:x}", Sha256::digest(bytes));
    let mut cache = cache().lock().map_err(|e| e.to_string())?;
    if !cache.iter().any(|v| v.key == key) {
        while cache.iter().map(|v| v.bytes.len()).sum::<usize>() + bytes.len() > CACHE_BYTES {
            cache.pop_front();
        }
        cache.push_back(Cached { key: key.clone(), mime: "application/octet-stream".into(), bytes: bytes.to_vec() });
    }
    Ok(key)
}
