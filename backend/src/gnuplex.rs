use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use tracing::warn;
use uuid::Uuid;

use crate::db::Db;
use crate::error::{Error, Result};
use crate::models::{FileExtension, MEDIA_TYPE_URL, MediaDir, MediaItem, Screenshot, Track};
use crate::mpv::Mpv;
use crate::screenshots;
use crate::subtitles;

/// Flush the pending scan results after this many paths, so a large library
/// does not have to fit in memory all at once.
const SCAN_BATCH_SIZE: usize = 1000;

/// Buffers discovered paths and writes them to the database in chunks.
struct ScanBatch<'a> {
    db: &'a Db,
    scan_id: &'a str,
    paths: Vec<String>,
}

impl<'a> ScanBatch<'a> {
    fn new(db: &'a Db, scan_id: &'a str) -> Self {
        Self {
            db,
            scan_id,
            paths: Vec::with_capacity(SCAN_BATCH_SIZE),
        }
    }

    fn push(&mut self, path: String) -> Result<()> {
        self.paths.push(path);
        if self.paths.len() >= SCAN_BATCH_SIZE {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if !self.paths.is_empty() {
            self.db
                .add_media_item_file_batch(&self.paths, self.scan_id)?;
            self.paths.clear();
        }
        Ok(())
    }
}

pub struct Gnuplex {
    db: Db,
    mpv: Arc<Mpv>,
    screenshots_dir: PathBuf,
    static_files: PathBuf,
    exe: PathBuf,
}

impl Gnuplex {
    pub fn new(
        db: Db,
        mpv: Arc<Mpv>,
        screenshots_dir: PathBuf,
        static_files: PathBuf,
        exe: PathBuf,
    ) -> Self {
        Self {
            db,
            mpv,
            screenshots_dir,
            static_files,
            exe,
        }
    }

    pub fn mpv(&self) -> &Arc<Mpv> {
        &self.mpv
    }

    pub fn screenshots_dir(&self) -> &Path {
        &self.screenshots_dir
    }

