use chrono::{Datelike, Timelike, Utc};
use rand::Rng;
use std::path::PathBuf;
use tokio::fs;
use uuid::Uuid;

/// Media type for organizing uploads
#[derive(Debug, Clone, Copy)]
pub enum MediaType {
    Gallery,
    Video,
    Audio,
}

impl MediaType {
    pub fn folder_name(&self) -> &'static str {
        match self {
            MediaType::Gallery => "gallery",
            MediaType::Video => "video",
            MediaType::Audio => "audio",
        }
    }


    /// Returns allowed extensions for this media type (lowercase, with dot)
    pub fn allowed_extensions(&self) -> &[&'static str] {
        match self {
            MediaType::Gallery => &[
                ".jpg", ".jpeg", ".png", ".gif", ".webp", 
                ".heic", ".heif", ".svg", ".raw", ".cr2", ".nef", ".dng"
            ],
            MediaType::Video => &[".mov", ".avi", ".mkv", ".mp4", ".webm"],
            MediaType::Audio => &[".flac", ".wav", ".m4a", ".mp3", ".aac", ".ogg"],
        }
    }

    /// Returns max file size in bytes (None = no limit)
    pub fn max_size_bytes(&self) -> Option<u64> {
        match self {
            MediaType::Gallery => Some(100 * 1024 * 1024), // 100 MB
            MediaType::Video => None,
            MediaType::Audio => None,
        }
    }

    /// Returns MIME type based on extension
    pub fn mime_type_for_extension(&self, ext: &str) -> &'static str {
        let ext_lower = ext.to_lowercase();
        match self {
            MediaType::Gallery => match ext_lower.as_str() {
                ".jpg" | ".jpeg" => "image/jpeg",
                ".png" => "image/png",
                ".gif" => "image/gif",
                ".webp" => "image/webp",
                ".heic" | ".heif" => "image/heic",
                ".svg" => "image/svg+xml",
                ".raw" | ".cr2" | ".nef" | ".dng" => "image/raw",
                _ => "application/octet-stream",
            },
            MediaType::Video => match ext_lower.as_str() {
                ".mov" => "video/quicktime",
                ".avi" => "video/x-msvideo",
                ".mkv" => "video/x-matroska",
                ".mp4" => "video/mp4",
                ".webm" => "video/webm",
                _ => "application/octet-stream",
            },
            MediaType::Audio => match ext_lower.as_str() {
                ".flac" => "audio/flac",
                ".wav" => "audio/wav",
                ".m4a" => "audio/mp4",
                ".mp3" => "audio/mpeg",
                ".aac" => "audio/aac",
                ".ogg" => "audio/ogg",
                _ => "application/octet-stream",
            },
        }
    }
}


/// Extract file extension from filename (lowercase, with dot)
pub fn get_extension(filename: &str) -> Option<String> {
    std::path::Path::new(filename)
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy().to_lowercase()))
}

/// Validate file extension for a media type
pub fn validate_extension(media_type: MediaType, filename: &str) -> Result<String, String> {
    let ext = get_extension(filename).ok_or_else(|| "File has no extension".to_string())?;
    
    if media_type.allowed_extensions().contains(&ext.as_str()) {
        Ok(ext)
    } else {
        Err(format!(
            "Invalid file extension '{}'. Allowed: {}",
            ext,
            media_type.allowed_extensions().join(", ")
        ))
    }
}

/// Validate file size for a media type
pub fn validate_size(media_type: MediaType, size_bytes: u64) -> Result<(), String> {
    if let Some(max_size) = media_type.max_size_bytes() {
        if size_bytes > max_size {
            let max_mb = max_size / (1024 * 1024);
            let file_mb = size_bytes / (1024 * 1024);
            return Err(format!(
                "File size ({} MB) exceeds maximum allowed size ({} MB)",
                file_mb, max_mb
            ));
        }
    }
    Ok(())
}

/// Generate the storage path for a file
/// Returns (relative_path, full_path)
pub fn generate_storage_path(
    storage_dir: &str,
    media_type: MediaType,
    extension: &str,
) -> (String, PathBuf) {
    let now = Utc::now();
    let year = now.year();
    let month = now.month();
    let day = now.day();
    let hour = now.hour();
    let minute = now.minute();
    let second = now.second();
    
    let uuid = Uuid::new_v4();
    
    // Folder: {type}/{YYYY}/{MM}/{YYYY-MM-DD}/
    let folder = format!(
        "{}/{}/{:02}/{:04}-{:02}-{:02}",
        media_type.folder_name(),
        year,
        month,
        year,
        month,
        day
    );
    
    // Filename: YYYY-MM-DD_HH-MM-SS_UUID.ext
    let filename = format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}_{}{}",
        year, month, day, hour, minute, second, uuid, extension
    );
    
    let relative_path = format!("{}/{}", folder, filename);
    let full_path = PathBuf::from(storage_dir).join(&relative_path);
    
    (relative_path, full_path)
}

