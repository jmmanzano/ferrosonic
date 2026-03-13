//! Main application module

pub mod actions;
mod cava;
mod equalizer;
mod input;
mod input_artists;
mod input_equalizer;
mod input_playlists;
mod input_queue;
mod input_radio;
mod input_server;
mod input_settings;
mod mouse;
mod mouse_artists;
mod mouse_playlists;
mod playback;
pub mod state;

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
#[cfg(not(windows))]
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::audio::ffmpeg::FfmpegController;
use crate::audio::mpv::MpvController;
use crate::audio::pipewire::PipeWireController;
use crate::config::Config;
use crate::error::{Error, UiError};
#[cfg(unix)]
use crate::mpris::server::{start_mpris_server, update_mpris_properties, MprisPlayer};
use crate::subsonic::SubsonicClient;
use crate::ui;

pub use actions::*;
pub use state::*;

/// Channel buffer size
const CHANNEL_SIZE: usize = 256;

/// Main application
pub struct App {
    /// Shared application state
    state: SharedState,
    /// Subsonic client
    subsonic: Option<SubsonicClient>,
    /// MPV audio controller
    mpv: MpvController,
    /// FFmpeg audio controller
    ffmpeg: FfmpegController,
    /// PipeWire sample rate controller
    pipewire: PipeWireController,
    /// Channel to send audio actions
    #[allow(dead_code)]
    audio_tx: mpsc::Sender<AudioAction>,
    /// Cava child process
    cava_process: Option<std::process::Child>,
    /// Cava pty master fd for reading output
    cava_pty_master: Option<std::fs::File>,
    /// Cava terminal parser
    cava_parser: Option<vt100::Parser>,
    /// Last mouse click position and time (for second-click detection)
    last_click: Option<(u16, u16, std::time::Instant)>,
    /// Whether to use FFmpeg backend instead of MPV
    use_ffmpeg_backend: bool,
    /// Channel to receive audio actions (from MPRIS)
    audio_rx: mpsc::Receiver<AudioAction>,
    /// Last song ID used for automatic similar-song extension
    last_auto_similar_seed_id: Option<String>,
    /// Debounced equalizer apply deadline
    pending_equalizer_apply_at: Option<std::time::Instant>,
    /// Last persisted queue JSON payload (used to avoid unnecessary writes)
    last_saved_queue_json: Option<String>,
    /// Last persisted UI state JSON payload (used to avoid unnecessary writes)
    last_saved_ui_state_json: Option<String>,
    /// MPRIS D-Bus server (Unix / D-Bus only)
    #[cfg(unix)]
    mpris_server: Option<mpris_server::Server<MprisPlayer>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueueSnapshot {
    queue: Vec<crate::subsonic::models::Child>,
    queue_position: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UiStateSnapshot {
    page_index: usize,
    artists_selected_index: Option<usize>,
    artists_selected_song: Option<usize>,
    artists_focus: usize,
    artists_tree_scroll_offset: usize,
    artists_song_scroll_offset: usize,
    artists_expanded: Vec<String>,
    queue_selected: Option<usize>,
    queue_scroll_offset: usize,
    #[serde(default)]
    queue_min_playback_rating: u8,
    playlists_selected_playlist: Option<usize>,
    playlists_selected_song: Option<usize>,
    playlists_focus: usize,
    playlists_playlist_scroll_offset: usize,
    playlists_song_scroll_offset: usize,
    radio_selected: Option<usize>,
    radio_scroll_offset: usize,
}

impl App {
    /// Create a new application instance
    pub fn new(config: Config) -> Self {
        let (audio_tx, audio_rx) = mpsc::channel(CHANNEL_SIZE);

        let state = new_shared_state(config.clone());

        let subsonic = if config.is_configured() {
            match SubsonicClient::new(&config.base_url, &config.username, &config.password) {
                Ok(client) => Some(client),
                Err(e) => {
                    warn!("Failed to create Subsonic client: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Self {
            state,
            subsonic,
            mpv: MpvController::new(),
            ffmpeg: FfmpegController::new(),
            pipewire: PipeWireController::new(),
            audio_tx,
            cava_process: None,
            cava_pty_master: None,
            cava_parser: None,
            last_click: None,
            use_ffmpeg_backend: false,
            audio_rx,
            last_auto_similar_seed_id: None,
            pending_equalizer_apply_at: None,
            last_saved_queue_json: None,
            last_saved_ui_state_json: None,
            #[cfg(unix)]
            mpris_server: None,
        }
    }

    /// Run the application
    pub async fn run(&mut self) -> Result<(), Error> {
        // Start audio backend based on config
        let use_ffmpeg = {
            let state = self.state.read().await;
            state.settings_state.audio_backend == AudioBackend::Ffmpeg
        };
        self.use_ffmpeg_backend = use_ffmpeg;

        if use_ffmpeg {
            if FfmpegController::check_available() {
                if let Err(e) = self.ffmpeg.start() {
                    warn!("Failed to start FFmpeg backend: {} - trying MPV", e);
                    let mut state = self.state.write().await;
                    state.notify_error(format!("FFmpeg error: {}. Falling back to MPV.", e));
                    drop(state);
                    // Fallback to MPV
                    if let Err(e) = self.mpv.start() {
                        warn!("Failed to start MPV: {}", e);
                    } else {
                        self.use_ffmpeg_backend = false;
                    }
                } else {
                    info!("FFmpeg audio backend started successfully");
                }
            } else {
                warn!("FFmpeg not found, falling back to MPV");
                let mut state = self.state.write().await;
                state.notify_error("FFmpeg not found. Using MPV instead.");
                state.settings_state.audio_backend = AudioBackend::Mpv;
                drop(state);
                if let Err(e) = self.mpv.start() {
                    warn!("Failed to start MPV: {}", e);
                } else {
                    self.use_ffmpeg_backend = false;
                }
            }
        } else {
            if let Err(e) = self.mpv.start() {
                warn!("Failed to start MPV: {} - audio playback won't work", e);
                let mut state = self.state.write().await;
                state.notify_error(format!("Failed to start MPV: {}. Is mpv installed?", e));
                drop(state);
            } else {
                info!("MPV started successfully, ready for playback");
            }
        }

        // Start MPRIS server for media key support (Unix / D-Bus only)
        #[cfg(unix)]
        match start_mpris_server(self.state.clone(), self.audio_tx.clone()).await {
            Ok(server) => {
                info!("MPRIS server started");
                self.mpris_server = Some(server);
            }
            Err(e) => {
                warn!("Failed to start MPRIS server: {} — media keys won't work", e);
            }
        }

        // Seed and load themes
        {
            use crate::ui::theme::{load_themes, seed_default_themes};
            if let Some(themes_dir) = crate::config::paths::themes_dir() {
                seed_default_themes(&themes_dir);
            }
            let themes = load_themes();
            let mut state = self.state.write().await;
            let theme_name = state.config.theme.clone();
            state.settings_state.themes = themes;
            state.settings_state.set_theme_by_name(&theme_name);
        }

        // Seed and load equalizer presets from JSON files
        {
            use crate::config::equalizer::{load_presets, seed_default_presets};
            seed_default_presets();
            let presets = load_presets();
            let mut state = self.state.write().await;
            let selected = state.config.equalizer_preset.clone();
            state.settings_state.equalizer_presets = presets;
            state.settings_state.set_equalizer_preset_by_name(&selected);
        }

        // Check if cava is available (Unix only — cava is not available on Windows)
        #[cfg(unix)]
        let cava_available = std::process::Command::new("which")
            .arg("cava")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        #[cfg(not(unix))]
        let cava_available = false;

        {
            let mut state = self.state.write().await;
            state.cava_available = cava_available;
            if !cava_available {
                state.settings_state.cava_enabled = false;
            }
        }

        // Start cava if enabled and available
        {
            let state = self.state.read().await;
            if state.settings_state.cava_enabled && cava_available {
                let td = state.settings_state.current_theme();
                let g = td.cava_gradient.clone();
                let h = td.cava_horizontal_gradient.clone();
                let cs = state.settings_state.cava_size as u32;
                drop(state);
                self.start_cava(&g, &h, cs);
            }
        }

        // Apply equalizer selection to the selected audio backend.
        self.apply_equalizer_from_state().await;

        // Setup terminal
        enable_raw_mode().map_err(UiError::TerminalInit)?;
        let mut stdout = io::stdout();
        #[cfg(not(windows))]
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        )
            .map_err(UiError::TerminalInit)?;
        #[cfg(windows)]
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
            .map_err(UiError::TerminalInit)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).map_err(UiError::TerminalInit)?;

        info!("Terminal initialized");

        // Load initial data if configured
        if self.subsonic.is_some() {
            self.load_initial_data().await;
        }

        // Restore persisted queue from previous session
        self.load_persisted_queue().await;
        self.load_persisted_ui_state().await;

        // Main event loop
        let result = self.event_loop(&mut terminal).await;

        // Persist queue snapshot one last time before shutdown
        self.maybe_persist_queue().await;
        self.maybe_persist_ui_state().await;

        // Cleanup cava
        self.stop_cava();

        // Cleanup audio backends
        let _ = self.mpv.quit();
        let _ = self.ffmpeg.quit();

        // Cleanup terminal
        disable_raw_mode().map_err(UiError::TerminalInit)?;
        #[cfg(not(windows))]
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            PopKeyboardEnhancementFlags
        )
            .map_err(UiError::TerminalInit)?;
        #[cfg(windows)]
        execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)
            .map_err(UiError::TerminalInit)?;
        terminal.show_cursor().map_err(UiError::Render)?;

        info!("Terminal restored");
        result
    }

    /// Load initial data from server
    async fn load_initial_data(&mut self) {
        if let Some(ref client) = self.subsonic {
            // Load artists
            match client.get_artists().await {
                Ok(artists) => {
                    let mut state = self.state.write().await;
                    let count = artists.len();
                    state.artists.artists = artists;
                    // Select first artist by default
                    if count > 0 {
                        state.artists.selected_index = Some(0);
                    }
                    info!("Loaded {} artists", count);
                }
                Err(e) => {
                    error!("Failed to load artists: {}", e);
                    let mut state = self.state.write().await;
                    state.notify_error(format!("Failed to load artists: {}", e));
                }
            }

            // Load playlists
            match client.get_playlists().await {
                Ok(playlists) => {
                    let mut state = self.state.write().await;
                    let count = playlists.len();
                    state.playlists.playlists = playlists;
                    info!("Loaded {} playlists", count);
                }
                Err(e) => {
                    error!("Failed to load playlists: {}", e);
                    // Don't show error for playlists if artists loaded
                }
            }

            // Load internet radio stations
            match client.get_internet_radio_stations().await {
                Ok(stations) => {
                    let mut state = self.state.write().await;
                    let count = stations.len();
                    if count > 0 {
                        state.radio.selected = Some(0);
                    }
                    state.radio.stations = stations;
                    info!("Loaded {} radio stations", count);
                }
                Err(e) => {
                    error!("Failed to load radio stations: {}", e);
                }
            }
        }
    }

    async fn load_persisted_queue(&mut self) {
        let Some(path) = crate::config::paths::queue_file() else {
            return;
        };

        if !path.exists() {
            return;
        }

        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read queue snapshot '{}': {}", path.display(), e);
                return;
            }
        };

