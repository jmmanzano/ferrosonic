//! MPV controller via JSON IPC

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, trace};

use crate::config::paths::mpv_socket_path;
use crate::error::AudioError;

// ── Cross-platform IPC socket ─────────────────────────────────────────────────

/// Wraps a platform-specific IPC stream:
///  - Unix  : UnixStream (domain socket)
///  - Windows: std::fs::File (named pipe)
struct IpcStream {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    inner: std::fs::File,
    /// Fallback for exotic targets (WASM, etc.)
    #[cfg(not(any(unix, windows)))]
    inner: std::io::Cursor<Vec<u8>>,
}

impl IpcStream {
    fn connect(path: &std::path::Path) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixStream;
            return Ok(Self {
                inner: UnixStream::connect(path)?,
            });
        }
        #[cfg(windows)]
        {
            // MPV on Windows exposes a named pipe, e.g. \\.\pipe\ferrosonic-mpv
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)?;
            return Ok(Self { inner: file });
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "IPC not supported on this platform",
            ))
        }
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            self.inner.set_read_timeout(timeout)
        }
        #[cfg(not(unix))]
        {
            // Named-pipe reads on Windows do not expose set_read_timeout on File.
            // MPV always responds quickly, so blocking reads are acceptable here.
            let _ = timeout;
            Ok(())
        }
    }

    fn try_clone(&self) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            return Ok(Self {
                inner: self.inner.try_clone()?,
            });
        }
        #[cfg(windows)]
        {
            return Ok(Self {
                inner: self.inner.try_clone()?,
            });
        }
        #[cfg(not(any(unix, windows)))]
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "IPC not supported on this platform",
        ))
    }
}

impl std::io::Read for IpcStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl std::io::Write for IpcStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

// ─────────────────────────────────────────────────────────────────────────────

/// MPV IPC command
#[derive(Debug, Serialize)]
struct MpvCommand {
    command: Vec<Value>,
    request_id: u64,
}

/// MPV IPC response
#[derive(Debug, Deserialize)]
struct MpvResponse {
    #[serde(default)]
    request_id: Option<u64>,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: String,
}

/// MPV event (used for deserialization and debug tracing)
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Fields populated by deserialization, read via Debug
struct MpvEvent {
    event: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    data: Option<Value>,
}

/// MPV controller
pub struct MpvController {
    /// Path to the IPC socket
    socket_path: PathBuf,
    /// MPV process handle
    process: Option<Child>,
    /// Request ID counter
    request_id: AtomicU64,
    /// Socket connection
    socket: Option<IpcStream>,
    /// Active lavfi equalizer filter, if any
    equalizer_filter: Option<String>,
}

impl MpvController {
    /// Create a new MPV controller
    pub fn new() -> Self {
        Self {
            socket_path: mpv_socket_path(),
            process: None,
            request_id: AtomicU64::new(1),
            socket: None,
            equalizer_filter: None,
        }
    }