    pub fn static_files(&self) -> &Path {
        &self.static_files
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    pub fn media_dirs(&self) -> Result<Vec<MediaDir>> {
        self.db.get_media_dirs()
    }

    pub fn set_media_dirs(&self, media_dirs: &[String]) -> Result<()> {
        self.db.set_media_dirs(media_dirs)
    }

    pub fn file_exts(&self) -> Result<Vec<FileExtension>> {
        self.db.get_file_exts()
    }

    pub fn set_file_exts(&self, extensions: &[String]) -> Result<()> {
        self.db.set_file_exts(extensions)
    }

    pub fn last_25_played(&self) -> Result<Vec<MediaItem>> {
        self.db.get_last_25_played()
    }

    pub fn media_items(&self, search: &str, offset: i64) -> Result<(Vec<MediaItem>, i64)> {
        self.db.get_media_items(search, offset)
    }

    /// Update GNUPlex's stored MediaItems using its configured MediaDirs.
    pub fn scan_lib(&self) -> Result<()> {
        let media_dirs = self.db.get_media_dirs()?;
        let file_extensions: HashSet<String> = self
            .db
            .get_file_exts()?
            .into_iter()
            .map(|extension| extension.extension.trim_start_matches('.').to_string())
            .collect();

        let scan_id = Uuid::new_v4().to_string();
        let mut batch = ScanBatch::new(&self.db, &scan_id);
        for media_dir in &media_dirs {
            match std::fs::metadata(&media_dir.path) {
                Ok(metadata) if metadata.is_dir() => {
                    walk_dir(Path::new(&media_dir.path), &file_extensions, &mut batch)?;
                }
                Ok(_) => warn!("skipping {} - not a directory", media_dir.path),
                Err(err) => warn!(
                    "skipping {} - could not stat this directory: {err}",
                    media_dir.path
                ),
            }
        }
        batch.flush()?;
        self.db.delete_media_item_files_not_matching_uuid(&scan_id)
    }

    /// Returns the currently playing MediaItems, or None when nothing is
    /// loaded.
    pub fn get_now_playing(&self) -> Result<Option<Vec<MediaItem>>> {
        let playlist = match self.mpv.get_now_playing() {
            Ok(playlist) => playlist,
            Err(Error::PropertyUnavailable) => return Ok(None),
            Err(err) => return Err(err),
        };
        let paths: Vec<String> = playlist
            .iter()
            .map(|entry| entry.filename.clone())
            .collect();
        let by_path: HashMap<String, MediaItem> = self
            .db
            .get_media_items_by_paths(&paths)?
            .into_iter()
            .map(|item| (item.path.clone(), item))
            .collect();

        let ordered: Vec<MediaItem> = playlist
            .into_iter()
            .map(|entry| {
                if let Some(item) = by_path.get(&entry.filename) {
                    let mut item = item.clone();
                    item.queue_id = Some(entry.id);
                    item
                } else {
                    MediaItem {
                        id: 0,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        deleted_at: None,
                        path: entry.filename.clone(),
                        last_played: String::new(),
                        temp: true,
                        media_type: MEDIA_TYPE_URL,
                        last_scan_uuid: String::new(),
                        queue_id: Some(entry.id),
                    }
                }
            })
            .collect();
        if ordered.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ordered))
        }
    }

    /// Play a MediaItem from the library by ID.
    pub fn play_by_id(&self, id: u64, play_next: bool, play_last: bool) -> Result<()> {
        let media_item = self
            .db
            .get_media_item_by_id(id)?
            .ok_or_else(|| Error::NotFound(format!("media item {id}")))?;
        self.mpv
            .set_now_playing(&media_item.path, play_next, play_last)?;
        self.db.update_last_played(media_item.id)
    }

    /// Play a MediaItem by path or URL.
    fn play_by_path(
        &self,
        path: &str,
        temp_url: bool,
        play_next: bool,
        play_last: bool,
    ) -> Result<()> {
        if temp_url {
            return self.mpv.set_now_playing(path, true, false);
        }
        let media_item = self
            .db
            .get_media_item_by_path(path)?
            .ok_or_else(|| Error::NotFound(path.to_string()))?;
        self.mpv
            .set_now_playing(&media_item.path, play_next, play_last)?;
        self.db.update_last_played(media_item.id)
    }

    /// Cast a URL to the media player. `temp` controls whether or not it is
    /// added to the library.
    pub fn cast(
        &self,
        url: &str,
        temp_url: bool,
        mut play_next: bool,
        play_last: bool,
    ) -> Result<()> {
        if !play_next && !play_last {
            play_next = true;
        }
        if temp_url {
            self.play_by_path(url, true, play_next, play_last)
        } else {
            self.db.add_media_item_url(url)?;
            self.play_by_path(url, false, play_next, play_last)
        }
    }

    /// Get the currently available subtitle tracks, or None when there are no
    /// subtitle tracks.
    pub fn get_subs(&self) -> Result<Option<Vec<Track>>> {
        let subs: Vec<Track> = self
            .mpv
            .get_tracks()?
            .into_iter()
            .filter(|track| track.track_type == "sub")
            .collect();
        if subs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(subs))
        }
    }

    pub fn set_sub_visibility(&self, visible: bool) -> Result<()> {
        self.mpv.set_sub_visibility(visible)
    }

    pub fn set_sub_track(&self, track_id: i64) -> Result<()> {
        self.mpv.set_sub_track(track_id)
    }

    /// Save subtitle delay permanently to the subtitle file.
    pub fn save_sub_delay(&self) -> Result<()> {
        let delay = self.mpv.get_sub_delay()?;
        if delay == 0.0 {
            return Ok(());
        }
        let filename = self.mpv.get_current_sub_filename()?;
        if filename.is_empty() {
            return Ok(());
        }
        subtitles::shift_subtitle_file(Path::new(&filename), -delay)?;
        self.mpv.sub_reload()?;
        self.mpv.set_sub_delay(0.0)
    }

    pub fn take_screenshot(&self) -> Result<Screenshot> {
        screenshots::take_screenshot(&self.mpv, &self.screenshots_dir, Utc::now())
    }

    pub fn list_recent_screenshots(&self, limit: i64) -> Result<Vec<Screenshot>> {
        screenshots::list_recent_screenshots(&self.screenshots_dir, limit)
    }
}

fn walk_dir(
    dir: &Path,
    file_extensions: &HashSet<String>,
    batch: &mut ScanBatch<'_>,
) -> Result<()> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) => {
            warn!(
                "Error 62b2b0e2-25d7-49db-a31a-5785c145229f: file read issue for {}: {err}",
                dir.display()
            );
            return Ok(());
        }
    };

    let mut entries: Vec<_> = read_dir
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(err) => {
                warn!(
                    "Error 62b2b0e2-25d7-49db-a31a-5785c145229f: could not read an entry in {}: {err}",
                    dir.display()
                );
                None
            }
        })
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                warn!(
                    "Error 62b2b0e2-25d7-49db-a31a-5785c145229f: could not read the file type for {}: {err}",
                    path.display()
                );
                continue;
            }
        };
        if file_type.is_dir() {
            walk_dir(&path, file_extensions, batch)?;
            continue;
        }
        if file_type.is_symlink() {
            match std::fs::metadata(&path) {
                Ok(metadata) if metadata.is_dir() => {
                    // Directory symlinks are intentionally not followed.
                    continue;
                }
                Ok(metadata) if metadata.is_file() => {}
                _ => continue,
            }
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if !file_extensions.contains(&extension) {
            batch.push(path.to_string_lossy().into_owned())?;
        }
    }
    Ok(())
}
