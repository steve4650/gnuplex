use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::watch;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::models::{PlaylistEntry, Track};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MpvResponse {
    data: Value,
    error: String,
}

pub struct Mpv {
    verbose: bool,
    config_dir: String,
    conn: Mutex<Option<BufReader<UnixStream>>>,
    process: Mutex<Option<Child>>,
    restart_count: Mutex<u32>,
}

impl Mpv {
    pub fn init(verbose: bool, config_dir: &str) -> Result<Arc<Self>, String> {
        let mpv = Arc::new(Self {
            verbose,
            config_dir: config_dir.to_string(),
            conn: Mutex::new(None),
            process: Mutex::new(None),
            restart_count: Mutex::new(0),
        });
        mpv.restart_process()?;
        Ok(mpv)
    }

    fn restart_process(&self) -> Result<(), String> {
        {
            let mut count = self.restart_count.lock().unwrap_or_else(|e| e.into_inner());
            if *count >= 10 {
                return Err(
                    "4b367530-abab-4964-a093-72f78cf523f2 reached limit of MPV restarts (10)"
                        .to_string(),
                );
            }
            *count += 1;
        }
        {
            let mut process = self.process.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(child) = process.as_mut()
                && child.try_wait().ok().flatten().is_none()
            {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        // Drop the old connection; mpv's IPC socket disappears with it.
        *self.conn.lock().unwrap_or_else(|e| e.into_inner()) = None;

        let socket_path = format!("/tmp/mpvsocket-{}", Uuid::new_v4());
        let mut command = Command::new("mpv");
        if self.verbose {
            command.arg("-v");
        }
        command
            .arg(format!("--config-dir={}", self.config_dir))
            .arg(format!("--input-ipc-server={socket_path}"))
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        info!("starting mpv process");
        let child = command
            .spawn()
            .map_err(|e| format!("1b2b2f70-d75c-4ee4-b977-95f7272f72e9: mpvdaemon.Run: {e}"))?;
        *self.process.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);

        let mut attempt = 0;
        loop {
            attempt += 1;
            match UnixStream::connect(&socket_path) {
                Ok(stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
                    *self.conn.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(BufReader::new(stream));
                    info!("Established unix socket connection to mpv.");
                    return Ok(());
                }
                Err(err) => {
                    if attempt >= 101 {
                        return Err(format!(
                            "84c2ca00-1df2-4af5-8bd4-9248d941ad7d: could not establish a unix socket connection to {socket_path}: {err}"
                        ));
                    }
                    warn!(
                        "Warning 6b94b8d9-9992-4276-8f6f-4d1b339cb59a: could not init unix socket to mpv, trying again in 2 seconds. {err}"
                    );
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
    }

    /// Restart mpv when it becomes unresponsive, exiting the daemon after
    /// the same 10-restart limit as the Go implementation.
    pub fn spawn_supervisor(self: &Arc<Self>, shutdown: watch::Sender<bool>) {
        let mpv = Arc::clone(self);
        std::thread::spawn(move || {
            loop {
                if mpv.ping().is_err() {
                    error!(
                        "Error b304c2bb-a940-4a3e-979d-2939e986ed87: failed to ping MPV. restarting MPV."
                    );
                    if let Err(err) = mpv.restart_process() {
                        error!(
                            "Error c7b92229-b6b8-4800-91d6-8b8afb733d9c: failed to restart MPV. stopping gnuplex: {err}"
                        );
                        let _ = shutdown.send(true);
                        return;
                    }
                }
                std::thread::sleep(Duration::from_secs(3));
            }
        });
    }

    fn request(&self, value: Value) -> Result<MpvResponse, String> {
        let encoded = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        let mut guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let conn = guard.as_mut().ok_or_else(|| {
            "5f0b1f1a-4f6c-4a53-a8a3-2e8f0c9a1f43: mpv is not connected".to_string()
        })?;
        conn.get_mut()
            .write_all(encoded.as_bytes())
            .and_then(|_| conn.get_mut().write_all(b"\n"))
            .and_then(|_| conn.get_mut().flush())
            .map_err(|e| e.to_string())?;
        let mut line = String::new();
        loop {
            line.clear();
            let n = conn.read_line(&mut line).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("EOF".to_string());
            }
            if line.contains("request_id") {
                let response: MpvResponse =
                    serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
                if response.error != "success" {
                    return Err(response.error);
                }
                return Ok(response);
            }
        }
    }

    fn get<T: DeserializeOwned + Default>(&self, property: &str) -> Result<T, String> {
        let response = self.request(json!({ "command": ["get_property", property] }))?;
        if response.data.is_null() {
            Ok(T::default())
        } else {
            serde_json::from_value(response.data).map_err(|e| e.to_string())
        }
    }

    fn set(&self, command: Value) -> Result<(), String> {
        self.request(json!({ "command": command })).map(|_| ())
    }

    pub fn play(&self) -> Result<(), String> {
        self.set(json!(["set_property", "pause", false]))
    }

    pub fn pause(&self) -> Result<(), String> {
        self.set(json!(["set_property", "pause", true]))
    }

    pub fn play_pause(&self) -> Result<(), String> {
        self.set(json!(["cycle", "pause"]))
    }

    pub fn skip(&self) -> Result<(), String> {
        self.save_last_watched()?;
        let playlist = self.get_now_playing()?;
        if playlist.len() > 1 {
            self.set(json!(["playlist-next"]))?;
        }
        let _ = self.get_now_playing()?;
        self.delete_queue_entry(0)
    }

    pub fn delete_queue_entry(&self, id: i64) -> Result<(), String> {
        self.save_last_watched()?;
        self.set(json!(["playlist-remove", id]))
    }

    fn save_last_watched(&self) -> Result<(), String> {
        self.set(json!(["write-watch-later-config"]))
    }

    pub fn get_paused(&self) -> Result<bool, String> {
        self.get("pause")
    }

    pub fn get_now_playing(&self) -> Result<Vec<PlaylistEntry>, String> {
        self.get("playlist")
    }

    pub fn get_current_filename(&self) -> Result<String, String> {
        let playlist = self.get_now_playing()?;
        for entry in &playlist {
            if entry.current {
                return Ok(entry.filename.clone());
            }
        }
        if playlist.is_empty() {
            return Err("property unavailable".to_string());
        }
        Ok(playlist[0].filename.clone())
    }

    pub fn set_now_playing(
        &self,
        filepath: &str,
        play_next: bool,
        play_last: bool,
    ) -> Result<(), String> {
        if play_next {
            self.set(json!(["loadfile", filepath, "insert-next-play"]))
        } else if play_last {
            self.set(json!(["loadfile", filepath, "append-play"]))
        } else {
            Err("GetNowPlaying called incorrectly; need to specify a mode".to_string())
        }
    }

    pub fn get_vol(&self) -> Result<i64, String> {
        let n: f64 = self.get("volume")?;
        Ok(n as i64)
    }

    pub fn set_vol(&self, vol: i64) -> Result<(), String> {
        self.set(json!(["set_property", "volume", vol]))
    }

    pub fn get_pos(&self) -> Result<i64, String> {
        let n: f64 = self.get("time-pos")?;
        Ok(n as i64)
    }

    pub fn get_time_remaining(&self) -> Result<i64, String> {
        let n: f64 = self.get("time-remaining")?;
        Ok(n as i64)
    }

    pub fn set_pos(&self, pos: i64) -> Result<(), String> {
        self.set(json!(["set_property", "time-pos", pos]))
    }

    pub fn screenshot_to_file(&self, filename: &str) -> Result<(), String> {
        self.set(json!(["screenshot-to-file", filename, "subtitles"]))
    }

    pub fn get_tracks(&self) -> Result<Vec<Track>, String> {
        self.get("track-list")
    }

    pub fn set_sub_visibility(&self, visible: bool) -> Result<(), String> {
        self.set(json!(["set_property", "sub-visibility", visible]))
    }

    pub fn set_sub_track(&self, track_id: i64) -> Result<(), String> {
        self.set(json!(["set_property", "sid", track_id]))
    }

    pub fn get_sub_delay(&self) -> Result<f64, String> {
        self.get("sub-delay")
    }

    pub fn set_sub_delay(&self, delay: f64) -> Result<(), String> {
        self.set(json!(["set_property", "sub-delay", delay]))
    }

    pub fn sub_seek(&self, skip: i64) -> Result<(), String> {
        self.set(json!(["sub-seek", skip]))
    }

    pub fn get_current_sub_filename(&self) -> Result<String, String> {
        for track in self.get_tracks()? {
            if track.track_type == "sub" && track.selected && track.external {
                return Ok(track.external_filename);
            }
        }
        Ok(String::new())
    }

    pub fn sub_reload(&self) -> Result<(), String> {
        self.set(json!(["sub-reload"]))
    }

    pub fn ping(&self) -> Result<i64, String> {
        self.get("pid")
    }

    pub fn set_filter(&self, filter: &str) -> Result<(), String> {
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
