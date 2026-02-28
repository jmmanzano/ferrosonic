use tracing::{debug, error, info, warn};

use crate::error::AudioError;
use crate::subsonic::models::InternetRadioStation;

use super::*;

// ── Audio dispatch helpers -- delegate to the active backend ──────────────

impl App {
    fn audio_loadfile(&mut self, url: &str) -> Result<(), AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.loadfile(url) } else { self.mpv.loadfile(url) }
    }
    fn audio_loadfile_append(&mut self, url: &str) -> Result<(), AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.loadfile_append(url) } else { self.mpv.loadfile_append(url) }
    }
    fn audio_pause(&mut self) -> Result<(), AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.pause() } else { self.mpv.pause() }
    }
    fn audio_resume(&mut self) -> Result<(), AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.resume() } else { self.mpv.resume() }
    }
    fn audio_toggle_pause(&mut self) -> Result<bool, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.toggle_pause() } else { self.mpv.toggle_pause() }
    }
    fn audio_is_paused(&mut self) -> Result<bool, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.is_paused() } else { self.mpv.is_paused() }
    }
    fn audio_stop(&mut self) -> Result<(), AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.stop() } else { self.mpv.stop() }
    }
    pub(super) fn audio_seek(&mut self, pos: f64) -> Result<(), AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.seek(pos) } else { self.mpv.seek(pos) }
    }
    pub(super) fn audio_seek_relative(&mut self, offset: f64) -> Result<(), AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.seek_relative(offset) } else { self.mpv.seek_relative(offset) }
    }
    pub(super) fn audio_set_volume(&mut self, vol: i32) -> Result<(), AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.set_volume(vol) } else { self.mpv.set_volume(vol) }
    }
    fn audio_get_time_pos(&mut self) -> Result<f64, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.get_time_pos() } else { self.mpv.get_time_pos() }
    }
    fn audio_get_duration(&mut self) -> Result<f64, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.get_duration() } else { self.mpv.get_duration() }
    }
    fn audio_get_sample_rate(&mut self) -> Result<Option<u32>, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.get_sample_rate() } else { self.mpv.get_sample_rate() }
    }
    fn audio_get_bit_depth(&mut self) -> Result<Option<u32>, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.get_bit_depth() } else { self.mpv.get_bit_depth() }
    }
    fn audio_get_audio_format(&mut self) -> Result<Option<String>, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.get_audio_format() } else { self.mpv.get_audio_format() }
    }
    fn audio_get_channels(&mut self) -> Result<Option<String>, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.get_channels() } else { self.mpv.get_channels() }
    }
    fn audio_is_idle(&mut self) -> Result<bool, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.is_idle() } else { self.mpv.is_idle() }
    }
    fn audio_is_running(&self) -> bool {
        if self.use_ffmpeg_backend { self.ffmpeg.is_running() } else { self.mpv.is_running() }
    }
    fn audio_get_playlist_count(&mut self) -> Result<usize, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.get_playlist_count() } else { self.mpv.get_playlist_count() }
    }
    fn audio_get_playlist_pos(&mut self) -> Result<Option<i64>, AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.get_playlist_pos() } else { self.mpv.get_playlist_pos() }
    }
    fn audio_playlist_remove(&mut self, idx: usize) -> Result<(), AudioError> {
        if self.use_ffmpeg_backend { self.ffmpeg.playlist_remove(idx) } else { self.mpv.playlist_remove(idx) }
    }
}

