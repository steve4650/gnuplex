use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use regex::Regex;
use rusqlite::functions::FunctionFlags;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::error::Result;
use crate::models::{FileExtension, MEDIA_TYPE_FILE, MEDIA_TYPE_URL, MediaDir, MediaItem};
use crate::timestamps::{parse_db_time, to_db_time};

const MEDIA_ITEM_COLUMNS: &str =
    "id, created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid";

/// `SELECT` every column `read_media_item` expects from `media_items`.
fn select_media_items(rest: &str) -> String {
    format!("SELECT {MEDIA_ITEM_COLUMNS} FROM media_items {rest}")
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn init(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
        )?;
        // SQLite calls this once per row. Keep the last compiled pattern
        // around, since a query always uses the same pattern for every row.
        let cached = RefCell::new(None);
        conn.create_scalar_function(
            "regexp",
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            move |ctx| {
                let pattern: String = ctx.get(0)?;
                let subject: String = ctx.get(1)?;
                let mut cached = cached.borrow_mut();
                if cached.as_ref().is_none_or(|(cached, _)| *cached != pattern) {
                    *cached = Regex::new(&pattern)
                        .ok()
                        .map(|regex| (pattern.clone(), regex));
                }
                Ok(cached
                    .as_ref()
                    .is_some_and(|(_, regex)| regex.is_match(&subject)))
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
        crate::sync::lock(&self.conn)
    }

    /// Update a MediaItem's LastPlayed attribute to be the current time.
    pub fn update_last_played(&self, id: i64) -> Result<()> {
        let now = to_db_time(&Utc::now());
        self.connection().execute(
            "UPDATE media_items SET last_played = ?, updated_at = ? WHERE id = ?",
            params![now, now, id],
        )?;
        Ok(())
    }

    pub fn get_file_exts(&self) -> Result<Vec<FileExtension>> {
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
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Replace the configured list of file extensions.
    pub fn set_file_exts(&self, extensions: &[String]) -> Result<()> {
        let mut extensions: Vec<String> = extensions
            .iter()
            .map(|extension| extension.to_lowercase())
            .collect();
        extensions.sort_unstable();
        extensions.dedup();

        let now = to_db_time(&Utc::now());
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM file_extensions", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO file_extensions (created_at, updated_at, deleted_at, extension) \
                 VALUES (?, ?, NULL, ?)",
            )?;
            for extension in &extensions {
                stmt.execute(params![now, now, extension])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_media_dirs(&self) -> Result<Vec<MediaDir>> {
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
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Set the configured list of directories to scan for MediaItems.
    pub fn set_media_dirs(&self, media_dirs: &[String]) -> Result<()> {
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
        let wanted: HashSet<&str> = media_dirs.iter().map(String::as_str).collect();
        for dir in media_dirs {
            let now = Utc::now();
            tx.execute(
                "INSERT INTO media_dirs (created_at, updated_at, deleted_at, path, last_scanned) \
                 VALUES (?, ?, NULL, ?, '') ON CONFLICT DO NOTHING",
                params![to_db_time(&now), to_db_time(&now), dir],
            )?;
        }
        for (id, path) in existing {
            if !wanted.contains(path.as_str()) {
                tx.execute("DELETE FROM media_dirs WHERE id = ?", params![id])?;
            }
        }
        Ok(tx.commit()?)
    }

    /// Get the most recent 25 played MediaItems.
    pub fn get_last_25_played(&self) -> Result<Vec<MediaItem>> {
        let conn = self.connection();
        let sql = select_media_items(
            "WHERE last_played != '' AND deleted_at IS NULL \
             ORDER BY last_played desc LIMIT 25",
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], read_media_item)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Get all MediaItems which match the given search string.
    pub fn get_media_items(&self, search: &str, offset: i64) -> Result<(Vec<MediaItem>, i64)> {
        let search = search.trim();
        let conn = self.connection();
        if search.is_empty() {
            let sql =
                select_media_items("WHERE deleted_at IS NULL ORDER BY path LIMIT 1000 OFFSET ?");
            let mut stmt = conn.prepare(&sql)?;
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
        let sql = select_media_items(
            "WHERE path REGEXP ? AND deleted_at IS NULL \
             ORDER BY path LIMIT 1000 OFFSET ?",
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![pattern, offset], read_media_item)?;
        let items = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM media_items WHERE path REGEXP ? AND deleted_at IS NULL",
            params![pattern],
            |row| row.get(0),
        )?;
        Ok((items, count))
    }

    pub fn get_media_items_by_paths(&self, paths: &[String]) -> Result<Vec<MediaItem>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", paths.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = select_media_items(&format!(
            "WHERE path IN ({placeholders}) AND deleted_at IS NULL"
        ));
        let conn = self.connection();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(paths.iter()), read_media_item)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Get a MediaItem by primary key.
    pub fn get_media_item_by_id(&self, id: u64) -> Result<Option<MediaItem>> {
        let id = i64::try_from(id).map_err(|_| {
            crate::error::Error::BadRequest(format!("media item id {id} is too large"))
        })?;
        let conn = self.connection();
        let sql = select_media_items("WHERE id = ? AND deleted_at IS NULL ORDER BY id LIMIT 1");
        conn.query_row(&sql, params![id], read_media_item)
            .optional()
            .map_err(Into::into)
    }

    /// Get a MediaItem by path.
    pub fn get_media_item_by_path(&self, path: &str) -> Result<Option<MediaItem>> {
        let conn = self.connection();
        let sql = select_media_items("WHERE path = ? AND deleted_at IS NULL ORDER BY id LIMIT 1");
        conn.query_row(&sql, params![path], read_media_item)
            .optional()
            .map_err(Into::into)
    }

    pub fn delete_media_item_files_not_matching_uuid(&self, uuid: &str) -> Result<()> {
        self.connection().execute(
            "DELETE FROM media_items WHERE last_scan_uuid != ? AND type = ?",
            params![uuid, MEDIA_TYPE_FILE],
        )?;
        Ok(())
    }

    pub fn add_media_item_file_batch(&self, paths: &[String], last_scan_uuid: &str) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        for chunk in paths.chunks(100) {
            let now = Utc::now();
            let created = to_db_time(&now);
            let updated = to_db_time(&now);
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
        Ok(tx.commit()?)
    }

    pub fn add_media_item_url(&self, url: &str) -> Result<()> {
        let now = Utc::now();
        self.connection().execute(
            "INSERT INTO media_items \
             (created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid) \
             VALUES (?, ?, NULL, ?, '', 0, ?, '') ON CONFLICT DO NOTHING",
            params![to_db_time(&now), to_db_time(&now), url, MEDIA_TYPE_URL],
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
/// Whitespace separates terms, which may be interspersed with any text.
pub fn build_search_regex(search: &str) -> String {
    let fields: Vec<String> = search.split_whitespace().map(regex::escape).collect();
    if fields.is_empty() {
        return String::new();
    }
    format!("(?i){}", fields.join(".*"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timestamps::rfc3339_string;

    #[test]
    fn build_search_regex_escapes_terms() {
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
        let dir = tempfile::tempdir().unwrap();
        let db = Db::init(dir.path().join("test.db")).unwrap();
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
                    params![to_db_time(&now), to_db_time(&now), path],
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
    }

    #[test]
    fn set_media_dirs_reconciles_existing_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::init(dir.path().join("test.db")).unwrap();
        db.set_media_dirs(&["/b".to_string(), "/a".to_string()])
            .unwrap();
        let paths: Vec<String> = db
            .get_media_dirs()
            .unwrap()
            .into_iter()
            .map(|media_dir| media_dir.path)
            .collect();
        assert_eq!(paths, ["/a", "/b"]);

        db.set_media_dirs(&["/b".to_string()]).unwrap();
        let paths: Vec<String> = db
            .get_media_dirs()
            .unwrap()
            .into_iter()
            .map(|media_dir| media_dir.path)
            .collect();
        assert_eq!(paths, ["/b"]);
    }

    #[test]
    fn set_file_exts_normalizes_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::init(dir.path().join("test.db")).unwrap();
        db.set_file_exts(&["MKV".to_string(), "mp4".to_string(), "mkv".to_string()])
            .unwrap();
        let extensions: Vec<String> = db
            .get_file_exts()
            .unwrap()
            .into_iter()
            .map(|extension| extension.extension)
            .collect();
        assert_eq!(extensions, ["mkv", "mp4"]);

        db.set_file_exts(&[]).unwrap();
        assert!(db.get_file_exts().unwrap().is_empty());
    }

    #[test]
    fn add_and_delete_files() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::init(dir.path().join("test.db")).unwrap();
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
    }

    #[test]
    fn legacy_gorm_database_boots_and_parses_timestamps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                r#"
CREATE TABLE `media_items` (`id` integer PRIMARY KEY AUTOINCREMENT,`created_at` datetime,`updated_at` datetime,`deleted_at` datetime,`path` text,`last_played` text,`temp` numeric DEFAULT false,`type` integer,`last_scan_uuid` text);
CREATE UNIQUE INDEX `idx_media_items_path` ON `media_items`(`path`);
INSERT INTO media_items (created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid)
VALUES ('2021-05-01 12:34:56.789+00:00', '2021-05-01 12:34:57.5+00:00', NULL, '/media/legacy.mkv', '2022-01-02 03:04:05.25+00:00', 0, 1, 'scan-legacy');
INSERT INTO media_items (created_at, updated_at, deleted_at, path, last_played, temp, type, last_scan_uuid)
VALUES ('2021-05-02 01:02:03', '2021-05-02 01:02:04', '2021-05-03 05:06:07', '/media/deleted.mkv', '', 0, 1, 'scan-legacy');
CREATE TABLE `media_dirs` (`id` integer PRIMARY KEY AUTOINCREMENT,`created_at` datetime,`updated_at` datetime,`deleted_at` datetime,`path` text,`last_scanned` text);
INSERT INTO media_dirs (created_at, updated_at, path, last_scanned)
VALUES ('2021-05-01 12:34:56.789+00:00', '2021-05-01 12:34:56.789+00:00', '/media', '2021-05-01T12:34:56Z');
CREATE TABLE `file_extensions` (`id` integer PRIMARY KEY AUTOINCREMENT,`created_at` datetime,`updated_at` datetime,`deleted_at` datetime,`extension` text);
INSERT INTO file_extensions (created_at, updated_at, extension)
VALUES ('2021-05-01 12:34:56.789+00:00', '2021-05-01 12:34:56.789+00:00', 'mkv');
"#,
            )
            .unwrap();
        }

        let db = Db::init(&path).unwrap();

        let (items, count) = db.get_media_items("", 0).unwrap();
        assert_eq!(count, 1);
        let item = &items[0];
        assert_eq!(item.path, "/media/legacy.mkv");
        assert_eq!(rfc3339_string(&item.created_at), "2021-05-01T12:34:56.789Z");
        assert_eq!(rfc3339_string(&item.updated_at), "2021-05-01T12:34:57.5Z");
        assert_eq!(item.deleted_at, None);
        assert_eq!(item.last_played, "2022-01-02 03:04:05.25+00:00");

        let dirs = db.get_media_dirs().unwrap();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].path, "/media");
        assert_eq!(
            rfc3339_string(&dirs[0].created_at),
            "2021-05-01T12:34:56.789Z"
        );

        let exts = db.get_file_exts().unwrap();
        assert_eq!(exts.len(), 1);
        assert_eq!(exts[0].extension, "mkv");
    }
}
