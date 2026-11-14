use std::sync::Mutex;

use chrono::{DateTime, Utc};
use regex::Regex;
use rusqlite::functions::FunctionFlags;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::gotime::{db_time_string, parse_db_time};
use crate::models::{FileExtension, MEDIA_TYPE_FILE, MEDIA_TYPE_URL, MediaDir, MediaItem};

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn init(path: &str, _verbose: bool) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.create_scalar_function(
            "regexp",
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| {
                let pattern: String = ctx.get(0)?;
                let subject: String = ctx.get(1)?;
                Ok(Regex::new(&pattern)
                    .map(|re| re.is_match(&subject))
                    .unwrap_or(false))
            },
        )?;
        conn.execute_batch(
            r#"
CREATE TABLE IF NOT EXISTS `media_items` (`id` integer PRIMARY KEY AUTOINCREMENT,`created_at` datetime,`updated_at` datetime,`deleted_at` datetime,`path` text,`last_played` text,`temp` numeric DEFAULT false,`type` integer,`last_scan_uuid` text);
CREATE UNIQUE INDEX IF NOT EXISTS `idx_media_items_path` ON `media_items`(`path`);
CREATE INDEX IF NOT EXISTS `idx_media_items_last_played` ON `media_items`(`last_played` desc);
CREATE INDEX IF NOT EXISTS `idx_media_items_last_scan_uuid` ON `media_items`(`last_scan_uuid`);
CREATE INDEX IF NOT EXISTS `idx_media_items_deleted_at` ON `media_items`(`deleted_at`);
CREATE TABLE IF NOT EXISTS `media_dirs` (`id` integer PRIMARY KEY AUTOINCREMENT,`created_at` datetime,`updated_at` datetime,`deleted_at` datetime,`path` text,`last_scanned` text);
CREATE UNIQUE INDEX IF NOT EXISTS `idx_media_dirs_path` ON `media_dirs`(`path`);
CREATE INDEX IF NOT EXISTS `idx_media_dirs_last_scanned` ON `media_dirs`(`last_scanned` desc);
CREATE INDEX IF NOT EXISTS `idx_media_dirs_deleted_at` ON `media_dirs`(`deleted_at`);
CREATE TABLE IF NOT EXISTS `file_extensions` (`id` integer PRIMARY KEY AUTOINCREMENT,`created_at` datetime,`updated_at` datetime,`deleted_at` datetime,`extension` text);
CREATE INDEX IF NOT EXISTS `idx_file_extensions_deleted_at` ON `file_extensions`(`deleted_at`);
"#,
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Update a MediaItem's LastPlayed attribute to be the current time.
    pub fn update_last_played(&self, id: i64) -> rusqlite::Result<()> {
        let now = Utc::now();
        self.connection().execute(
            "UPDATE media_items SET last_played = ?, updated_at = ? WHERE id = ?",
            params![
                now.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                db_time_string(&now),
                id
            ],
        )?;
        Ok(())
    }

    pub fn get_file_exts(&self) -> rusqlite::Result<Vec<FileExtension>> {
        let conn = self.connection();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, updated_at, deleted_at, extension FROM file_extensions WHERE deleted_at IS NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(FileExtension {
                id: row.get(0)?,
                created_at: read_time(row.get(1)?),
                updated_at: read_time(row.get(2)?),
                deleted_at: read_opt_time(row.get(3)?),
                extension: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Replace the configured list of file extensions.
    pub fn set_file_exts(&self, extensions: &[String]) -> rusqlite::Result<()> {
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM file_extensions WHERE 1 = 1", [])?;
        for extension in extensions {
            let now = Utc::now();
            tx.execute(
                "INSERT INTO file_extensions (created_at, updated_at, deleted_at, extension) \
                 VALUES (?, ?, NULL, ?) ON CONFLICT DO NOTHING",
                params![
                    db_time_string(&now),
                    db_time_string(&now),
                    extension.to_lowercase()
                ],
            )?;
        }
        tx.commit()
    }

    pub fn get_media_dirs(&self) -> rusqlite::Result<Vec<MediaDir>> {
        let conn = self.connection();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, updated_at, deleted_at, path, last_scanned \
             FROM media_dirs WHERE deleted_at IS NULL ORDER BY path",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MediaDir {
                id: row.get(0)?,
                created_at: read_time(row.get(1)?),
                updated_at: read_time(row.get(2)?),
                deleted_at: read_opt_time(row.get(3)?),
                path: row.get(4)?,
                last_scanned: row.get(5)?,
            })
        })?;
        rows.collect()
    }

    /// Set the configured list of directories to scan for MediaItems.
    pub fn set_media_dirs(&self, media_dirs: &[String]) -> rusqlite::Result<()> {
        let mut conn = self.connection();
        let existing = {
            let mut stmt = conn.prepare(
                "SELECT id, path FROM media_dirs WHERE deleted_at IS NULL ORDER BY path",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let tx = conn.transaction()?;
        let wanted: std::collections::HashSet<&str> =
            media_dirs.iter().map(String::as_str).collect();
        for dir in media_dirs {
            let now = Utc::now();
            tx.execute(
                "INSERT INTO media_dirs (created_at, updated_at, deleted_at, path, last_scanned) \
                 VALUES (?, ?, NULL, ?, '') ON CONFLICT DO NOTHING",
                params![db_time_string(&now), db_time_string(&now), dir],
            )?;
        }
        for (id, path) in existing {
            if !wanted.contains(path.as_str()) {
                tx.execute("DELETE FROM media_dirs WHERE id = ?", params![id])?;
            }
        }
        tx.commit()
    }

    /// Get the most recent 25 played MediaItems.
    pub fn get_last_25_played(&self) -> rusqlite::Result<Vec<MediaItem>> {
        let conn = self.connection();
        let mut stmt = conn.prepare(
            "SELECT id, created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid \
             FROM media_items WHERE last_played != '' AND deleted_at IS NULL \
             ORDER BY last_played desc LIMIT 25",
        )?;
        let rows = stmt.query_map([], read_media_item)?;
        rows.collect()
    }

    /// Get all MediaItems which match the given search string.
    pub fn get_media_items(
        &self,
        search: &str,
        offset: i64,
    ) -> rusqlite::Result<(Vec<MediaItem>, i64)> {
        let search = search.trim();
        let conn = self.connection();
        if search.is_empty() {
            let mut stmt = conn.prepare(
                "SELECT id, created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid \
                 FROM media_items WHERE deleted_at IS NULL ORDER BY path LIMIT 1000 OFFSET ?",
            )?;
            let rows = stmt.query_map(params![offset], read_media_item)?;
            let items = rows.collect::<rusqlite::Result<Vec<_>>>()?;
            let count: i64 = conn.query_row(
                "SELECT count(*) FROM media_items WHERE deleted_at IS NULL",
                [],
                |row| row.get(0),
            )?;
            return Ok((items, count));
        }

        let pattern = build_search_regex(search);
        let mut stmt = conn.prepare(
            "SELECT id, created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid \
             FROM media_items WHERE path REGEXP ? AND deleted_at IS NULL \
             ORDER BY path LIMIT 1000 OFFSET ?",
        )?;
        let rows = stmt.query_map(params![pattern, offset], read_media_item)?;
        let items = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM media_items WHERE path REGEXP ? AND deleted_at IS NULL",
            params![pattern],
            |row| row.get(0),
        )?;
        Ok((items, count))
    }

    pub fn get_media_items_by_paths(&self, paths: &[String]) -> rusqlite::Result<Vec<MediaItem>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", paths.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid \
             FROM media_items WHERE path IN ({placeholders}) AND deleted_at IS NULL"
        );
        let conn = self.connection();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(paths.iter()), read_media_item)?;
        rows.collect()
    }

    /// Get a MediaItem by primary key, as GORM's `First(&item, id)` does.
    pub fn get_media_item_by_id(&self, id: u64) -> rusqlite::Result<Option<MediaItem>> {
        let conn = self.connection();
        conn.query_row(
            "SELECT id, created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid \
             FROM media_items WHERE id = ? AND deleted_at IS NULL ORDER BY id LIMIT 1",
            params![id as i64],
            read_media_item,
        )
        .optional()
    }

    /// Get a MediaItem by path, as GORM's `First(&item, "path = ?", path)` does.
    pub fn get_media_item_by_path(&self, path: &str) -> rusqlite::Result<Option<MediaItem>> {
        let conn = self.connection();
        conn.query_row(
            "SELECT id, created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid \
             FROM media_items WHERE path = ? AND deleted_at IS NULL ORDER BY id LIMIT 1",
            params![path],
            read_media_item,
        )
        .optional()
    }

    pub fn delete_media_item_files_not_matching_uuid(&self, uuid: &str) -> rusqlite::Result<()> {
        self.connection().execute(
            "DELETE FROM media_items WHERE last_scan_uuid != ? AND type = ?",
            params![uuid, MEDIA_TYPE_FILE],
        )?;
        Ok(())
    }

    pub fn add_media_item_file_batch(
        &self,
        paths: &[String],
        last_scan_uuid: &str,
    ) -> rusqlite::Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        for chunk in paths.chunks(100) {
            let now = Utc::now();
            let created = db_time_string(&now);
            let updated = db_time_string(&now);
            let mut values = Vec::with_capacity(chunk.len() * 8);
            let mut sql = String::from(
                "INSERT INTO media_items \
                 (created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid) VALUES ",
            );
            for (i, path) in chunk.iter().enumerate() {
                if i != 0 {
                    sql.push(',');
                }
                sql.push_str("(?, ?, NULL, ?, '', 0, ?, ?)");
                values.push(Value::Text(created.clone()));
                values.push(Value::Text(updated.clone()));
                values.push(Value::Text(path.clone()));
                values.push(Value::Integer(MEDIA_TYPE_FILE));
                values.push(Value::Text(last_scan_uuid.to_string()));
            }
            sql.push_str(
                " ON CONFLICT (path) DO UPDATE SET last_scan_uuid = excluded.last_scan_uuid",
            );
            tx.execute(&sql, params_from_iter(values))?;
        }
        tx.commit()
    }

    pub fn add_media_item_url(&self, url: &str) -> rusqlite::Result<()> {
        let now = Utc::now();
        self.connection().execute(
            "INSERT INTO media_items \
             (created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid) \
             VALUES (?, ?, NULL, ?, '', 0, ?, '') ON CONFLICT DO NOTHING",
            params![
                db_time_string(&now),
                db_time_string(&now),
                url,
                MEDIA_TYPE_URL
            ],
        )?;
        Ok(())
    }
}