impl App {
    /// Update playback position and audio info from MPV
    pub(super) async fn update_playback_info(&mut self) {
        // Only update if something should be playing
        let state = self.state.read().await;
        let is_playing = state.now_playing.state == PlaybackState::Playing;
        let is_active = is_playing || state.now_playing.state == PlaybackState::Paused;
        drop(state);

        if !is_active || !self.audio_is_running() {
            return;
        }

        // Check for track advancement
        if is_playing {
            // Early transition: if near end of track and no preloaded next track,
            // advance immediately instead of waiting for idle detection
            {
                let state = self.state.read().await;
                let time_remaining = state.now_playing.duration - state.now_playing.position;
                let has_next = state
                    .queue_position
                    .map(|p| p + 1 < state.queue.len())
                    .unwrap_or(false);
                drop(state);

                if has_next && time_remaining > 0.0 && time_remaining < 2.0 {
                    if let Ok(count) = self.audio_get_playlist_count() {
                        if count < 2 {
                            info!("Near end of track with no preloaded next — advancing early");
                            let _ = self.next_track().await;
                            return;
                        }
                    }
                }
            }

            // Re-preload if the appended track was lost
            if let Ok(count) = self.audio_get_playlist_count() {
                if count == 1 {
                    let state = self.state.read().await;
                    if let Some(pos) = state.queue_position {
                        if pos + 1 < state.queue.len() {
                            drop(state);
                            debug!("Playlist count is 1, re-preloading next track");
                            self.preload_next_track(pos).await;
                        }
                    }
                }
            }

            // Check if MPV advanced to next track in playlist (gapless transition)
            if let Ok(Some(mpv_pos)) = self.audio_get_playlist_pos() {
                if mpv_pos == 1 {
                    // Gapless advance happened - update our state to match
                    let state = self.state.read().await;
                    if let Some(current_pos) = state.queue_position {
                        let next_pos = current_pos + 1;
                        if next_pos < state.queue.len() {
                            drop(state);
                            info!("Gapless advancement to track {}", next_pos);

                            // Update state - keep audio properties since they'll be similar
                            // for gapless transitions (same album, same format)
                            let mut state = self.state.write().await;
                            state.queue_position = Some(next_pos);
                            if let Some(song) = state.queue.get(next_pos).cloned() {
                                state.now_playing.song = Some(song.clone());
                                state.now_playing.position = 0.0;
                                state.now_playing.duration = song.duration.unwrap_or(0) as f64;
                                // Don't reset audio properties - let them update naturally
                                // This avoids triggering PipeWire rate changes unnecessarily
                            }
                            drop(state);

                            // Remove the finished track (index 0) from MPV's playlist
                            // This is less disruptive than playlist_clear during playback
                            let _ = self.audio_playlist_remove(0);

                            // Preload the next track for continued gapless playback
                            self.preload_next_track(next_pos).await;
                            return;
                        }
                    }
                    drop(state);
                }
            }

            // Check if MPV went idle (track ended, no preloaded track)
            if let Ok(idle) = self.audio_is_idle() {
                if idle {
                    info!("Track ended, advancing to next");
                    let _ = self.next_track().await;
                    return;
                }
            }
        }

        // Get position from MPV
        if let Ok(position) = self.audio_get_time_pos() {
            let mut state = self.state.write().await;
            state.now_playing.position = position;
        }

        // Get duration if not set
        {
            let state = self.state.read().await;
            if state.now_playing.duration <= 0.0 {
                drop(state);
                if let Ok(duration) = self.audio_get_duration() {
                    if duration > 0.0 {
                        let mut state = self.state.write().await;
                        state.now_playing.duration = duration;
                    }
                }
            }
        }

        // Get audio properties - keep polling until we get valid values
        // MPV may not have them ready immediately when playback starts
        {
            let state = self.state.read().await;
            let need_sample_rate = state.now_playing.sample_rate.is_none();
            drop(state);

            if need_sample_rate {
                // Try to get audio properties from MPV
                let sample_rate = self.audio_get_sample_rate().ok().flatten();
                let bit_depth = self.audio_get_bit_depth().ok().flatten();
                let format = self.audio_get_audio_format().ok().flatten();
                let channels = self.audio_get_channels().ok().flatten();

                // Only update if we got a valid sample rate (indicates audio is ready)
                if let Some(rate) = sample_rate {
                    // Only switch PipeWire sample rate if it's actually different
                    // This avoids unnecessary rate switches during gapless playback
                    // of albums with the same sample rate
                    let current_pw_rate = self.pipewire.get_current_rate();
                    if current_pw_rate != Some(rate) {
                        info!("Sample rate change: {:?} -> {} Hz", current_pw_rate, rate);
                        if let Err(e) = self.pipewire.set_rate(rate) {
                            warn!("Failed to set PipeWire sample rate: {}", e);
                        }
                    } else {
                        debug!(
                            "Sample rate unchanged at {} Hz, skipping PipeWire switch",
                            rate
                        );
                    }

                    let mut state = self.state.write().await;
                    state.now_playing.sample_rate = Some(rate);
                    state.now_playing.bit_depth = bit_depth;
                    state.now_playing.format = format;
                    state.now_playing.channels = channels;
                }
            }
        }

        // Update MPRIS properties to keep external clients in sync (Unix only)
        #[cfg(unix)]
        if let Some(ref server) = self.mpris_server {
            if let Err(e) = update_mpris_properties(server, &self.state).await {
                debug!("Failed to update MPRIS properties: {}", e);
            }
        }
    }

