use sqlx::{mysql::MySqlPoolOptions, MySql, Pool};

pub struct Database {
    pub pool: Pool<MySql>,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = MySqlPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;

        Ok(Database { pool })
    }

    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        // Create users table with role
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS users (
                id INT AUTO_INCREMENT PRIMARY KEY,
                name VARCHAR(255) NOT NULL,
                username VARCHAR(255) NOT NULL UNIQUE,
                email VARCHAR(255) NOT NULL UNIQUE,
                password_hash VARCHAR(255) NOT NULL,
                role ENUM('superuser', 'admin', 'user') NOT NULL DEFAULT 'user',
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Check if gallery table has legacy schema (e.g. contains 'filename' column instead of 'original_filename')
        let has_legacy_gallery = match sqlx::query("SHOW COLUMNS FROM gallery LIKE 'filename'")
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(_)) => true,
            _ => false,
        };

        if has_legacy_gallery {
            tracing::info!("Dropping legacy gallery, videos, and audio tables to apply new schema");
            let _ = sqlx::query("DROP TABLE IF EXISTS audio").execute(&self.pool).await;
            let _ = sqlx::query("DROP TABLE IF EXISTS videos").execute(&self.pool).await;
            let _ = sqlx::query("DROP TABLE IF EXISTS gallery").execute(&self.pool).await;
        }

        // Create gallery table with file storage columns
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS gallery (
                id INT AUTO_INCREMENT PRIMARY KEY,
                user_id INT NOT NULL,
                title VARCHAR(255) NOT NULL,
                original_filename VARCHAR(255) NOT NULL,
                stored_path VARCHAR(512) NOT NULL,
                size_bytes BIGINT NOT NULL,
                mime_type VARCHAR(100) NOT NULL,
                visibility ENUM('public', 'private') NOT NULL DEFAULT 'private',
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Check if gallery table is missing 'visibility' column (for existing installations)
        let has_visibility = match sqlx::query("SHOW COLUMNS FROM gallery LIKE 'visibility'")
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(_)) => true,
            _ => false,
        };

        if !has_visibility {
            tracing::info!("Adding visibility column to gallery table");
            sqlx::query("ALTER TABLE gallery ADD COLUMN visibility ENUM('public', 'private') NOT NULL DEFAULT 'private'")
                .execute(&self.pool)
                .await?;
        }

        // Check if gallery table is missing 'short_id' column (for existing installations)
        let has_short_id = match sqlx::query("SHOW COLUMNS FROM gallery LIKE 'short_id'")
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(_)) => true,
            _ => false,
        };

        if !has_short_id {
            tracing::info!("Adding short_id column to gallery table");
            // Add column as nullable first
            sqlx::query("ALTER TABLE gallery ADD COLUMN short_id CHAR(8)")
                .execute(&self.pool)
                .await?;

            // Backfill existing rows with unique short_ids
            let existing_rows: Vec<(i32,)> = sqlx::query_as("SELECT id FROM gallery WHERE short_id IS NULL")
                .fetch_all(&self.pool)
                .await?;

            for (id,) in existing_rows {
                loop {
                    let short_id = crate::media::generate_short_id();
                    let result = sqlx::query("UPDATE gallery SET short_id = ? WHERE id = ?")
                        .bind(&short_id)
                        .bind(id)
                        .execute(&self.pool)
                        .await;
                    
                    if result.is_ok() {
                        break;
                    }
                    // If collision, retry with new short_id
                }
            }

            // Now make it NOT NULL and UNIQUE
            sqlx::query("ALTER TABLE gallery MODIFY COLUMN short_id CHAR(8) NOT NULL")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE gallery ADD UNIQUE KEY unique_short_id (short_id)")
                .execute(&self.pool)
                .await?;
            
            tracing::info!("Gallery short_id column added and backfilled successfully");
        }

        // Check if gallery table is missing 'thumbnail_path' column (for pre-generated thumbnails)
        let has_thumbnail_path = match sqlx::query("SHOW COLUMNS FROM gallery LIKE 'thumbnail_path'")
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(_)) => true,
            _ => false,
        };

        if !has_thumbnail_path {
            tracing::info!("Adding thumbnail_path column to gallery table");
            sqlx::query("ALTER TABLE gallery ADD COLUMN thumbnail_path VARCHAR(512)")
                .execute(&self.pool)
                .await?;
            
            tracing::info!("Gallery thumbnail_path column added successfully");
        }

        // Check if gallery table is missing 'pinned' column
        let has_pinned = match sqlx::query("SHOW COLUMNS FROM gallery LIKE 'pinned'")
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(_)) => true,
            _ => false,
        };

        if !has_pinned {
            tracing::info!("Adding pinned column to gallery table");
            sqlx::query("ALTER TABLE gallery ADD COLUMN pinned BOOLEAN NOT NULL DEFAULT FALSE")
                .execute(&self.pool)
                .await?;
            
            tracing::info!("Gallery pinned column added successfully");
        }

        // Check if gallery table is missing 'status' column
        let has_status = match sqlx::query("SHOW COLUMNS FROM gallery LIKE 'status'")
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(_)) => true,
            _ => false,
        };

        if !has_status {
            tracing::info!("Adding status column to gallery table");
            sqlx::query(
                "ALTER TABLE gallery ADD COLUMN status ENUM('processing', 'active', 'failed_processing') NOT NULL DEFAULT 'active'"
            )
            .execute(&self.pool)
            .await?;
            
            tracing::info!("Gallery status column added successfully");
        }

        // Check if gallery table is missing 'preview_path' column
        let has_preview_path = match sqlx::query("SHOW COLUMNS FROM gallery LIKE 'preview_path'")
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(_)) => true,
            _ => false,
        };

        if !has_preview_path {
            tracing::info!("Adding preview_path column to gallery table");
            sqlx::query("ALTER TABLE gallery ADD COLUMN preview_path VARCHAR(512)")
                .execute(&self.pool)
                .await?;
            
            tracing::info!("Gallery preview_path column added successfully");
        }

        // Check if gallery table is missing 'pin_order' column
        let has_pin_order = match sqlx::query("SHOW COLUMNS FROM gallery LIKE 'pin_order'")
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(_)) => true,
            _ => false,
        };

        if !has_pin_order {
            tracing::info!("Adding pin_order column to gallery table");
            sqlx::query("ALTER TABLE gallery ADD COLUMN pin_order INT NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
            
            tracing::info!("Gallery pin_order column added successfully");
        }

        // Create videos table with file storage columns
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS videos (
                id INT AUTO_INCREMENT PRIMARY KEY,
                user_id INT NOT NULL,
                title VARCHAR(255) NOT NULL,
                original_filename VARCHAR(255) NOT NULL,
                stored_path VARCHAR(512) NOT NULL,
                size_bytes BIGINT NOT NULL,
                mime_type VARCHAR(100) NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // --- Video table column migrations (idempotent) ---

        // Add visibility column to videos
        let has_video_visibility = matches!(
            sqlx::query("SHOW COLUMNS FROM videos LIKE 'visibility'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_video_visibility {
            tracing::info!("Adding visibility column to videos table");
            sqlx::query("ALTER TABLE videos ADD COLUMN visibility ENUM('public', 'private') NOT NULL DEFAULT 'private'")
                .execute(&self.pool)
                .await?;
        }

        // Add description column to videos
        let has_video_description = matches!(
            sqlx::query("SHOW COLUMNS FROM videos LIKE 'description'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_video_description {
            tracing::info!("Adding description column to videos table");
            sqlx::query("ALTER TABLE videos ADD COLUMN description TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Add short_id column to videos (with backfill)
        let has_video_short_id = matches!(
            sqlx::query("SHOW COLUMNS FROM videos LIKE 'short_id'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_video_short_id {
            tracing::info!("Adding short_id column to videos table");
            // Add column as nullable first
            sqlx::query("ALTER TABLE videos ADD COLUMN short_id CHAR(8)")
                .execute(&self.pool)
                .await?;

            // Backfill existing rows with unique short_ids
            let existing_rows: Vec<(i32,)> = sqlx::query_as("SELECT id FROM videos WHERE short_id IS NULL")
                .fetch_all(&self.pool)
                .await?;

            for (id,) in existing_rows {
                loop {
                    let short_id = crate::media::generate_short_id();
                    let result = sqlx::query("UPDATE videos SET short_id = ? WHERE id = ?")
                        .bind(&short_id)
                        .bind(id)
                        .execute(&self.pool)
                        .await;

                    if result.is_ok() {
                        break;
                    }
                    // If collision, retry with new short_id
                }
            }

            // Now make it NOT NULL and UNIQUE
            sqlx::query("ALTER TABLE videos MODIFY COLUMN short_id CHAR(8) NOT NULL")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE videos ADD UNIQUE KEY unique_video_short_id (short_id)")
                .execute(&self.pool)
                .await?;

            tracing::info!("Videos short_id column added and backfilled successfully");
        }

        // Add thumbnail_path column to videos
        let has_video_thumbnail = matches!(
            sqlx::query("SHOW COLUMNS FROM videos LIKE 'thumbnail_path'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_video_thumbnail {
            tracing::info!("Adding thumbnail_path column to videos table");
            sqlx::query("ALTER TABLE videos ADD COLUMN thumbnail_path VARCHAR(512)")
                .execute(&self.pool)
                .await?;
        }

        // Add transcoded_path column to videos (for web-safe mp4 when original is mkv/avi/mov)
        let has_video_transcoded = matches!(
            sqlx::query("SHOW COLUMNS FROM videos LIKE 'transcoded_path'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_video_transcoded {
            tracing::info!("Adding transcoded_path column to videos table");
            sqlx::query("ALTER TABLE videos ADD COLUMN transcoded_path VARCHAR(512)")
                .execute(&self.pool)
                .await?;
        }

        // Add pinned column to videos
        let has_video_pinned = matches!(
            sqlx::query("SHOW COLUMNS FROM videos LIKE 'pinned'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_video_pinned {
            tracing::info!("Adding pinned column to videos table");
            sqlx::query("ALTER TABLE videos ADD COLUMN pinned BOOLEAN NOT NULL DEFAULT FALSE")
                .execute(&self.pool)
                .await?;
        }

        // Add pin_order column to videos
        let has_video_pin_order = matches!(
            sqlx::query("SHOW COLUMNS FROM videos LIKE 'pin_order'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_video_pin_order {
            tracing::info!("Adding pin_order column to videos table");
            sqlx::query("ALTER TABLE videos ADD COLUMN pin_order INT NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }

        // Add status column to videos
        let has_video_status = matches!(
            sqlx::query("SHOW COLUMNS FROM videos LIKE 'status'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_video_status {
            tracing::info!("Adding status column to videos table");
            sqlx::query(
                "ALTER TABLE videos ADD COLUMN status ENUM('processing', 'active', 'failed_processing') NOT NULL DEFAULT 'active'"
            )
            .execute(&self.pool)
            .await?;
        }

        // Add processing_progress column to videos
        let has_video_progress = matches!(
            sqlx::query("SHOW COLUMNS FROM videos LIKE 'processing_progress'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_video_progress {
            tracing::info!("Adding processing_progress column to videos table");
            sqlx::query("ALTER TABLE videos ADD COLUMN processing_progress INT NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }

        // Add thumbnail_is_custom column to videos (true when user uploaded their own thumbnail)
        let has_video_thumbnail_custom = matches!(
            sqlx::query("SHOW COLUMNS FROM videos LIKE 'thumbnail_is_custom'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_video_thumbnail_custom {
            tracing::info!("Adding thumbnail_is_custom column to videos table");
            sqlx::query("ALTER TABLE videos ADD COLUMN thumbnail_is_custom BOOLEAN NOT NULL DEFAULT FALSE")
                .execute(&self.pool)
                .await?;
        }

        // Create audio table with file storage columns
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS audio (
                id INT AUTO_INCREMENT PRIMARY KEY,
                user_id INT NOT NULL,
                title VARCHAR(255) NOT NULL,
                original_filename VARCHAR(255) NOT NULL,
                stored_path VARCHAR(512) NOT NULL,
                size_bytes BIGINT NOT NULL,
                mime_type VARCHAR(100) NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // --- Audio table column migrations (idempotent) ---

        // Add description column to audio
        let has_audio_description = matches!(
            sqlx::query("SHOW COLUMNS FROM audio LIKE 'description'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_audio_description {
            tracing::info!("Adding description column to audio table");
            sqlx::query("ALTER TABLE audio ADD COLUMN description TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Add visibility column to audio
        let has_audio_visibility = matches!(
            sqlx::query("SHOW COLUMNS FROM audio LIKE 'visibility'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_audio_visibility {
            tracing::info!("Adding visibility column to audio table");
            sqlx::query("ALTER TABLE audio ADD COLUMN visibility ENUM('public', 'private') NOT NULL DEFAULT 'private'")
                .execute(&self.pool)
                .await?;
        }

        // Add thumbnail_path column to audio (optional cover art)
        let has_audio_thumbnail = matches!(
            sqlx::query("SHOW COLUMNS FROM audio LIKE 'thumbnail_path'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_audio_thumbnail {
            tracing::info!("Adding thumbnail_path column to audio table");
            sqlx::query("ALTER TABLE audio ADD COLUMN thumbnail_path VARCHAR(512)")
                .execute(&self.pool)
                .await?;
        }

        // Add pinned column to audio
        let has_audio_pinned = matches!(
            sqlx::query("SHOW COLUMNS FROM audio LIKE 'pinned'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_audio_pinned {
            tracing::info!("Adding pinned column to audio table");
            sqlx::query("ALTER TABLE audio ADD COLUMN pinned BOOLEAN NOT NULL DEFAULT FALSE")
                .execute(&self.pool)
                .await?;
        }

        // Add pin_order column to audio
        let has_audio_pin_order = matches!(
            sqlx::query("SHOW COLUMNS FROM audio LIKE 'pin_order'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_audio_pin_order {
            tracing::info!("Adding pin_order column to audio table");
            sqlx::query("ALTER TABLE audio ADD COLUMN pin_order INT NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }

        // Add short_id column to audio (with backfill)
        let has_audio_short_id = matches!(
            sqlx::query("SHOW COLUMNS FROM audio LIKE 'short_id'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_audio_short_id {
            tracing::info!("Adding short_id column to audio table");
            // Add column as nullable first
            sqlx::query("ALTER TABLE audio ADD COLUMN short_id CHAR(8)")
                .execute(&self.pool)
                .await?;

            // Backfill existing rows with unique short_ids
            let existing_rows: Vec<(i32,)> = sqlx::query_as("SELECT id FROM audio WHERE short_id IS NULL")
                .fetch_all(&self.pool)
                .await?;

            for (id,) in existing_rows {
                loop {
                    let short_id = crate::media::generate_short_id();
                    let result = sqlx::query("UPDATE audio SET short_id = ? WHERE id = ?")
                        .bind(&short_id)
                        .bind(id)
                        .execute(&self.pool)
                        .await;

                    if result.is_ok() {
                        break;
                    }
                    // If collision, retry with new short_id
                }
            }

            // Now make it NOT NULL and UNIQUE
            sqlx::query("ALTER TABLE audio MODIFY COLUMN short_id CHAR(8) NOT NULL")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE audio ADD UNIQUE KEY unique_audio_short_id (short_id)")
                .execute(&self.pool)
                .await?;

            tracing::info!("Audio short_id column added and backfilled successfully");
        }

        // Create audio_thumbnails table (supports multiple cover art images per audio item)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS audio_thumbnails (
                id INT AUTO_INCREMENT PRIMARY KEY,
                audio_id INT NOT NULL,
                thumbnail_path VARCHAR(512) NOT NULL,
                is_primary BOOLEAN NOT NULL DEFAULT FALSE,
                sort_order INT NOT NULL DEFAULT 0,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (audio_id) REFERENCES audio(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Backfill audio_thumbnails with existing thumbnail_path values
        let existing_thumbnails: Result<Vec<(i32, String)>, _> = sqlx::query_as(
            "SELECT id, thumbnail_path FROM audio WHERE thumbnail_path IS NOT NULL"
        )
        .fetch_all(&self.pool)
        .await;

        if let Ok(rows) = existing_thumbnails {
            for (audio_id, thumb_path) in rows {
                let already_exists: Result<Option<(i32,)>, _> = sqlx::query_as(
                    "SELECT id FROM audio_thumbnails WHERE audio_id = ? AND thumbnail_path = ?"
                )
                .bind(audio_id)
                .bind(&thumb_path)
                .fetch_optional(&self.pool)
                .await;

                if matches!(already_exists, Ok(None)) {
                    let _ = sqlx::query(
                        "INSERT INTO audio_thumbnails (audio_id, thumbnail_path, is_primary, sort_order) VALUES (?, ?, TRUE, 0)"
                    )
                    .bind(audio_id)
                    .bind(&thumb_path)
                    .execute(&self.pool)
                    .await;
                }
            }
        }

        // --- Audio Thumbnails table column migrations (idempotent) ---

        // Add short_id column to audio_thumbnails
        let has_audio_thumb_short_id = matches!(
            sqlx::query("SHOW COLUMNS FROM audio_thumbnails LIKE 'short_id'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_audio_thumb_short_id {
            tracing::info!("Adding short_id column to audio_thumbnails table");
            sqlx::query("ALTER TABLE audio_thumbnails ADD COLUMN short_id CHAR(8)")
                .execute(&self.pool)
                .await?;

            // Backfill existing rows with unique short_ids
            let existing_rows: Vec<(i32,)> = sqlx::query_as("SELECT id FROM audio_thumbnails WHERE short_id IS NULL")
                .fetch_all(&self.pool)
                .await?;

            for (id,) in existing_rows {
                loop {
                    let short_id = crate::media::generate_short_id();
                    let result = sqlx::query("UPDATE audio_thumbnails SET short_id = ? WHERE id = ?")
                        .bind(&short_id)
                        .bind(id)
                        .execute(&self.pool)
                        .await;

                    if result.is_ok() {
                        break;
                    }
                    // If collision, retry with new short_id
                }
            }

            // Now make it NOT NULL and UNIQUE
            sqlx::query("ALTER TABLE audio_thumbnails MODIFY COLUMN short_id CHAR(8) NOT NULL")
                .execute(&self.pool)
                .await?;
            sqlx::query("ALTER TABLE audio_thumbnails ADD UNIQUE KEY unique_audio_thumb_short_id (short_id)")
                .execute(&self.pool)
                .await?;

            tracing::info!("Audio thumbnails short_id column added and backfilled successfully");
        }

        // Add raw_path column to audio_thumbnails
        let has_audio_thumb_raw_path = matches!(
            sqlx::query("SHOW COLUMNS FROM audio_thumbnails LIKE 'raw_path'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_audio_thumb_raw_path {
            tracing::info!("Adding raw_path column to audio_thumbnails table");
            sqlx::query("ALTER TABLE audio_thumbnails ADD COLUMN raw_path VARCHAR(512)")
                .execute(&self.pool)
                .await?;

            // Backfill: set raw_path = thumbnail_path for existing rows (best-effort fallback)
            // since the original bytes were never retained during initial upload
            sqlx::query("UPDATE audio_thumbnails SET raw_path = thumbnail_path WHERE raw_path IS NULL")
                .execute(&self.pool)
                .await?;

            tracing::info!("Audio thumbnails raw_path column added and backfilled");
        }

        // Add preview_path column to audio_thumbnails
        let has_audio_thumb_preview_path = matches!(
            sqlx::query("SHOW COLUMNS FROM audio_thumbnails LIKE 'preview_path'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_audio_thumb_preview_path {
            tracing::info!("Adding preview_path column to audio_thumbnails table");
            sqlx::query("ALTER TABLE audio_thumbnails ADD COLUMN preview_path VARCHAR(512)")
                .execute(&self.pool)
                .await?;

            tracing::info!("Audio thumbnails preview_path column added");
        }

        // Add status column to audio_thumbnails
        let has_audio_thumb_status = matches!(
            sqlx::query("SHOW COLUMNS FROM audio_thumbnails LIKE 'status'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !has_audio_thumb_status {
            tracing::info!("Adding status column to audio_thumbnails table");
            sqlx::query(
                "ALTER TABLE audio_thumbnails ADD COLUMN status ENUM('processing', 'active', 'failed_processing') NOT NULL DEFAULT 'active'"
            )
            .execute(&self.pool)
            .await?;

            tracing::info!("Audio thumbnails status column added");
        }

        // Make thumbnail_path nullable (allow NULL during processing)
        let thumb_path_nullable = matches!(
            sqlx::query("SHOW COLUMNS FROM audio_thumbnails WHERE Field = 'thumbnail_path' AND Null = 'YES'")
                .fetch_optional(&self.pool)
                .await,
            Ok(Some(_))
        );
        if !thumb_path_nullable {
            tracing::info!("Making audio_thumbnails.thumbnail_path nullable");
            sqlx::query("ALTER TABLE audio_thumbnails MODIFY COLUMN thumbnail_path VARCHAR(512)")
                .execute(&self.pool)
                .await?;

            tracing::info!("Audio thumbnails thumbnail_path is now nullable");
        }

        // Create blog_posts table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS blog_posts (
                id INT AUTO_INCREMENT PRIMARY KEY,
                author_id INT NOT NULL,
                title VARCHAR(255) NOT NULL,
                content TEXT NOT NULL,
                published BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                FOREIGN KEY (author_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create notes table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS notes (
                id INT AUTO_INCREMENT PRIMARY KEY,
                user_id INT NOT NULL,
                title VARCHAR(255) NOT NULL,
                content TEXT NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create clipboard table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS clipboard (
                id INT AUTO_INCREMENT PRIMARY KEY,
                user_id INT NOT NULL,
                content TEXT NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create sessions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id VARCHAR(255) PRIMARY KEY,
                user_id INT NOT NULL,
                refresh_token VARCHAR(255) NOT NULL,
                user_agent TEXT,
                ip_address VARCHAR(45),
                last_active TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
                is_revoked BOOLEAN DEFAULT FALSE,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                expires_at TIMESTAMP NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        tracing::info!("Database migrations completed");
        Ok(())
    }
}
