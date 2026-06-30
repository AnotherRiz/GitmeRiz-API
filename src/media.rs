use chrono::{Datelike, Timelike, Utc};
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

    pub fn prefix(&self) -> &'static str {
        match self {
            MediaType::Gallery => "image",
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

/// Get month name from month number
fn month_name(month: u32) -> &'static str {
    match month {
        1 => "january",
        2 => "february",
        3 => "march",
        4 => "april",
        5 => "may",
        6 => "june",
        7 => "july",
        8 => "august",
        9 => "september",
        10 => "october",
        11 => "november",
        12 => "december",
        _ => "unknown",
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
    media_dir: &str,
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
    
    // Folder: {type}/{YYYY}/{month}/{type-YYYY-MM-DD}/
    let folder = format!(
        "{}/{}/{}/{}-{:04}-{:02}-{:02}",
        media_type.folder_name(),
        year,
        month_name(month),
        media_type.prefix(),
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
    let full_path = PathBuf::from(media_dir).join(&relative_path);
    
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
pub async fn delete_file(media_dir: &str, stored_path: &str) -> Result<(), std::io::Error> {
    let full_path = PathBuf::from(media_dir).join(stored_path);
    if full_path.exists() {
        fs::remove_file(full_path).await?;
    }
    Ok(())
}

/// Read file from disk
pub async fn read_file(media_dir: &str, stored_path: &str) -> Result<Vec<u8>, std::io::Error> {
    let full_path = PathBuf::from(media_dir).join(stored_path);
    fs::read(full_path).await
}