    /// Toggle play/pause
    pub(super) async fn toggle_pause(&mut self) -> Result<(), Error> {
        let state = self.state.read().await;
        let is_playing = state.now_playing.state == PlaybackState::Playing;
        let is_paused = state.now_playing.state == PlaybackState::Paused;
        drop(state);

        if !is_playing && !is_paused {
            return Ok(());
        }

        match self.audio_toggle_pause() {
            Ok(now_paused) => {
                let mut state = self.state.write().await;
                if now_paused {
                    state.now_playing.state = PlaybackState::Paused;
                    debug!("Paused playback");
                } else {
                    state.now_playing.state = PlaybackState::Playing;
                    debug!("Resumed playback");
                }
            }
            Err(e) => {
                error!("Failed to toggle pause: {}", e);
            }
        }
        Ok(())
    }

    /// Pause playback (only if currently playing)
    pub(super) async fn pause_playback(&mut self) -> Result<(), Error> {
        let state = self.state.read().await;
        if state.now_playing.state != PlaybackState::Playing {
            return Ok(());
        }
        drop(state);

        match self.audio_pause() {
            Ok(()) => {
                let mut state = self.state.write().await;
                state.now_playing.state = PlaybackState::Paused;
                debug!("Paused playback");
            }
            Err(e) => {
                error!("Failed to pause: {}", e);
            }
        }
        Ok(())
    }

    /// Resume playback (only if currently paused)
    pub(super) async fn resume_playback(&mut self) -> Result<(), Error> {
        let state = self.state.read().await;
        if state.now_playing.state != PlaybackState::Paused {
            return Ok(());
        }
        drop(state);

        match self.audio_resume() {
            Ok(()) => {
                let mut state = self.state.write().await;
                state.now_playing.state = PlaybackState::Playing;
                debug!("Resumed playback");
            }
            Err(e) => {
                error!("Failed to resume: {}", e);
            }
        }
        Ok(())
    }

    /// Play next track in queue
    pub(super) async fn next_track(&mut self) -> Result<(), Error> {
        let state = self.state.read().await;
        let queue_len = state.queue.len();
        let current_pos = state.queue_position;
        drop(state);

        if queue_len == 0 {
            return Ok(());
        }

        let next_pos = match current_pos {
            Some(pos) if pos + 1 < queue_len => pos + 1,
            _ => {
                info!("Reached end of queue");
                let _ = self.audio_stop();
                let mut state = self.state.write().await;
                state.now_playing.state = PlaybackState::Stopped;
                state.now_playing.position = 0.0;
                return Ok(());
            }
        };

        self.play_queue_position(next_pos).await
    }

    /// Play previous track in queue (or restart current if < 3 seconds in)
    pub(super) async fn prev_track(&mut self) -> Result<(), Error> {
        let state = self.state.read().await;
        let queue_len = state.queue.len();
        let current_pos = state.queue_position;
        let position = state.now_playing.position;
        drop(state);

        if queue_len == 0 {
            return Ok(());
        }

        if position < 3.0 {
            if let Some(pos) = current_pos {
                if pos > 0 {
                    return self.play_queue_position(pos - 1).await;
                }
            }
            if let Err(e) = self.audio_seek(0.0) {
                error!("Failed to restart track: {}", e);
            } else {
                let mut state = self.state.write().await;
                state.now_playing.position = 0.0;
            }
            return Ok(());
        }

        debug!("Restarting current track (position: {:.1}s)", position);
        if let Err(e) = self.audio_seek(0.0) {
            error!("Failed to restart track: {}", e);
        } else {
            let mut state = self.state.write().await;
            state.now_playing.position = 0.0;
        }
        Ok(())
    }

