use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use tracing::warn;
use uuid::Uuid;

use crate::db::Db;
use crate::models::{MediaItem, Screenshot, Track};
use crate::mpv::Mpv;
use crate::screenshots;
use crate::subtitles;

pub struct Gnuplex {
    pub db: Db,
    pub mpv: Arc<Mpv>,
    pub screenshots_dir: PathBuf,
    pub static_files: PathBuf,
    pub exe: String,
    play_queue: Mutex<Vec<MediaItem>>,
}

impl Gnuplex {
    pub fn new(
        db: Db,
        mpv: Arc<Mpv>,
        screenshots_dir: PathBuf,
        static_files: PathBuf,
        exe: String,
    ) -> Self {
        Self {
            db,
            mpv,
            screenshots_dir,
            static_files,
            exe,
            play_queue: Mutex::new(Vec::new()),
        }
    }

    /// Update GNUPlex's stored MediaItems using its configured MediaDirs.
    pub fn scan_lib(&self) -> Result<(), String> {
        let media_dirs = self.db.get_media_dirs().map_err(|e| e.to_string())?;
        let file_exts = self.db.get_file_exts().map_err(|e| e.to_string())?;
        let file_ext_h: HashSet<String> = file_exts.into_iter().map(|e| e.extension).collect();

        let last_scan_uuid = Uuid::new_v4().to_string();
        let mut batch: Vec<String> = Vec::new();
        for (i, media_dir) in media_dirs.iter().enumerate() {
            if i % 100 == 0 && i != 0 {
                self.db
                    .add_media_item_file_batch(&batch, &last_scan_uuid)
                    .map_err(|e| e.to_string())?;
            }
            match std::fs::metadata(&media_dir.path) {
                Ok(metadata) if metadata.is_dir() => {
                    walk_dir(Path::new(&media_dir.path), &file_ext_h, &mut batch);
                }
                Ok(_) => {}
                Err(err) => {
                    warn!(
                        "skipping {} - could not stat this directory: {err}",
                        media_dir.path
                    );
                }
            }
        }
        if !batch.is_empty() {
            self.db
                .add_media_item_file_batch(&batch, &last_scan_uuid)
                .map_err(|e| e.to_string())?;
        }
        self.db
            .delete_media_item_files_not_matching_uuid(&last_scan_uuid)
            .map_err(|e| e.to_string())
    }

    /// Returns the currently playing MediaItems, or None for JSON `null`.
    pub fn get_now_playing(&self) -> Result<Option<Vec<MediaItem>>, String> {
        let playlist = match self.mpv.get_now_playing() {
            Ok(playlist) => playlist,
            Err(err) if err == "property unavailable" => return Ok(None),
            Err(err) => return Err(err),
        };
        let paths: Vec<String> = playlist.iter().map(|e| e.filename.clone()).collect();
        let media_items = self
            .db
            .get_media_items_by_paths(&paths)
            .map_err(|e| e.to_string())?;
        let by_path: std::collections::HashMap<String, MediaItem> = media_items
            .into_iter()
            .map(|item| (item.path.clone(), item))
            .collect();
        let mut ordered = Vec::new();
        for entry in playlist {
            if let Some(item) = by_path.get(&entry.filename) {
                let mut item = item.clone();
                item.queue_id = Some(entry.id);
                ordered.push(item);
            }
        }
        if ordered.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ordered))
        }
    }

    /// Play a MediaItem from the library by ID.
    pub fn play_by_id(&self, id: u64, play_next: bool, play_last: bool) -> Result<(), String> {
        let media_item = self
            .db
            .get_media_item_by_id(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "record not found".to_string())?;
        *self.play_queue.lock().unwrap_or_else(|e| e.into_inner()) = vec![media_item.clone()];
        self.mpv
            .set_now_playing(&media_item.path, play_next, play_last)?;
        self.db
            .update_last_played(media_item.id)
            .map_err(|e| e.to_string())
    }

    /// Play a MediaItem by path or URL.
    fn play_by_path(
        &self,
        path: &str,
        temp_url: bool,
        play_next: bool,
        play_last: bool,
    ) -> Result<(), String> {
        if temp_url {
            return self.mpv.set_now_playing(path, true, false);
        }
        let media_item = self
            .db
            .get_media_item_by_path(path)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "record not found".to_string())?;
        *self.play_queue.lock().unwrap_or_else(|e| e.into_inner()) = vec![media_item.clone()];
        self.mpv
            .set_now_playing(&media_item.path, play_next, play_last)?;
        self.db
            .update_last_played(media_item.id)
            .map_err(|e| e.to_string())
    }

    /// Cast a URL to the media player. `temp` controls whether or not it is
    /// added to the library.
    pub fn cast(
        &self,
        url: &str,
        temp_url: bool,
        play_next: bool,
        play_last: bool,
    ) -> Result<(), String> {
        if temp_url {
            self.play_by_path(url, true, play_next, play_last)
        } else {
            self.db.add_media_item_url(url).map_err(|e| e.to_string())?;
            self.play_by_path(url, false, play_next, play_last)
        }
    }

    /// Get the currently available subtitle tracks, or None for JSON `null`.
    pub fn get_subs(&self) -> Result<Option<Vec<Track>>, String> {
        let tracks = self.mpv.get_tracks()?;
        let mut res = Vec::new();
        for track in tracks {
            if track.track_type == "sub" {
                res.push(track);
            }
        }
        if res.is_empty() {
            Ok(None)
        } else {
            Ok(Some(res))
        }
    }

    pub fn set_sub_visibility(&self, visible: bool) -> Result<(), String> {
        self.mpv.set_sub_visibility(visible)
    }

    pub fn set_sub_track(&self, track_id: i64) -> Result<(), String> {
        self.mpv.set_sub_track(track_id)
    }

    /// Save subtitle delay permanently to the subtitle file.
    pub fn save_sub_delay(&self) -> Result<(), String> {
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

    pub fn take_screenshot(&self) -> Result<Screenshot, String> {
        screenshots::take_screenshot(&self.mpv, &self.screenshots_dir, Utc::now())
    }

    pub fn list_recent_screenshots(&self, limit: i64) -> Result<Vec<Screenshot>, String> {
        screenshots::list_recent_screenshots(&self.screenshots_dir, limit)
    }
}

fn walk_dir(dir: &Path, file_ext_h: &HashSet<String>, batch: &mut Vec<String>) {
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
    // filepath.WalkDir visits directory entries in lexical order.
    let mut entries: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|a| a.file_name());
    for entry in entries {
        let path = entry.path();
        // WalkDir does not follow symlinks to directories: they are treated
        // as files, exactly like Go's filepath.WalkDir.
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            walk_dir(&path, file_ext_h, batch);
            continue;
        }
        let path_str = path.to_string_lossy().to_string();
        let ext = match path_str.rfind('.') {
            Some(index) => &path_str[index + 1..],
            None => path_str.as_str(),
        };
        let ext = ext.to_lowercase();
        if !file_ext_h.contains(&ext) {
            batch.push(path_str);
        }
    }
}
