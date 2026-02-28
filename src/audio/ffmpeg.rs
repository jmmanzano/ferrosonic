//! FFmpeg audio backend — spawns ffmpeg to decode, uses rodio to play audio

use std::io::{BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use tracing::{debug, info, warn};

use crate::error::AudioError;

/// A rodio Source that reads raw s16le PCM from ffmpeg stdout
struct FfmpegSource {
    reader: BufReader<std::process::ChildStdout>,
    sample_rate: u32,
    channels: u16,
    buf: Vec<i16>,
    pos: usize,
}

impl FfmpegSource {
    fn new(stdout: std::process::ChildStdout, sample_rate: u32, channels: u16) -> Self {
        Self {
            reader: BufReader::with_capacity(65536, stdout),
            sample_rate,
            channels,
            buf: Vec::with_capacity(4096),
            pos: 0,
        }
    }
}

impl Iterator for FfmpegSource {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        if self.pos >= self.buf.len() {
            let mut raw = [0u8; 8192];
            let n = self.reader.read(&mut raw).ok()?;
            if n == 0 {
                return None;
            }
            self.buf.clear();
            // Convert pairs of bytes to i16 samples (little-endian)
            let pairs = n / 2;
            for i in 0..pairs {
                let lo = raw[i * 2];
                let hi = raw[i * 2 + 1];
                self.buf.push(i16::from_le_bytes([lo, hi]));
            }
            self.pos = 0;
        }
        let sample = self.buf[self.pos];
        self.pos += 1;
        Some(sample)
    }
}

impl Source for FfmpegSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

pub struct FfmpegController {
    process: Option<Child>,
    sink: Option<Sink>,
    _stream: Option<OutputStream>,
    _stream_handle: Option<OutputStreamHandle>,
    paused: Arc<AtomicBool>,
    started: bool,
    start_time: Option<Instant>,
    accumulated_time: f64,
}

impl FfmpegController {
    pub fn new() -> Self {
        Self {
            process: None,
            sink: None,
            _stream: None,
            _stream_handle: None,
            paused: Arc::new(AtomicBool::new(false)),
            started: false,
            start_time: None,
            accumulated_time: 0.0,
        }
    }

    /// Check that ffmpeg is available on the system
    pub fn check_available() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn start(&mut self) -> Result<(), AudioError> {
        // Initialize rodio output stream
        let (stream, handle) = OutputStream::try_default()
            .map_err(|e| AudioError::MpvIpc(format!("Failed to open audio output: {}", e)))?;
        self._stream = Some(stream);
        self._stream_handle = Some(handle);
        self.started = true;
        info!("FFmpeg audio backend started (rodio output ready)");
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.started
    }

