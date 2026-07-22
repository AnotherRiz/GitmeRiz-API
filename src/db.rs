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