fn read_time(value: Option<String>) -> DateTime<Utc> {
    value
        .as_deref()
        .and_then(parse_db_time)
        .unwrap_or(DateTime::UNIX_EPOCH)
}

fn read_opt_time(value: Option<String>) -> Option<DateTime<Utc>> {
    value.as_deref().and_then(parse_db_time)
}

fn read_media_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<MediaItem> {
    Ok(MediaItem {
        id: row.get(0)?,
        created_at: read_time(row.get(1)?),
        updated_at: read_time(row.get(2)?),
        deleted_at: read_opt_time(row.get(3)?),
        path: row.get(4)?,
        last_played: row.get(5)?,
        temp: row.get::<_, i64>(6)? != 0,
        media_type: row.get(7)?,
        last_scan_uuid: row.get(8)?,
        queue_id: None,
    })
}

/// Convert a user search string into a safe, case-insensitive regex pattern.
/// Successive spaces in search turn into .* in regex, and special regex
/// characters are escaped exactly like Go's regexp.QuoteMeta.
pub fn build_search_regex(search: &str) -> String {
    let fields: Vec<String> = search
        .split_whitespace()
        .map(quote_meta)
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return String::new();
    }
    format!("(?i){}", fields.join(".*"))
}

fn quote_meta(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    for ch in field.chars() {
        if matches!(
            ch,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MediaItem as Item;

    #[test]
    fn build_search_regex_matches_go() {
        let tests = [
            ("", ""),
            ("   ", ""),
            ("sopranos s02", "(?i)sopranos.*s02"),
            ("sopranos   s02", "(?i)sopranos.*s02"),
            ("  sopranos   s02  ", "(?i)sopranos.*s02"),
            ("sopranos (1999) s02", r"(?i)sopranos.*\(1999\).*s02"),
            ("[1080p] movie", r"(?i)\[1080p\].*movie"),
            (".* test +", r"(?i)\.\*.*test.*\+"),
        ];
        for (input, expected) in tests {
            assert_eq!(build_search_regex(input), expected);
        }
    }

    #[test]
    fn media_items_queries_work() {
        let dir = std::env::temp_dir().join(format!("gnuplex-db-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::init(dir.join("test.db").to_str().unwrap(), false).unwrap();
        let paths = [
            "/media/Sopranos (1999) -    S02E01.mkv",
            "/media/Sopranos S01E01.mkv",
            "/media/The Wire S01E01.mkv",
            "/media/Movie [1080p].mp4",
            "/media/Show (2020) S01.mkv",
        ];
        let now = Utc::now();
        {
            let conn = db.connection();
            for path in paths {
                conn.execute(
                    "INSERT INTO media_items (created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid) \
                     VALUES (?, ?, NULL, ?, '', 0, 1, '')",
                    params![db_time_string(&now), db_time_string(&now), path],
                )
                .unwrap();
            }
        }
        let tests = [
            ("sopranos s02", 1, "/media/Sopranos (1999) -    S02E01.mkv"),
            (
                "sopranos    s02",
                1,
                "/media/Sopranos (1999) -    S02E01.mkv",
            ),
            ("SOPRANOS S02", 1, "/media/Sopranos (1999) -    S02E01.mkv"),
            ("sopranos", 2, "/media/Sopranos (1999) -    S02E01.mkv"),
            ("[1080p]", 1, "/media/Movie [1080p].mp4"),
            ("show (2020)", 1, "/media/Show (2020) S01.mkv"),
            (".*", 0, ""),
            ("", 5, "/media/Movie [1080p].mp4"),
            ("   ", 5, "/media/Movie [1080p].mp4"),
        ];
        for (search, count, first) in tests {
            let (items, got_count) = db.get_media_items(search, 0).unwrap();
            assert_eq!(got_count, count, "search {search:?}");
            if count > 0 {
                assert_eq!(items[0].path, first, "search {search:?}");
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_and_delete_files() {
        let dir = std::env::temp_dir().join(format!("gnuplex-db-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::init(dir.join("test.db").to_str().unwrap(), false).unwrap();
        db.add_media_item_file_batch(&["/a.mkv".to_string(), "/b.mkv".to_string()], "scan-1")
            .unwrap();
        db.add_media_item_file_batch(&["/a.mkv".to_string()], "scan-2")
            .unwrap();
        let items = db.get_media_items("", 0).unwrap().0;
        assert_eq!(items.len(), 2);
        assert_eq!(
            items
                .iter()
                .find(|i| i.path == "/a.mkv")
                .unwrap()
                .last_scan_uuid,
            "scan-2"
        );
        db.delete_media_item_files_not_matching_uuid("scan-2")
            .unwrap();
        let items = db.get_media_items("", 0).unwrap().0;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, "/a.mkv");
        let _ = Item {
            id: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
            path: String::new(),
            last_played: String::new(),
            temp: false,
            media_type: MEDIA_TYPE_FILE,
            last_scan_uuid: String::new(),
            queue_id: None,
        };
        std::fs::remove_dir_all(&dir).ok();
    }
}
