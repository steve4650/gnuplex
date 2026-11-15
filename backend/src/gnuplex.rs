use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use tracing::warn;
use uuid::Uuid;

use crate::db::Db;
use crate::error::{Error, Result};
use crate::models::{MediaItem, Screenshot, Track};
use crate::mpv::Mpv;
use crate::screenshots;
use crate::subtitles;

/// Flush the pending scan results after this many paths, so a large library
/// does not have to fit in memory all at once.
const SCAN_BATCH_SIZE: usize = 1000;

pub struct Gnuplex {
    pub db: Db,
    pub mpv: Arc<Mpv>,
    pub screenshots_dir: PathBuf,
    pub static_files: PathBuf,
    pub exe: PathBuf,
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

    /// Update GNUPlex's stored MediaItems using its configured MediaDirs.
    pub fn scan_lib(&self) -> Result<()> {
        let media_dirs = self.db.get_media_dirs()?;
        let file_extensions: HashSet<String> = self
            .db
            .get_file_exts()?
            .into_iter()
            .map(|extension| extension.extension)
            .collect();

        let scan_id = Uuid::new_v4().to_string();
        let mut batch = Vec::new();
        for media_dir in &media_dirs {
            match std::fs::metadata(&media_dir.path) {
                Ok(metadata) if metadata.is_dir() => {
                    walk_dir(Path::new(&media_dir.path), &file_extensions, &mut batch);
                }
                Ok(_) => warn!("skipping {} - not a directory", media_dir.path),
                Err(err) => warn!(
                    "skipping {} - could not stat this directory: {err}",
                    media_dir.path
                ),
            }
            if batch.len() >= SCAN_BATCH_SIZE {
                self.db.add_media_item_file_batch(&batch, &scan_id)?;
                batch.clear();
            }
        }
        if !batch.is_empty() {
            self.db.add_media_item_file_batch(&batch, &scan_id)?;
        }
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
        let mut by_path: HashMap<String, MediaItem> = self
            .db
            .get_media_items_by_paths(&paths)?
            .into_iter()
            .map(|item| (item.path.clone(), item))
            .collect();

        let ordered: Vec<MediaItem> = playlist
            .into_iter()
            .filter_map(|entry| {
                let mut item = by_path.remove(&entry.filename)?;
                item.queue_id = Some(entry.id);
                Some(item)
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
    pub fn cast(&self, url: &str, temp_url: bool, play_next: bool, play_last: bool) -> Result<()> {
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
        subtitles::shift_subtitle_file(&filename, -delay)?;
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

fn walk_dir(dir: &Path, file_extensions: &HashSet<String>, batch: &mut Vec<String>) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) => {
            warn!(
                "Error 62b2b0e2-25d7-49db-a31a-5785c145229f: file read issue for {}: {err}",
                dir.display()
            );
            return;
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
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        // Directory symlinks are intentionally not followed.
        let is_dir = match entry.file_type() {
            Ok(file_type) => file_type.is_dir(),
            Err(err) => {
                warn!(
                    "Error 62b2b0e2-25d7-49db-a31a-5785c145229f: could not read the file type for {}: {err}",
                    path.display()
                );
                continue;
            }
        };
        if is_dir {
            walk_dir(&path, file_extensions, batch);
            continue;
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if !file_extensions.contains(&extension) {
            batch.push(path.to_string_lossy().into_owned());
        }
    }
}
