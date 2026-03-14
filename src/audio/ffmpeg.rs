//! FFmpeg audio backend using CPAL for direct hardware access
//! CPAL provides better control over hardware buffering to prevent underruns

use std::io::{BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig};
use tracing::{debug, info, warn};

use crate::error::AudioError;

/// Shared state between ffmpeg decoder and audio stream
struct AudioState {
    /// Ring buffer for decoded samples
    buffer: Vec<f32>,
    /// Write position in the ring buffer
    write_pos: usize,
    /// Read position in the ring buffer
    read_pos: usize,
    /// Whether stream is actively playing
    playing: bool,
    /// Whether data source ended
    finished: bool,
    /// Stop signal for decoder thread
    should_stop: bool,
    /// Generation id to invalidate old decoder threads on track changes
    generation: u64,
}

impl AudioState {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity],
            write_pos: 0,
            read_pos: 0,
            playing: false,
            finished: false,
            should_stop: false,
            generation: 0,
        }
    }

    fn begin_new_stream(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        // Clear buffer completely
        self.buffer.fill(0.0);
        self.write_pos = 0;
        self.read_pos = 0;
        self.playing = true;
        self.finished = false;
        self.should_stop = false;
        self.generation
    }

    fn available_write(&self) -> usize {
        let capacity = self.buffer.len();
        if self.write_pos >= self.read_pos {
            capacity - (self.write_pos - self.read_pos) - 1
        } else {
            self.read_pos - self.write_pos - 1
        }
    }

    fn push_samples(&mut self, samples: &[f32]) -> usize {
        let mut written = 0;
        for &sample in samples {
            if self.available_write() == 0 {
                break;
            }
            self.buffer[self.write_pos] = sample;
            self.write_pos = (self.write_pos + 1) % self.buffer.len();
            written += 1;
        }
        written
    }

    fn available_read(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            self.buffer.len() - self.read_pos + self.write_pos
        }
    }

    fn pop_samples(&mut self, out: &mut [f32]) -> usize {
        let mut read = 0;
        for sample in out.iter_mut() {
            if self.available_read() == 0 {
                break;
            }
            *sample = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % self.buffer.len();
            read += 1;
        }
        read
    }
}

pub struct FfmpegController {
    stream: Option<Stream>,
    audio_state: Arc<Mutex<AudioState>>,
    decode_thread: Option<std::thread::JoinHandle<()>>,
    paused: Arc<AtomicBool>,
    started: bool,
    start_time: Option<Instant>,
    accumulated_time: f64,
    device: Option<Device>,
    config: Option<StreamConfig>,
    equalizer_filter: Option<String>,
    current_url: Option<String>,
}

