//! Shared media type classification for all LibreWin apps.
//!
//! Two complementary classification schemes:
//!
//! - [`category_for_ext`] — Prism-style detailed categories ("image", "image_convert",
//!   "video", "audio", "pdf", "model", "unknown"). Expects lowercase input.
//! - [`mime_for_ext`] — MIME type string for a lowercase extension.
//! - [`media_type_for`] — Fade-style broad type ("image", "video", "audio", "unknown").
//!   Case-insensitive. All image variants (including raw/special formats) map to "image".

/// Classify a **lowercase** file extension into a Prism viewer category.
///
/// Returns one of: `"image"`, `"image_convert"`, `"video"`, `"audio"`,
/// `"pdf"`, `"model"`, `"unknown"`.
///
/// `"image_convert"` covers formats that require an ImageMagick pre-conversion
/// step before the browser can display them (RAW, HDR, multi-layer, etc.).
pub fn category_for_ext(ext: &str) -> &'static str {
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" => "image",
        "heic" | "heif" | "tiff" | "tif" | "psd" | "exr" | "hdr" | "dds" | "raw" | "cr2"
        | "cr3" | "nef" | "arw" | "dng" | "orf" | "rw2" | "xcf" => "image_convert",
        "mp4" | "m4v" | "webm" | "mov" | "avi" | "mkv" | "flv" | "wmv" | "mpg" | "mpeg" | "ogv"
        | "ts" | "3gp" | "divx" | "rmvb" | "asf" => "video",
        "mp3" | "aac" | "ogg" | "oga" | "wav" | "flac" | "m4a" | "opus" | "wma" | "aiff"
        | "aif" | "alac" | "ac3" | "dts" => "audio",
        "pdf" => "pdf",
        "obj" | "gltf" | "glb" | "stl" | "fbx" | "ply" | "3ds" => "model",
        _ => "unknown",
    }
}

/// Map a **lowercase** file extension to its MIME type string.
///
/// Returns `"application/octet-stream"` for unrecognised extensions.
pub fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        // video
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "ogv" => "video/ogg",
        "ts" => "video/mp2t",
        "flv" => "video/x-flv",
        "wmv" => "video/x-ms-wmv",
        "3gp" => "video/3gpp",
        "mpeg" | "mpg" => "video/mpeg",
        // audio
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "m4a" | "alac" => "audio/mp4",
        "opus" => "audio/ogg; codecs=opus",
        "wma" => "audio/x-ms-wma",
        "aiff" | "aif" => "audio/aiff",
        // image
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        // document
        "pdf" => "application/pdf",
        // 3D models
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        "obj" => "text/plain",
        "stl" => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

/// Determine the broad media type for a file extension (case-insensitive).
///
/// Returns one of: `"image"`, `"video"`, `"audio"`, `"unknown"`.
///
/// Unlike [`category_for_ext`], all image variants — including RAW and
/// special-format files — map to `"image"` here. Use this for conversion
/// pipeline routing where sub-categories are not needed.
pub fn media_type_for(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "tiff" | "tif" | "bmp" | "gif" | "avif" | "heic"
        | "heif" | "psd" | "svg" | "ico" | "raw" | "cr2" | "cr3" | "nef" | "arw" | "dng"
        | "orf" | "rw2" | "exr" | "hdr" | "dds" | "xcf" => "image",
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "flv" | "wmv" | "ts" | "mpg" | "mpeg"
        | "3gp" | "ogv" | "divx" | "rmvb" | "asf" => "video",
        "mp3" | "wav" | "flac" | "ogg" | "oga" | "aac" | "opus" | "m4a" | "wma" | "aiff"
        | "aif" | "alac" | "ac3" | "dts" => "audio",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_image() {
        assert_eq!(category_for_ext("png"), "image");
        assert_eq!(category_for_ext("jpg"), "image");
        assert_eq!(category_for_ext("webp"), "image");
    }

    #[test]
    fn category_image_convert() {
        assert_eq!(category_for_ext("heic"), "image_convert");
        assert_eq!(category_for_ext("tiff"), "image_convert");
        assert_eq!(category_for_ext("raw"), "image_convert");
        assert_eq!(category_for_ext("exr"), "image_convert");
    }

    #[test]
    fn category_video() {
        assert_eq!(category_for_ext("mp4"), "video");
        assert_eq!(category_for_ext("mkv"), "video");
        assert_eq!(category_for_ext("divx"), "video");
    }

    #[test]
    fn category_audio() {
        assert_eq!(category_for_ext("mp3"), "audio");
        assert_eq!(category_for_ext("flac"), "audio");
        assert_eq!(category_for_ext("ac3"), "audio");
    }

    #[test]
    fn category_pdf_and_model() {
        assert_eq!(category_for_ext("pdf"), "pdf");
        assert_eq!(category_for_ext("gltf"), "model");
        assert_eq!(category_for_ext("stl"), "model");
    }

    #[test]
    fn mime_video() {
        assert_eq!(mime_for_ext("mp4"), "video/mp4");
        assert_eq!(mime_for_ext("webm"), "video/webm");
        assert_eq!(mime_for_ext("mkv"), "video/x-matroska");
    }

    #[test]
    fn mime_audio() {
        assert_eq!(mime_for_ext("mp3"), "audio/mpeg");
        assert_eq!(mime_for_ext("flac"), "audio/flac");
        assert_eq!(mime_for_ext("wav"), "audio/wav");
    }

    #[test]
    fn mime_image() {
        assert_eq!(mime_for_ext("png"), "image/png");
        assert_eq!(mime_for_ext("jpg"), "image/jpeg");
        assert_eq!(mime_for_ext("webp"), "image/webp");
    }

    #[test]
    fn mime_unknown_falls_back() {
        assert_eq!(mime_for_ext("xyz"), "application/octet-stream");
    }

    #[test]
    fn media_type_image_variants() {
        assert_eq!(media_type_for("jpg"), "image");
        assert_eq!(media_type_for("heic"), "image");
        assert_eq!(media_type_for("raw"), "image");
        assert_eq!(media_type_for("exr"), "image");
    }

    #[test]
    fn media_type_case_insensitive() {
        assert_eq!(media_type_for("JPG"), "image");
        assert_eq!(media_type_for("MP4"), "video");
        assert_eq!(media_type_for("FLAC"), "audio");
    }

    #[test]
    fn media_type_unknown() {
        assert_eq!(media_type_for("xyz"), "unknown");
        assert_eq!(media_type_for(""), "unknown");
    }
}