    /// Play a specific position in the queue
    pub(super) async fn play_queue_position(&mut self, pos: usize) -> Result<(), Error> {
        let state = self.state.read().await;
        let song = match state.queue.get(pos) {
            Some(s) => s.clone(),
            None => return Ok(()),
        };
        drop(state);

        let stream_url = if let Some(ref client) = self.subsonic {
            match client.get_stream_url(&song.id) {
                Ok(url) => url,
                Err(e) => {
                    error!("Failed to get stream URL: {}", e);
                    let mut state = self.state.write().await;
                    state.notify_error(format!("Failed to get stream URL: {}", e));
                    return Ok(());
                }
            }
        } else {
            return Ok(());
        };

        {
            let mut state = self.state.write().await;
            state.queue_position = Some(pos);
            state.now_playing.song = Some(song.clone());
            state.now_playing.state = PlaybackState::Playing;
            state.now_playing.position = 0.0;
            state.now_playing.duration = song.duration.unwrap_or(0) as f64;
            state.now_playing.sample_rate = None;
            state.now_playing.bit_depth = None;
            state.now_playing.format = None;
            state.now_playing.channels = None;
        }

        info!("Playing: {} (queue pos {})", song.title, pos);
        if self.audio_is_paused().unwrap_or(false) {
            let _ = self.audio_resume();
        }
        if let Err(e) = self.audio_loadfile(&stream_url) {
            error!("Failed to play: {}", e);
            let mut state = self.state.write().await;
            state.notify_error(format!("Audio error: {}", e));
            return Ok(());
        }

        self.preload_next_track(pos).await;

        Ok(())
    }

    /// Pre-load the next track into MPV's playlist for gapless playback
    pub(super) async fn preload_next_track(&mut self, current_pos: usize) {
        let state = self.state.read().await;
        let next_pos = current_pos + 1;

        if next_pos >= state.queue.len() {
            return;
        }

        let next_song = match state.queue.get(next_pos) {
            Some(s) => s.clone(),
            None => return,
        };
        drop(state);

        if let Some(ref client) = self.subsonic {
            if let Ok(url) = client.get_stream_url(&next_song.id) {
                debug!("Pre-loading next track for gapless: {}", next_song.title);
                if let Err(e) = self.audio_loadfile_append(&url) {
                    debug!("Failed to pre-load next track: {}", e);
                } else if let Ok(count) = self.audio_get_playlist_count() {
                    if count < 2 {
                        warn!(
                            "Preload may have failed: playlist count is {} (expected 2)",
                            count
                        );
                    } else {
                        debug!("Preload confirmed: playlist count is {}", count);
                    }
                }
            }
        }
    }

    /// Stop playback and clear the queue
    pub(super) async fn stop_playback(&mut self) -> Result<(), Error> {
        let _ = self.audio_stop();

        let mut state = self.state.write().await;
        state.now_playing.state = PlaybackState::Stopped;
        state.now_playing.song = None;
        state.now_playing.position = 0.0;
        state.now_playing.duration = 0.0;
        state.now_playing.sample_rate = None;
        state.now_playing.bit_depth = None;
        state.now_playing.format = None;
        state.now_playing.channels = None;
        state.queue.clear();
        state.queue_position = None;
        state.playing_radio = false;
        Ok(())
    }

    /// Play an internet radio station (direct stream, no queue)
    pub(super) async fn play_radio_station(&mut self, station: &InternetRadioStation) -> Result<(), Error> {
        // Stop any current playback first
        let _ = self.audio_stop();

        let stream_url = station.stream_url.clone();

        // Create a pseudo-song for NowPlaying display
        let pseudo_song = crate::subsonic::models::Child {
            id: station.id.clone(),
            parent: None,
            is_dir: false,
            title: station.name.clone(),
            album: Some("Internet Radio".to_string()),
            artist: station.home_page_url.clone().or_else(|| Some("Radio".to_string())),
            track: None,
            year: None,
            genre: None,
            cover_art: None,
            size: None,
            content_type: None,
            suffix: None,
            duration: None,
            bit_rate: None,
            path: None,
            disc_number: None,
        };

        {
            let mut state = self.state.write().await;
            state.playing_radio = true;
            state.queue.clear();
            state.queue_position = None;
            state.now_playing.song = Some(pseudo_song.clone());
            state.now_playing.state = PlaybackState::Playing;
            state.now_playing.position = 0.0;
            state.now_playing.duration = 0.0;
            state.now_playing.sample_rate = None;
            state.now_playing.bit_depth = None;
            state.now_playing.format = None;
            state.now_playing.channels = None;
        }

        info!("Playing radio station: {}", station.name);
        if let Err(e) = self.audio_loadfile(&stream_url) {
            error!("Failed to play radio station: {}", e);
            let mut state = self.state.write().await;
            state.notify_error(format!("Radio error: {}", e));
            state.now_playing.state = PlaybackState::Stopped;
            state.now_playing.song = None;
            state.playing_radio = false;
            return Ok(());
        }

        let mut state = self.state.write().await;
        state.notify(format!("Playing: {}", station.name));
        Ok(())
    }
}