impl FfmpegController {
    pub fn new() -> Self {
        Self {
            stream: None,
            audio_state: Arc::new(Mutex::new(AudioState::new(2097152))),
            decode_thread: None,
            paused: Arc::new(AtomicBool::new(false)),
            started: false,
            start_time: None,
            accumulated_time: 0.0,
            device: None,
            config: None,
            equalizer_filter: None,
            current_url: None,
        }
    }

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
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::MpvIpc("No audio output device found".to_string()))?;

        let config = StreamConfig {
            channels: 2,
            sample_rate: 48000,
            buffer_size: cpal::BufferSize::Default,
        };

        info!(
            "FFmpeg audio backend started (CPAL device: {})",
            device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "Unknown".to_string())
        );

        self.device = Some(device);
        self.config = Some(config);
        self.started = true;
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.started
    }

    pub fn loadfile(&mut self, url: &str) -> Result<(), AudioError> {
        self.loadfile_at_position(url, 0.0)
    }

    fn loadfile_at_position(&mut self, url: &str, start_position: f64) -> Result<(), AudioError> {
        // Signal previous decode thread to stop and detach it
        {
            let mut state = self.audio_state.lock().unwrap();
            state.should_stop = true;
        }
        let _ = self.decode_thread.take();

        info!("FFmpeg loading: {}", url.split('?').next().unwrap_or(url));
        self.current_url = Some(url.to_string());

        let device = self
            .device
            .as_ref()
            .ok_or_else(|| AudioError::MpvIpc("Audio device not initialized".to_string()))?;

        let config = self
            .config
            .as_ref()
            .ok_or_else(|| AudioError::MpvIpc("Audio config not initialized".to_string()))?;

        // Reset buffer and start a fresh stream generation
        let generation = {
            let mut state = self.audio_state.lock().unwrap();
            state.begin_new_stream()
        };

        let url = url.to_string();
        let audio_state = Arc::clone(&self.audio_state);
        let equalizer_filter = self.equalizer_filter.clone();
        let start_position = start_position.max(0.0);

        // Start new decode thread
        let decode_thread = std::thread::spawn(move || {
            if let Err(e) =
                Self::decode_stream(&url, audio_state, generation, equalizer_filter, start_position)
            {
                warn!("FFmpeg decode error: {}", e);
            }
        });
        
        self.decode_thread = Some(decode_thread);

        let audio_state = Arc::clone(&self.audio_state);
        let paused = Arc::clone(&self.paused);

        let stream = device
            .build_output_stream(
                &config,
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if paused.load(Ordering::SeqCst) {
                        for sample in output.iter_mut() {
                            *sample = 0.0;
                        }
                        return;
                    }

                    let mut state = audio_state.lock().unwrap();
                    let read = state.pop_samples(output);

                    for i in read..output.len() {
                        output[i] = 0.0;
                    }

                    if read < output.len() && !state.finished {
                        warn!("Audio buffer underrun: got {} of {} samples", read, output.len());
                    }
                },
                move |err| {
                    warn!("CPAL stream error: {}", err);
                },
                None,
            )
            .map_err(|e| AudioError::MpvIpc(format!("Failed to build stream: {}", e)))?;

        stream
            .play()
            .map_err(|e| AudioError::MpvIpc(format!("Failed to play stream: {}", e)))?;

        self.stream = Some(stream);
        self.paused.store(false, Ordering::SeqCst);
        self.start_time = Some(Instant::now());
        self.accumulated_time = start_position;

        debug!("FFmpeg playback started with CPAL");
        Ok(())
    }

    fn decode_stream(
        url: &str,
        audio_state: Arc<Mutex<AudioState>>,
        generation: u64,
        equalizer_filter: Option<String>,
        start_position: f64,
    ) -> Result<(), String> {
        let mut args = vec![
            "-reconnect".to_string(), "1".to_string(),
            "-reconnect_streamed".to_string(), "1".to_string(),
            "-reconnect_delay_max".to_string(), "5".to_string(),
            "-probesize".to_string(), "64M".to_string(),
            "-analyzeduration".to_string(), "20M".to_string(),
            "-fflags".to_string(), "+discardcorrupt".to_string(),
        ];

        if start_position > 0.0 {
            args.extend(["-ss".to_string(), format!("{:.3}", start_position)]);
        }
        args.extend(["-i".to_string(), url.to_string()]);

        let mut filters = Vec::new();
        if let Some(eq) = equalizer_filter {
            filters.push(eq);
        }
        filters.push("aresample=async=1:min_hard_comp=0.100000".to_string());
        args.extend([
            "-f".to_string(), "s32le".to_string(),
            "-acodec".to_string(), "pcm_s32le".to_string(),
            "-ac".to_string(), "2".to_string(),
            "-ar".to_string(), "48000".to_string(),
            "-af".to_string(), filters.join(","),
        ]);

        args.extend([
            "-bufsize".to_string(), "2M".to_string(),
            "-loglevel".to_string(), "error".to_string(),
            "pipe:1".to_string(),
        ]);

        let mut child = Command::new("ffmpeg")
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "No stdout from ffmpeg".to_string())?;

        let mut reader = BufReader::with_capacity(262144, stdout);
        let mut raw = [0u8; 131072];
        let mut decode_buffer = Vec::with_capacity(32768);
        const SCALE: f32 = 1.0 / 2147483648.0;

        loop {
            // Check if we should stop
            {
                let state = audio_state.lock().unwrap();
                if state.should_stop || state.generation != generation {
                    debug!("Decode thread stopping on request");
                    return Ok(());
                }
            }

            let n = reader.read(&mut raw).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }

            decode_buffer.clear();
            for chunk in raw[..n].chunks_exact(4) {
                let i32_sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let f32_sample = (i32_sample as f32) * SCALE;
                decode_buffer.push(f32_sample);
            }

            let mut offset = 0usize;
            while offset < decode_buffer.len() {
                // Check for stop signal before waiting
                {
                    let state = audio_state.lock().unwrap();
                    if state.should_stop || state.generation != generation {
                        return Ok(());
                    }
                }

                let mut state = audio_state.lock().unwrap();
                let written = state.push_samples(&decode_buffer[offset..]);
                drop(state);

                if written == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                } else {
                    offset += written;
                }
            }
        }

        {
            let mut state = audio_state.lock().unwrap();
            if state.generation == generation {
                state.finished = true;
            }
        }

        debug!("FFmpeg decode finished");
        Ok(())
    }

    fn stop_current(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.pause();
        }
        
        // Signal decode thread to stop
        {
            let mut state = self.audio_state.lock().unwrap();
            state.should_stop = true;
            state.generation = state.generation.wrapping_add(1);
            state.write_pos = 0;
            state.read_pos = 0;
            state.finished = true;
        }
        let _ = self.decode_thread.take();
        
        self.start_time = None;
        self.accumulated_time = 0.0;
    }

    pub fn pause(&mut self) -> Result<(), AudioError> {
        if let Some(start) = self.start_time.take() {
            self.accumulated_time += start.elapsed().as_secs_f64();
        }
        self.paused.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), AudioError> {
        self.start_time = Some(Instant::now());
        self.paused.store(false, Ordering::SeqCst);
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
        let state = self.audio_state.lock().unwrap();
        Ok(state.available_read() == 0 && state.finished)
    }

    pub fn set_volume(&mut self, _volume: i32) -> Result<(), AudioError> {
        Ok(())
    }

    pub fn quit(&mut self) -> Result<(), AudioError> {
        self.stop_current();
        self.current_url = None;
        self.started = false;
        info!("FFmpeg backend shut down");
        Ok(())
    }

    /// Set the equalizer filter chain. Pass None to disable.
    pub fn set_equalizer_filter(&mut self, filter: Option<String>) -> Result<(), AudioError> {
        self.set_equalizer_filter_with_mode(filter, true)
    }

    /// Set the equalizer filter chain and force a stream restart from the start.
    /// Useful for live radio streams where seek-based resume is not meaningful.
    pub fn set_equalizer_filter_restart_stream(
        &mut self,
        filter: Option<String>,
    ) -> Result<(), AudioError> {
        self.set_equalizer_filter_with_mode(filter, false)
    }

    fn set_equalizer_filter_with_mode(
        &mut self,
        filter: Option<String>,
        preserve_position: bool,
    ) -> Result<(), AudioError> {
        self.equalizer_filter = filter;

        // Rebuild FFmpeg filter graph at current position so EQ changes are audible now
        // without jumping back to the beginning of the track.
        if let Some(url) = self.current_url.clone() {
            let position = if preserve_position {
                self.get_time_pos().unwrap_or(0.0)
            } else {
                0.0
            };
            self.loadfile_at_position(&url, position)?;
        }

        Ok(())
    }

    pub fn seek(&mut self, _position: f64) -> Result<(), AudioError> {
        let position = _position.max(0.0);
        let was_paused = self.paused.load(Ordering::SeqCst);

        if let Some(url) = self.current_url.clone() {
            self.loadfile_at_position(&url, position)?;
            if was_paused {
                self.pause()?;
            }
            Ok(())
        } else {
            warn!("Seek requested in FFmpeg backend without an active stream");
            Ok(())
        }
    }

    pub fn seek_relative(&mut self, _offset: f64) -> Result<(), AudioError> {
        let position = (self.get_time_pos()? + _offset).max(0.0);
        self.seek(position)
    }

    pub fn loadfile_append(&mut self, _path: &str) -> Result<(), AudioError> {
        debug!("Gapless preload not supported in FFmpeg backend");
        Ok(())
    }

    pub fn get_playlist_count(&self) -> Result<usize, AudioError> {
        Ok(if self.stream.is_some() { 1 } else { 0 })
    }

    pub fn get_playlist_pos(&self) -> Result<Option<i64>, AudioError> {
        Ok(if self.stream.is_some() { Some(0) } else { None })
    }

    pub fn playlist_remove(&mut self, _index: usize) -> Result<(), AudioError> {
        Ok(())
    }

    pub fn get_duration(&self) -> Result<f64, AudioError> {
        Ok(0.0)
    }

    pub fn get_sample_rate(&self) -> Result<Option<u32>, AudioError> {
        Ok(Some(48000))
    }

    pub fn get_bit_depth(&self) -> Result<Option<u32>, AudioError> {
        Ok(Some(32))
    }

    pub fn get_audio_format(&self) -> Result<Option<String>, AudioError> {
        Ok(Some("s32le".to_string()))
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
