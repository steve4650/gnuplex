use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::watch;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::{PlaylistEntry, Track};
use crate::sync::lock;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MpvResponse {
    data: Value,
    error: String,
    request_id: Option<i64>,
}

pub struct Mpv {
    verbose: bool,
    config_dir: PathBuf,
    conn: Mutex<Option<BufReader<UnixStream>>>,
    process: Mutex<Option<Child>>,
    restart_count: Mutex<u32>,
    next_request_id: AtomicI64,
}

impl Mpv {
    pub fn init(verbose: bool, config_dir: &Path) -> Result<Arc<Self>> {
        let mpv = Arc::new(Self {
            verbose,
            config_dir: config_dir.to_path_buf(),
            conn: Mutex::new(None),
            process: Mutex::new(None),
            restart_count: Mutex::new(0),
            next_request_id: AtomicI64::new(1),
        });
        mpv.restart_process()?;
        Ok(mpv)
    }

    fn restart_process(&self) -> Result<()> {
        {
            let mut count = lock(&self.restart_count);
            if *count >= 10 {
                return Err(Error::Mpv(
                    "4b367530-abab-4964-a093-72f78cf523f2: reached limit of MPV restarts (10)"
                        .to_string(),
                ));
            }
            *count += 1;
        }
        {
            let mut process = lock(&self.process);
            if let Some(child) = process.as_mut()
                && child.try_wait().ok().flatten().is_none()
            {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        // Drop the old connection; mpv's IPC socket disappears with it.
        *lock(&self.conn) = None;

        let socket_path = PathBuf::from(format!("/tmp/mpvsocket-{}", Uuid::new_v4()));
        let mut command = Command::new("mpv");
        if self.verbose {
            command.arg("-v");
        }
        command
            .arg(format!("--config-dir={}", self.config_dir.display()))
            .arg(format!("--input-ipc-server={}", socket_path.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        info!("starting mpv process");
        let child = command.spawn().map_err(|e| {
            Error::Mpv(format!(
                "1b2b2f70-d75c-4ee4-b977-95f7272f72e9: mpvdaemon.Run: {e}"
            ))
        })?;
        *lock(&self.process) = Some(child);

        let mut attempt = 0;
        loop {
            attempt += 1;
            match UnixStream::connect(&socket_path) {
                Ok(stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
                    *lock(&self.conn) = Some(BufReader::new(stream));
                    info!("Established unix socket connection to mpv.");
                    return Ok(());
                }
                Err(err) => {
                    if attempt >= 101 {
                        return Err(Error::Mpv(format!(
                            "84c2ca00-1df2-4af5-8bd4-9248d941ad7d: could not establish a unix socket connection to {}: {err}",
                            socket_path.display()
                        )));
                    }
                    warn!(
                        "6b94b8d9-9992-4276-8f6f-4d1b339cb59a: could not init unix socket to mpv, trying again in 2 seconds. {err}"
                    );
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
    }

    /// Restart mpv when it becomes unresponsive, exiting the daemon after the
    /// same 10-restart limit as before.
    pub fn spawn_supervisor(self: &Arc<Self>, shutdown: watch::Sender<bool>) {
        let mpv = Arc::clone(self);
        std::thread::spawn(move || {
            loop {
                if mpv.ping().is_err() {
                    error!(
                        "b304c2bb-a940-4a3e-979d-2939e986ed87: failed to ping MPV. restarting MPV."
                    );
                    if let Err(err) = mpv.restart_process() {
                        error!(
                            "c7b92229-b6b8-4800-91d6-8b8afb733d9c: failed to restart MPV. stopping gnuplex: {err}"
                        );
                        let _ = shutdown.send(true);
                        return;
                    }
                }
                std::thread::sleep(Duration::from_secs(3));
            }
        });
    }

    fn request(&self, command: Value) -> Result<MpvResponse> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let encoded = serde_json::to_string(&json!({
            "command": command,
            "request_id": request_id,
        }))?;

        let mut guard = lock(&self.conn);
        let conn = guard.as_mut().ok_or_else(|| {
            Error::Mpv("5f0b1f1a-4f6c-4a53-a8a3-2e8f0c9a1f43: mpv is not connected".to_string())
        })?;
        conn.get_mut().write_all(encoded.as_bytes())?;
        conn.get_mut().write_all(b"\n")?;
        conn.get_mut().flush()?;

        let mut line = String::new();
        loop {
            line.clear();
            if conn.read_line(&mut line)? == 0 {
                return Err(Error::Mpv("mpv closed the IPC connection".to_string()));
            }
            let response: MpvResponse = match serde_json::from_str(line.trim()) {
                Ok(response) => response,
                Err(err) => {
                    return Err(Error::Mpv(format!(
                        "could not decode mpv response {:?}: {err}",
                        line.trim()
                    )));
                }
            };
            if response.request_id != Some(request_id) {
                // Events and responses to other requests are interleaved on
                // the same stream.
                continue;
            }
            return match response.error.as_str() {
                "success" => Ok(response),
                "property unavailable" => Err(Error::PropertyUnavailable),
                _ => Err(Error::Mpv(response.error)),
            };
        }
    }

    fn get<T: DeserializeOwned + Default>(&self, property: &str) -> Result<T> {
        let response = self.request(json!(["get_property", property]))?;
        if response.data.is_null() {
            Ok(T::default())
        } else {
            serde_json::from_value(response.data).map_err(Into::into)
        }
    }

    fn set(&self, command: Value) -> Result<()> {
        self.request(command).map(|_| ())
    }

    pub fn play(&self) -> Result<()> {
        self.set(json!(["set_property", "pause", false]))
    }

    pub fn pause(&self) -> Result<()> {
        self.set(json!(["set_property", "pause", true]))
    }

    pub fn play_pause(&self) -> Result<()> {
        self.set(json!(["cycle", "pause"]))
    }

    pub fn skip(&self) -> Result<()> {
        self.save_last_watched()?;
        let playlist = self.get_now_playing()?;
        if playlist.len() > 1 {
            self.set(json!(["playlist-next"]))?;
        }
        self.delete_queue_entry(0)
    }

    pub fn delete_queue_entry(&self, id: i64) -> Result<()> {
        self.save_last_watched()?;
        self.set(json!(["playlist-remove", id]))
    }

    fn save_last_watched(&self) -> Result<()> {
        self.set(json!(["write-watch-later-config"]))
    }

    pub fn get_paused(&self) -> Result<bool> {
        self.get("pause")
    }

    pub fn get_now_playing(&self) -> Result<Vec<PlaylistEntry>> {
        self.get("playlist")
    }

    pub fn get_current_filename(&self) -> Result<String> {
        let playlist = self.get_now_playing()?;
        if let Some(entry) = playlist.iter().find(|entry| entry.current) {
            return Ok(entry.filename.clone());
        }
        playlist
            .first()
            .map(|entry| entry.filename.clone())
            .ok_or(Error::PropertyUnavailable)
    }

    pub fn set_now_playing(&self, filepath: &str, play_next: bool, play_last: bool) -> Result<()> {
        if play_next {
            self.set(json!(["loadfile", filepath, "insert-next-play"]))
        } else if play_last {
            self.set(json!(["loadfile", filepath, "append-play"]))
        } else {
            Err(Error::BadRequest(
                "GetNowPlaying called incorrectly; need to specify a mode".to_string(),
            ))
        }
    }

    pub fn get_vol(&self) -> Result<i64> {
        let volume: f64 = self.get("volume")?;
        Ok(volume as i64)
    }

    pub fn set_vol(&self, vol: i64) -> Result<()> {
        self.set(json!(["set_property", "volume", vol]))
    }

    pub fn get_pos(&self) -> Result<i64> {
        let position: f64 = self.get("time-pos")?;
        Ok(position as i64)
    }

    pub fn get_time_remaining(&self) -> Result<i64> {
        let remaining: f64 = self.get("time-remaining")?;
        Ok(remaining as i64)
    }

    pub fn set_pos(&self, pos: i64) -> Result<()> {
        self.set(json!(["set_property", "time-pos", pos]))
    }

    pub fn screenshot_to_file(&self, filename: &str) -> Result<()> {
        self.set(json!(["screenshot-to-file", filename, "subtitles"]))
    }

    pub fn get_tracks(&self) -> Result<Vec<Track>> {
        self.get("track-list")
    }

    pub fn set_sub_visibility(&self, visible: bool) -> Result<()> {
        self.set(json!(["set_property", "sub-visibility", visible]))
    }

    pub fn set_sub_track(&self, track_id: i64) -> Result<()> {
        self.set(json!(["set_property", "sid", track_id]))
    }

    pub fn get_sub_delay(&self) -> Result<f64> {
        self.get("sub-delay")
    }

    pub fn set_sub_delay(&self, delay: f64) -> Result<()> {
        self.set(json!(["set_property", "sub-delay", delay]))
    }

    pub fn sub_seek(&self, skip: i64) -> Result<()> {
        self.set(json!(["sub-seek", skip]))
    }

    pub fn get_current_sub_filename(&self) -> Result<String> {
        for track in self.get_tracks()? {
            if track.track_type == "sub" && track.selected && track.external {
                return Ok(track.external_filename);
            }
        }
        Ok(String::new())
    }

    pub fn sub_reload(&self) -> Result<()> {
        self.set(json!(["sub-reload"]))
    }

    pub fn ping(&self) -> Result<i64> {
        self.get("pid")
    }

    pub fn set_filter(&self, filter: &str) -> Result<()> {
        let filter_cmd = match filter.trim().to_lowercase().as_str() {
            "bw" => "lavfi=[format=gray]",
            "grainy" => {
                "scale=480:trunc(ow/a/2)*2,setsar=1:1,eq=saturation=0.8,noise=alls=20:allf=t+u"
            }
            "mirror" => "lavfi=[split[back][front];[back]hflip[back];[front][back]hstack]",
            "8bit" => "lavfi=[scale=iw/20:-1,scale=iw*20:-1:flags=neighbor]",
            "sepia" => {
                "lavfi=[colorchannelmixer=.393:.769:.189:0:.349:.686:.168:0:.272:.534:.131,eq=contrast=1.1:brightness=-0.02]"
            }
            "psychedelic" => "lavfi=[lagfun=decay=0.95]",
            "tron" => "lavfi=[edgedetect=low=0.1:high=0.4]",
            _ => "",
        };
        self.set(json!(["set_property", "vf", filter_cmd]))
    }
}