/// Create directory if it doesn't exist
pub async fn ensure_directory_exists(path: &PathBuf) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}

/// Save file bytes to disk
pub async fn save_file(path: &PathBuf, data: &[u8]) -> Result<(), std::io::Error> {
    ensure_directory_exists(path).await?;
    fs::write(path, data).await
}

/// Delete file from disk
pub async fn delete_file(storage_dir: &str, stored_path: &str) -> Result<(), std::io::Error> {
    let full_path = PathBuf::from(storage_dir).join(stored_path);
    if full_path.exists() {
        fs::remove_file(full_path).await?;
    }
    Ok(())
}

/// Read file from disk
pub async fn read_file(storage_dir: &str, stored_path: &str) -> Result<Vec<u8>, std::io::Error> {
    let full_path = PathBuf::from(storage_dir).join(stored_path);
    fs::read(full_path).await
}

/// Generate a random 8-character short ID using URL-safe alphanumeric characters
pub fn generate_short_id() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    
    (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Generate thumbnail path from original stored path
/// Converts: gallery/2026/07/2026-07-01/2026-07-01_15-15-01_UUID.png
/// To:       gallery/2026/07/2026-07-01/2026-07-01_15-15-01_UUID-thumb.webp
pub fn generate_thumbnail_path(original_stored_path: &str) -> String {
    // Remove extension from original path
    let path_without_ext = if let Some(pos) = original_stored_path.rfind('.') {
        &original_stored_path[..pos]
    } else {
        original_stored_path
    };
    
    // Append -thumb.webp
    format!("{}-thumb.webp", path_without_ext)
}

/// Generate preview path from original stored path
/// Converts: gallery/2026/07/2026-07-01/2026-07-01_15-15-01_UUID.png
/// To:       gallery/2026/07/2026-07-01/2026-07-01_15-15-01_UUID-preview.webp
pub fn generate_preview_path(original_stored_path: &str) -> String {
    // Remove extension from original path
    let path_without_ext = if let Some(pos) = original_stored_path.rfind('.') {
        &original_stored_path[..pos]
    } else {
        original_stored_path
    };
    
    // Append -preview.webp
    format!("{}-preview.webp", path_without_ext)
}

/// Generate thumbnail and preview from image bytes in one pass (optimized)
/// - Decodes image ONCE (most expensive operation)
/// - Creates preview from decoded image
/// - Creates thumbnail from preview (cascading resize - faster!)
/// Returns: (thumbnail_bytes, preview_bytes)
pub fn generate_thumbnail_and_preview(image_data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    use image::{GenericImageView, ImageReader};
    use std::io::Cursor;
    
    // DECODE ONCE - Most expensive operation!
    let img = ImageReader::new(Cursor::new(image_data))
        .with_guessed_format()
        .map_err(|e| format!("Failed to detect image format: {}", e))?
        .decode()
        .map_err(|e| format!("Failed to decode image: {}", e))?;
    
    let (orig_width, orig_height) = img.dimensions();
    
    // === PREVIEW: Resize from original (1280px max) ===
    let preview_img = if orig_width > 1280 {
        let ratio = 1280.0 / orig_width as f32;
        let new_height = (orig_height as f32 * ratio) as u32;
        img.resize(1280, new_height, image::imageops::FilterType::Lanczos3)
    } else {
        img.clone() // Keep original if already smaller
    };
    
    // Encode preview to WebP (quality 85)
    let preview_rgba = preview_img.to_rgba8();
    let (preview_width, preview_height) = (preview_rgba.width(), preview_rgba.height());
    let preview_encoder = webp::Encoder::from_rgba(&preview_rgba, preview_width, preview_height);
    let preview_encoded = preview_encoder.encode(85.0);
    
    // === THUMBNAIL: Cascading resize from preview (500px max) ===
    // This is MUCH faster than resizing from original 4000px image!
    let (preview_w, preview_h) = preview_img.dimensions();
    let thumb_img = if preview_w > 500 {
        let ratio = 500.0 / preview_w as f32;
        let new_height = (preview_h as f32 * ratio) as u32;
        preview_img.resize(500, new_height, image::imageops::FilterType::Lanczos3)
    } else {
        preview_img // Already small enough
    };
    
    // Encode thumbnail to WebP (quality 80)
    let thumb_rgba = thumb_img.to_rgba8();
    let (thumb_width, thumb_height) = (thumb_rgba.width(), thumb_rgba.height());
    let thumb_encoder = webp::Encoder::from_rgba(&thumb_rgba, thumb_width, thumb_height);
    let thumb_encoded = thumb_encoder.encode(80.0);
    
    Ok((thumb_encoded.to_vec(), preview_encoded.to_vec()))
}
