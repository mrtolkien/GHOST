use std::path::Path;

use base64::Engine;

const MAX_IMAGE_DIMENSION: u32 = 2048;
const JPEG_QUALITY: u8 = 85;

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

pub fn is_image_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

pub fn mime_type_from_extension(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Load an image from disk, optionally resize/compress, return (base64, mime_type).
pub fn load_image_base64(path: &Path) -> Result<(String, String), String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("failed to read image '{}': {e}", path.display()))?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let original_mime = mime_type_from_extension(ext).to_string();

    if let Some((compressed, mime)) = compress_image(&bytes) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
        Ok((b64, mime))
    } else {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Ok((b64, original_mime))
    }
}

/// Resize if > MAX_IMAGE_DIMENSION, recompress as JPEG.
/// Returns None if image is already small enough or can't be decoded.
fn compress_image(bytes: &[u8]) -> Option<(Vec<u8>, String)> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    if w <= MAX_IMAGE_DIMENSION && h <= MAX_IMAGE_DIMENSION {
        return None;
    }
    let resized = img.resize(
        MAX_IMAGE_DIMENSION,
        MAX_IMAGE_DIMENSION,
        image::imageops::FilterType::Lanczos3,
    );
    let mut buf = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
    resized.write_with_encoder(encoder).ok()?;
    Some((buf.into_inner(), "image/jpeg".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_image_extension_known_types() {
        assert!(is_image_extension("png"));
        assert!(is_image_extension("PNG"));
        assert!(is_image_extension("jpg"));
        assert!(is_image_extension("jpeg"));
        assert!(is_image_extension("gif"));
        assert!(is_image_extension("webp"));
    }

    #[test]
    fn is_image_extension_rejects_non_image() {
        assert!(!is_image_extension("txt"));
        assert!(!is_image_extension("rs"));
        assert!(!is_image_extension("pdf"));
    }

    #[test]
    fn mime_type_from_extension_returns_correct_types() {
        assert_eq!(mime_type_from_extension("png"), "image/png");
        assert_eq!(mime_type_from_extension("jpg"), "image/jpeg");
        assert_eq!(mime_type_from_extension("jpeg"), "image/jpeg");
        assert_eq!(mime_type_from_extension("gif"), "image/gif");
        assert_eq!(mime_type_from_extension("webp"), "image/webp");
        assert_eq!(mime_type_from_extension("txt"), "application/octet-stream");
    }

    #[test]
    fn compress_image_small_image_returns_none() {
        // Create a 2x2 PNG in memory
        let img = image::RgbImage::new(2, 2);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let bytes = buf.into_inner();

        assert!(compress_image(&bytes).is_none());
    }

    #[test]
    fn load_image_base64_with_small_png() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.png");
        let img = image::RgbImage::new(2, 2);
        img.save(&path).unwrap();

        let (b64, mime) = load_image_base64(&path).unwrap();
        assert!(!b64.is_empty());
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn load_image_base64_missing_file() {
        let result = load_image_base64(Path::new("/nonexistent/image.png"));
        assert!(result.is_err());
    }
}