        let snapshot: QueueSnapshot = match serde_json::from_str(&contents) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to parse queue snapshot '{}': {}", path.display(), e);
                return;
            }
        };

        let mut state = self.state.write().await;
        state.queue = snapshot.queue;
        state.queue_position = snapshot
            .queue_position
            .filter(|&pos| pos < state.queue.len());
        state.queue_state.selected = if state.queue.is_empty() {
            None
        } else {
            state.queue_position.or(Some(0))
        };

        info!("Restored persisted queue with {} songs", state.queue.len());

        if let Ok(json) = serde_json::to_string(&QueueSnapshot {
            queue: state.queue.clone(),
            queue_position: state.queue_position,
        }) {
            self.last_saved_queue_json = Some(json);
        }
    }

    async fn maybe_persist_queue(&mut self) {
        let snapshot = {
            let state = self.state.read().await;
            QueueSnapshot {
                queue: state.queue.clone(),
                queue_position: state.queue_position,
            }
        };

        let json = match serde_json::to_string(&snapshot) {
            Ok(j) => j,
            Err(e) => {
                warn!("Failed to serialize queue snapshot: {}", e);
                return;
            }
        };

        if self.last_saved_queue_json.as_deref() == Some(json.as_str()) {
            return;
        }

        let Some(path) = crate::config::paths::queue_file() else {
            return;
        };

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!("Failed to create queue snapshot directory '{}': {}", parent.display(), e);
                return;
            }
        }

        if let Err(e) = std::fs::write(&path, &json) {
            warn!("Failed to persist queue snapshot '{}': {}", path.display(), e);
            return;
        }

        self.last_saved_queue_json = Some(json);
    }

    fn build_ui_snapshot(state: &AppState) -> UiStateSnapshot {
        let mut artists_expanded: Vec<String> = state.artists.expanded.iter().cloned().collect();
        artists_expanded.sort();

        UiStateSnapshot {
            page_index: state.page.index(),
            artists_selected_index: state.artists.selected_index,
            artists_selected_song: state.artists.selected_song,
            artists_focus: state.artists.focus,
            artists_tree_scroll_offset: state.artists.tree_scroll_offset,
            artists_song_scroll_offset: state.artists.song_scroll_offset,
            artists_expanded,
            queue_selected: state.queue_state.selected,
            queue_scroll_offset: state.queue_state.scroll_offset,
            queue_min_playback_rating: state.queue_state.min_playback_rating.min(5),
            playlists_selected_playlist: state.playlists.selected_playlist,
            playlists_selected_song: state.playlists.selected_song,
            playlists_focus: state.playlists.focus,
            playlists_playlist_scroll_offset: state.playlists.playlist_scroll_offset,
            playlists_song_scroll_offset: state.playlists.song_scroll_offset,
            radio_selected: state.radio.selected,
            radio_scroll_offset: state.radio.scroll_offset,
        }
    }

    async fn load_persisted_ui_state(&mut self) {
        let Some(path) = crate::config::paths::ui_state_file() else {
            return;
        };

        if !path.exists() {
            return;
        }

        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read UI snapshot '{}': {}", path.display(), e);
                return;
            }
        };

        let snapshot: UiStateSnapshot = match serde_json::from_str(&contents) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to parse UI snapshot '{}': {}", path.display(), e);
                return;
            }
        };

        let mut state = self.state.write().await;

        state.page = Page::from_index(snapshot.page_index);

        state.artists.expanded = snapshot.artists_expanded.into_iter().collect();
        state.artists.focus = snapshot.artists_focus.min(1);
        state.artists.tree_scroll_offset = snapshot.artists_tree_scroll_offset;
        state.artists.song_scroll_offset = snapshot.artists_song_scroll_offset;
        let artist_tree_len = crate::ui::pages::artists::build_tree_items(&state).len();
        state.artists.selected_index = snapshot
            .artists_selected_index
            .filter(|&idx| idx < artist_tree_len);
        state.artists.selected_song = snapshot
            .artists_selected_song
            .filter(|&idx| idx < state.artists.songs.len());

        state.queue_state.selected = snapshot.queue_selected.filter(|&idx| idx < state.queue.len());
        state.queue_state.scroll_offset = snapshot.queue_scroll_offset;
        state.queue_state.min_playback_rating = snapshot.queue_min_playback_rating.min(5);

        state.playlists.selected_playlist = snapshot
            .playlists_selected_playlist
            .filter(|&idx| idx < state.playlists.playlists.len());
        state.playlists.selected_song = snapshot
            .playlists_selected_song
            .filter(|&idx| idx < state.playlists.songs.len());
        state.playlists.focus = snapshot.playlists_focus.min(1);
        state.playlists.playlist_scroll_offset = snapshot.playlists_playlist_scroll_offset;
        state.playlists.song_scroll_offset = snapshot.playlists_song_scroll_offset;

        state.radio.selected = snapshot.radio_selected.filter(|&idx| idx < state.radio.stations.len());
        state.radio.scroll_offset = snapshot.radio_scroll_offset;

        info!("Restored UI state (page: {})", state.page.label());

        if let Ok(json) = serde_json::to_string(&Self::build_ui_snapshot(&state)) {
            self.last_saved_ui_state_json = Some(json);
        }
    }

    async fn maybe_persist_ui_state(&mut self) {
        let json = {
            let state = self.state.read().await;
            match serde_json::to_string(&Self::build_ui_snapshot(&state)) {
                Ok(j) => j,
                Err(e) => {
                    warn!("Failed to serialize UI snapshot: {}", e);
                    return;
                }
            }
        };

        if self.last_saved_ui_state_json.as_deref() == Some(json.as_str()) {
            return;
        }

        let Some(path) = crate::config::paths::ui_state_file() else {
            return;
        };

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!("Failed to create UI snapshot directory '{}': {}", parent.display(), e);
                return;
            }
        }

        if let Err(e) = std::fs::write(&path, &json) {
            warn!("Failed to persist UI snapshot '{}': {}", path.display(), e);
            return;
        }

        self.last_saved_ui_state_json = Some(json);
    }

    async fn set_song_rating_and_sync(
        &mut self,
        song_id: String,
        rating: u8,
        title: String,
    ) -> Result<(), Error> {
        if rating > 5 {
            let mut state = self.state.write().await;
            state.notify_error("Rating must be between 0 and 5");
            return Ok(());
        }

        if let Some(ref client) = self.subsonic {
            match client.set_rating(&song_id, rating).await {
                Ok(()) => {
                    let mut state = self.state.write().await;
                    Self::sync_song_rating_in_state(&mut state, &song_id, rating);
                    state.notify(format!("Rating: {}/5 - {}", rating, title));
                }
                Err(e) => {
                    let mut state = self.state.write().await;
                    state.notify_error(format!("Failed to set rating: {}", e));
                }
            }
        } else {
            let mut state = self.state.write().await;
            state.notify_error("Server not configured");
        }

        Ok(())
    }

    fn sync_song_rating_in_state(state: &mut AppState, song_id: &str, rating: u8) {
        let new_rating = if rating == 0 { None } else { Some(rating) };

        for song in &mut state.artists.songs {
            if song.id == song_id {
                song.user_rating = new_rating;
            }
        }

        for song in &mut state.queue {
            if song.id == song_id {
                song.user_rating = new_rating;
            }
        }

        if let Some(song) = &mut state.now_playing.song {
            if song.id == song_id {
                song.user_rating = new_rating;
            }
        }
    }

    /// Main event loop
    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<(), Error> {
        let mut last_playback_update = std::time::Instant::now();
        let mut last_queue_persist_check = std::time::Instant::now();

        loop {
            // Determine tick rate based on whether cava is active
            let cava_active = self.cava_parser.is_some();
            let tick_rate = if cava_active {
                Duration::from_millis(16) // ~60fps
            } else {
                Duration::from_millis(100)
            };

            // Draw UI
            {
                let mut state = self.state.write().await;
                terminal
                    .draw(|frame| ui::draw(frame, &mut state))
                    .map_err(UiError::Render)?;
            }

            // Check for quit
            {
                let state = self.state.read().await;
                if state.should_quit {
                    break;
                }
            }

            // Handle events with timeout
            if event::poll(tick_rate).map_err(UiError::Input)? {
                let event = event::read().map_err(UiError::Input)?;
                self.handle_event(event).await?;
            }

            // Process any pending audio actions (from MPRIS)
            while let Ok(action) = self.audio_rx.try_recv() {
                match action {
                    AudioAction::TogglePause => { let _ = self.toggle_pause().await; }
                    AudioAction::Pause => { let _ = self.pause_playback().await; }
                    AudioAction::Resume => { let _ = self.resume_playback().await; }
                    AudioAction::Next => { let _ = self.next_track().await; }
                    AudioAction::Previous => { let _ = self.prev_track().await; }
                    AudioAction::Stop => { let _ = self.stop_playback().await; }
                    AudioAction::Seek(pos) => {
                        if let Err(e) = self.audio_seek(pos) {
                            warn!("MPRIS seek failed: {}", e);
                        } else {
                            let mut state = self.state.write().await;
                            state.now_playing.position = pos;
                        }
                    }
                    AudioAction::SeekRelative(offset) => {
                        let _ = self.audio_seek_relative(offset);
                    }
                    AudioAction::SetVolume(vol) => {
                        let _ = self.audio_set_volume(vol);
                    }
                }
            }

            // Read cava output (non-blocking)
            self.read_cava_output().await;

            // Apply pending equalizer change after debounce window.
            self.maybe_apply_scheduled_equalizer().await;

            // Update playback position every ~500ms
            let now = std::time::Instant::now();
            if now.duration_since(last_playback_update) >= Duration::from_millis(500) {
                last_playback_update = now;
                self.update_playback_info().await;
            }

            // Check for notification auto-clear (after 2 seconds)
            {
                let mut state = self.state.write().await;
                state.check_notification_timeout();
            }

            // Persist queue snapshot every second if it changed
            if last_queue_persist_check.elapsed() >= Duration::from_secs(1) {
                last_queue_persist_check = std::time::Instant::now();
                self.maybe_persist_queue().await;
                self.maybe_persist_ui_state().await;
            }
        }

        Ok(())
    }
}