    pub fn loadfile(&mut self, url: &str) -> Result<(), AudioError> {
        // Stop any current playback
        self.stop_current();

        info!("FFmpeg loading: {}", url.split('?').next().unwrap_or(url));

        // Spawn ffmpeg to decode the URL to raw PCM
        let mut child = Command::new("ffmpeg")
            .args([
                "-i", url,
                "-f", "s16le",
                "-acodec", "pcm_s16le",
                "-ac", "2",
                "-ar", "44100",
                "-loglevel", "error",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| AudioError::MpvIpc(format!("Failed to spawn ffmpeg: {}", e)))?;

        let stdout = child.stdout.take()
            .ok_or_else(|| AudioError::MpvIpc("No stdout from ffmpeg".to_string()))?;

        let source = FfmpegSource::new(stdout, 44100, 2);

        // Create sink and play
        let handle = self._stream_handle.as_ref()
            .ok_or_else(|| AudioError::MpvIpc("Audio output not initialized".to_string()))?;
        let sink = Sink::try_new(handle)
            .map_err(|e| AudioError::MpvIpc(format!("Failed to create audio sink: {}", e)))?;
        sink.append(source);

        self.process = Some(child);
        self.sink = Some(sink);
        self.paused.store(false, Ordering::SeqCst);
        self.start_time = Some(Instant::now());
        self.accumulated_time = 0.0;

        debug!("FFmpeg playback started");
        Ok(())
    }

    fn stop_current(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.start_time = None;
        self.accumulated_time = 0.0;
    }

    pub fn pause(&mut self) -> Result<(), AudioError> {
        if let Some(ref sink) = self.sink {
            sink.pause();
            // Accumulate elapsed time
            if let Some(start) = self.start_time.take() {
                self.accumulated_time += start.elapsed().as_secs_f64();
            }
            self.paused.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), AudioError> {
        if let Some(ref sink) = self.sink {
            sink.play();
            self.start_time = Some(Instant::now());
            self.paused.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    pub fn toggle_pause(&mut self) -> Result<bool, AudioError> {
        let is_paused = self.paused.load(Ordering::SeqCst);
        if is_paused {
            self.resume()?;
        } else {
            self.pause()?;
        }
        Ok(!is_paused)
    }

    pub fn is_paused(&self) -> Result<bool, AudioError> {
        Ok(self.paused.load(Ordering::SeqCst))
    }

    pub fn stop(&mut self) -> Result<(), AudioError> {
        self.stop_current();
        Ok(())
    }

    pub fn get_time_pos(&self) -> Result<f64, AudioError> {
        let elapsed = match self.start_time {
            Some(start) => start.elapsed().as_secs_f64(),
            None => 0.0,
        };
        Ok(self.accumulated_time + elapsed)
    }

    pub fn is_idle(&self) -> Result<bool, AudioError> {
        if let Some(ref sink) = self.sink {
            Ok(sink.empty())
        } else {
            Ok(true)
        }
    }

    pub fn set_volume(&mut self, volume: i32) -> Result<(), AudioError> {
        if let Some(ref sink) = self.sink {
            let vol = (volume.clamp(0, 100) as f32) / 100.0;
            sink.set_volume(vol);
        }
        Ok(())
    }

    pub fn quit(&mut self) -> Result<(), AudioError> {
        self.stop_current();
        self.started = false;
        info!("FFmpeg backend shut down");
        Ok(())
    }

    // Stubs for features not fully supported by FFmpeg backend
    pub fn seek(&mut self, _position: f64) -> Result<(), AudioError> {
        warn!("Seek not supported in FFmpeg backend");
        Ok(())
    }

    pub fn seek_relative(&mut self, _offset: f64) -> Result<(), AudioError> {
        warn!("Seek not supported in FFmpeg backend");
        Ok(())
    }

    pub fn loadfile_append(&mut self, _path: &str) -> Result<(), AudioError> {
        debug!("Gapless preload not supported in FFmpeg backend");
        Ok(())
    }

    pub fn get_playlist_count(&self) -> Result<usize, AudioError> {
        Ok(if self.sink.is_some() { 1 } else { 0 })
    }

    pub fn get_playlist_pos(&self) -> Result<Option<i64>, AudioError> {
        Ok(if self.sink.is_some() { Some(0) } else { None })
    }

    pub fn playlist_remove(&mut self, _index: usize) -> Result<(), AudioError> {
        Ok(())
    }

    pub fn get_duration(&self) -> Result<f64, AudioError> {
        Ok(0.0)
    }

    pub fn get_sample_rate(&self) -> Result<Option<u32>, AudioError> {
        Ok(Some(44100))
    }

    pub fn get_bit_depth(&self) -> Result<Option<u32>, AudioError> {
        Ok(Some(16))
    }

    pub fn get_audio_format(&self) -> Result<Option<String>, AudioError> {
        Ok(Some("s16le".to_string()))
    }

    pub fn get_channels(&self) -> Result<Option<String>, AudioError> {
        Ok(Some("Stereo".to_string()))
    }
}

impl Drop for FfmpegController {
    fn drop(&mut self) {
        let _ = self.quit();
    }
}

impl Default for FfmpegController {
    fn default() -> Self {
        Self::new()
    }
}