    /// Start MPV process if not running
    pub fn start(&mut self) -> Result<(), AudioError> {
        if self.process.is_some() {
            return Ok(());
        }

        // Remove existing socket if present
        let _ = std::fs::remove_file(&self.socket_path);

        info!("Starting MPV with socket: {}", self.socket_path.display());

        let child = Command::new("mpv")
            .arg("--idle") // Stay running when nothing playing
            .arg("--no-video") // Audio only
            .arg("--no-terminal") // No MPV UI
            .arg("--gapless-audio=yes") // Gapless playback between tracks
            .arg("--prefetch-playlist=yes") // Pre-buffer next track
            .arg("--cache=yes") // Enable cache for network streams
            .arg("--cache-secs=120") // Cache up to 2 minutes ahead
            .arg("--demuxer-max-bytes=100MiB") // Allow large demuxer buffer
            .arg(format!("--input-ipc-server={}", self.socket_path.display()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(AudioError::MpvSpawn)?;

        self.process = Some(child);

        // Wait for socket to become available
        for _ in 0..50 {
            if self.socket_path.exists() {
                std::thread::sleep(Duration::from_millis(50));
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        if !self.socket_path.exists() {
            return Err(AudioError::MpvIpc("Socket not created".to_string()));
        }

        self.connect()?;
        info!("MPV started successfully");
        Ok(())
    }

    /// Connect to the MPV socket
    fn connect(&mut self) -> Result<(), AudioError> {
        let stream = IpcStream::connect(&self.socket_path).map_err(AudioError::MpvSocket)?;

        // Set read timeout (no-op on Windows, 100ms on Unix)
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(AudioError::MpvSocket)?;

        self.socket = Some(stream);
        debug!("Connected to MPV socket");
        Ok(())
    }

    /// Check if MPV is running
    pub fn is_running(&self) -> bool {
        self.socket.is_some()
    }

    /// Send a command to MPV
    fn send_command(&mut self, args: Vec<Value>) -> Result<Option<Value>, AudioError> {
        let socket = self.socket.as_mut().ok_or(AudioError::MpvNotRunning)?;

        let request_id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let cmd = MpvCommand {
            command: args,
            request_id,
        };

        let json = serde_json::to_string(&cmd)?;
        debug!("Sending MPV command: {}", json);

        writeln!(socket, "{}", json).map_err(|e| AudioError::MpvIpc(e.to_string()))?;
        socket
            .flush()
            .map_err(|e| AudioError::MpvIpc(e.to_string()))?;

        // Read response
        let mut reader = BufReader::new(socket.try_clone().map_err(AudioError::MpvSocket)?);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => return Err(AudioError::MpvIpc("Socket closed".to_string())),
                Ok(_) => {
                    if let Ok(resp) = serde_json::from_str::<MpvResponse>(&line) {
                        if resp.request_id == Some(request_id) {
                            if resp.error != "success" {
                                return Err(AudioError::MpvIpc(resp.error));
                            }
                            return Ok(resp.data);
                        }
                    }
                    // Log discarded events for diagnostics
                    if let Ok(event) = serde_json::from_str::<MpvEvent>(&line) {
                        trace!("MPV event: {:?}", event);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Timeout, try again
                    continue;
                }
                Err(e) => return Err(AudioError::MpvIpc(e.to_string())),
            }
        }
    }

    /// Load and play a file/URL (replaces current playlist)
    pub fn loadfile(&mut self, path: &str) -> Result<(), AudioError> {
        info!("Loading: {}", path.split('?').next().unwrap_or(path));
        self.send_command(vec![json!("loadfile"), json!(path), json!("replace")])?;
        self.apply_equalizer_filter()?;
        Ok(())
    }

    /// Append a file/URL to the playlist (for gapless playback)
    pub fn loadfile_append(&mut self, path: &str) -> Result<(), AudioError> {
        debug!(
            "Appending to playlist: {}",
            path.split('?').next().unwrap_or(path)
        );
        self.send_command(vec![json!("loadfile"), json!(path), json!("append")])?;
        Ok(())
    }

    /// Remove a specific entry from the playlist by index
    pub fn playlist_remove(&mut self, index: usize) -> Result<(), AudioError> {
        debug!("Removing playlist entry {}", index);
        self.send_command(vec![json!("playlist-remove"), json!(index)])?;
        Ok(())
    }

    /// Get current playlist position (0-indexed)
    pub fn get_playlist_pos(&mut self) -> Result<Option<i64>, AudioError> {
        let data = self.send_command(vec![json!("get_property"), json!("playlist-pos")])?;
        Ok(data.and_then(|v| v.as_i64()))
    }

    /// Get playlist count
    pub fn get_playlist_count(&mut self) -> Result<usize, AudioError> {
        let data = self.send_command(vec![json!("get_property"), json!("playlist-count")])?;
        Ok(data.and_then(|v| v.as_u64()).unwrap_or(0) as usize)
    }

    /// Pause playback
    pub fn pause(&mut self) -> Result<(), AudioError> {
        debug!("Pausing playback");
        self.send_command(vec![json!("set_property"), json!("pause"), json!(true)])?;
        Ok(())
    }

    /// Resume playback
    pub fn resume(&mut self) -> Result<(), AudioError> {
        debug!("Resuming playback");
        self.send_command(vec![json!("set_property"), json!("pause"), json!(false)])?;
        Ok(())
    }

    /// Toggle pause
    pub fn toggle_pause(&mut self) -> Result<bool, AudioError> {
        let paused = self.is_paused()?;
        if paused {
            self.resume()?;
        } else {
            self.pause()?;
        }
        Ok(!paused)
    }

    /// Check if paused
    pub fn is_paused(&mut self) -> Result<bool, AudioError> {
        let data = self.send_command(vec![json!("get_property"), json!("pause")])?;
        Ok(data.and_then(|v| v.as_bool()).unwrap_or(false))
    }

    /// Stop playback
    pub fn stop(&mut self) -> Result<(), AudioError> {
        debug!("Stopping playback");
        self.send_command(vec![json!("stop")])?;
        Ok(())
    }

    /// Seek to position (seconds)
    pub fn seek(&mut self, position: f64) -> Result<(), AudioError> {
        debug!("Seeking to {:.1}s", position);
        self.send_command(vec![json!("seek"), json!(position), json!("absolute")])?;
        Ok(())
    }

    /// Seek relative to current position
    pub fn seek_relative(&mut self, offset: f64) -> Result<(), AudioError> {
        debug!("Seeking {:+.1}s", offset);
        self.send_command(vec![json!("seek"), json!(offset), json!("relative")])?;
        Ok(())
    }

    /// Get current playback position in seconds
    pub fn get_time_pos(&mut self) -> Result<f64, AudioError> {
        let data = self.send_command(vec![json!("get_property"), json!("time-pos")])?;
        Ok(data.and_then(|v| v.as_f64()).unwrap_or(0.0))
    }

    /// Get total duration in seconds
    pub fn get_duration(&mut self) -> Result<f64, AudioError> {
        let data = self.send_command(vec![json!("get_property"), json!("duration")])?;
        Ok(data.and_then(|v| v.as_f64()).unwrap_or(0.0))
    }

    /// Set volume (0-100)
    pub fn set_volume(&mut self, volume: i32) -> Result<(), AudioError> {
        debug!("Setting volume to {}", volume);
        self.send_command(vec![
            json!("set_property"),
            json!("volume"),
            json!(volume.clamp(0, 100)),
        ])?;
        Ok(())
    }

    /// Get audio sample rate
    pub fn get_sample_rate(&mut self) -> Result<Option<u32>, AudioError> {
        let data = self.send_command(vec![
            json!("get_property"),
            json!("audio-params/samplerate"),
        ])?;
        Ok(data.and_then(|v| v.as_u64()).map(|v| v as u32))
    }

    /// Get audio bit depth
    pub fn get_bit_depth(&mut self) -> Result<Option<u32>, AudioError> {
        // MPV returns format string like "s16" or "s32"
        let data = self.send_command(vec![json!("get_property"), json!("audio-params/format")])?;
        let format = data.and_then(|v| v.as_str().map(String::from));

        Ok(format.and_then(|f| {
            if f.contains("32") || f.contains("float") {
                Some(32)
            } else if f.contains("24") {
                Some(24)
            } else if f.contains("16") {
                Some(16)
            } else if f.contains("8") {
                Some(8)
            } else {
                None
            }
        }))
    }

    /// Get audio format string
    pub fn get_audio_format(&mut self) -> Result<Option<String>, AudioError> {
        let data = self.send_command(vec![json!("get_property"), json!("audio-params/format")])?;
        Ok(data.and_then(|v| v.as_str().map(String::from)))
    }

    /// Get audio channel layout
    pub fn get_channels(&mut self) -> Result<Option<String>, AudioError> {
        let data = self.send_command(vec![
            json!("get_property"),
            json!("audio-params/channel-count"),
        ])?;
        let count = data.and_then(|v| v.as_u64()).map(|v| v as u32);

        Ok(count.map(|c| match c {
            1 => "Mono".to_string(),
            2 => "Stereo".to_string(),
            n => format!("{}ch", n),
        }))
    }

    /// Check if anything is loaded
    pub fn is_idle(&mut self) -> Result<bool, AudioError> {
        let data = self.send_command(vec![json!("get_property"), json!("idle-active")])?;
        Ok(data.and_then(|v| v.as_bool()).unwrap_or(true))
    }

    /// Quit MPV
    pub fn quit(&mut self) -> Result<(), AudioError> {
        if self.socket.is_some() {
            let _ = self.send_command(vec![json!("quit")]);
        }

        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        self.socket = None;
        let _ = std::fs::remove_file(&self.socket_path);

        info!("MPV shut down");
        Ok(())
    }

    /// Set the equalizer lavfi filter. Pass None to disable.
    pub fn set_equalizer_filter(&mut self, filter: Option<String>) -> Result<(), AudioError> {
        self.equalizer_filter = filter;
        if self.socket.is_some() {
            self.apply_equalizer_filter()?;
        }
        Ok(())
    }

    fn apply_equalizer_filter(&mut self) -> Result<(), AudioError> {
        // Always clear the previous filter stack first.
        self.send_command(vec![json!("af"), json!("clr")])?;

        if let Some(filter) = self.equalizer_filter.clone() {
            self.send_command(vec![json!("af"), json!("add"), json!(filter)])?;
        }

        Ok(())
    }

}

impl Drop for MpvController {
    fn drop(&mut self) {
        let _ = self.quit();
    }
}

impl Default for MpvController {
    fn default() -> Self {
        Self::new()
    }
}
