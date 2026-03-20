//! Shared application state

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use ratatui::layout::Rect;

use crate::config::Config;
use crate::config::equalizer::EqualizerPreset;
use crate::subsonic::models::{Album, Artist, Child, InternetRadioStation, Playlist};
use crate::ui::theme::{ThemeColors, ThemeData};

/// Current page in the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Artists,
    Queue,
    Playlists,
    Radio,
    Server,
    Settings,
    Equalizer,
}

impl Page {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Page::Artists,
            1 => Page::Queue,
            2 => Page::Playlists,
            3 => Page::Radio,
            4 => Page::Server,
            5 => Page::Settings,
            6 => Page::Equalizer,
            _ => Page::Artists,
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Page::Artists => 0,
            Page::Queue => 1,
            Page::Playlists => 2,
            Page::Radio => 3,
            Page::Server => 4,
            Page::Settings => 5,
            Page::Equalizer => 6,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Page::Artists => "Artists",
            Page::Queue => "Queue",
            Page::Playlists => "Playlists",
            Page::Radio => "Radio",
            Page::Server => "Server",
            Page::Settings => "Settings",
            Page::Equalizer => "Equalizer",
        }
    }

    pub fn shortcut(&self) -> &'static str {
        match self {
            Page::Artists => "F1",
            Page::Queue => "F2",
            Page::Playlists => "F3",
            Page::Radio => "F4",
            Page::Server => "F5",
            Page::Settings => "F6",
            Page::Equalizer => "F7",
        }
    }
}

/// Playback state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// Now playing information
#[derive(Debug, Clone, Default)]
pub struct NowPlaying {
    /// Currently playing song
    pub song: Option<Child>,
    /// Playback state
    pub state: PlaybackState,
    /// Current position in seconds
    pub position: f64,
    /// Total duration in seconds
    pub duration: f64,
    /// Audio sample rate (Hz)
    pub sample_rate: Option<u32>,
    /// Audio bit depth
    pub bit_depth: Option<u32>,
    /// Audio format/codec
    pub format: Option<String>,
    /// Audio channel layout (e.g., "Stereo", "Mono", "5.1ch")
    pub channels: Option<String>,
}

impl NowPlaying {
    pub fn progress_percent(&self) -> f64 {
        if self.duration > 0.0 {
            (self.position / self.duration).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn format_position(&self) -> String {
        format_duration(self.position)
    }

    pub fn format_duration(&self) -> String {
        format_duration(self.duration)
    }
}

/// Format duration in MM:SS or HH:MM:SS format
pub fn format_duration(seconds: f64) -> String {
    let total_secs = seconds as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

/// Artists page state
#[derive(Debug, Clone, Default)]
pub struct ArtistsState {
    /// List of all artists
    pub artists: Vec<Artist>,
    /// Currently selected index in the tree (artists + expanded albums)
    pub selected_index: Option<usize>,
    /// Set of expanded artist IDs
    pub expanded: std::collections::HashSet<String>,
    /// Albums cached per artist ID
    pub albums_cache: std::collections::HashMap<String, Vec<Album>>,
    /// Songs in the selected album (shown in right pane)
    pub songs: Vec<Child>,
    /// Currently selected song index
    pub selected_song: Option<usize>,
    /// Artist filter text
    pub filter: String,
    /// Whether filter input is active
    pub filter_active: bool,
    /// Focus: 0 = tree, 1 = songs
    pub focus: usize,
    /// Scroll offset for the tree list (set after render)
    pub tree_scroll_offset: usize,
    /// Scroll offset for the songs list (set after render)
    pub song_scroll_offset: usize,
}

/// Queue page state
#[derive(Debug, Clone, Default)]
pub struct QueueState {
    /// Currently selected index in the queue
    pub selected: Option<usize>,
    /// Scroll offset for the queue list (set after render)
    pub scroll_offset: usize,
    /// Minimum user rating required for playback (0 = disabled).
    /// Unrated songs are always allowed.
    pub min_playback_rating: u8,
}

/// Playlists page state
#[derive(Debug, Clone, Default)]
pub struct PlaylistsState {
    /// List of all playlists
    pub playlists: Vec<Playlist>,
    /// Currently selected playlist index
    pub selected_playlist: Option<usize>,
    /// Songs in the selected playlist
    pub songs: Vec<Child>,
    /// Currently selected song index
    pub selected_song: Option<usize>,
    /// Focus: 0 = playlists, 1 = songs
    pub focus: usize,
    /// Scroll offset for the playlists list (set after render)
    pub playlist_scroll_offset: usize,
    /// Scroll offset for the songs list (set after render)
    pub song_scroll_offset: usize,
}

/// Radio page state
#[derive(Debug, Clone, Default)]
pub struct RadioState {
    pub stations: Vec<InternetRadioStation>,
    pub selected: Option<usize>,
    pub scroll_offset: usize,
}

/// Audio backend selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioBackend {
    #[default]
    Mpv,
    Ffmpeg,
}

impl AudioBackend {
    pub fn label(&self) -> &'static str {
        match self {
            AudioBackend::Mpv => "MPV",
            AudioBackend::Ffmpeg => "FFmpeg",
        }
    }
    pub fn toggle(&self) -> Self {
        match self {
            AudioBackend::Mpv => AudioBackend::Ffmpeg,
            AudioBackend::Ffmpeg => AudioBackend::Mpv,
        }
    }
}

/// Server page state (connection settings)
#[derive(Debug, Clone, Default)]
pub struct ServerState {
    /// Currently focused field (0-4: URL, Username, Password, Test, Save)
    pub selected_field: usize,
    /// Edit values
    pub base_url: String,
    pub username: String,
    pub password: String,
    /// Status message
    pub status: Option<String>,
}

/// Settings page state
#[derive(Debug, Clone)]
pub struct SettingsState {
    /// Currently focused field (0=Theme, 1=Cava, 2=CavaSize, 3=AudioBackend, 4=NonStopMode, 5=Equalizer)
    pub selected_field: usize,
    /// Available themes (Default + loaded from files)
    pub themes: Vec<ThemeData>,
    /// Index of the currently selected theme in `themes`
    pub theme_index: usize,
    /// Cava visualizer enabled
    pub cava_enabled: bool,
    /// Cava visualizer height percentage (10-80, step 5)
    pub cava_size: u8,
    /// Audio backend
    pub audio_backend: AudioBackend,
    /// Auto-extend queue with similar songs when the last song is playing
    pub non_stop_mode: bool,
    /// Equalizer enabled state
    pub equalizer_enabled: bool,
    /// Equalizer presets loaded from JSON files
    pub equalizer_presets: Vec<EqualizerPreset>,
    /// Selected equalizer preset index
    pub equalizer_preset_index: usize,
    /// Whether the user is currently editing band gains
    pub eq_editing: bool,
    /// Which band (0–9) is focused in edit mode
    pub eq_selected_band: usize,
    /// Whether the user is currently renaming the selected preset
    pub eq_renaming: bool,
    /// Text buffer used while renaming the selected preset
    pub eq_rename_buffer: String,
    /// Desktop notifications when a song changes
    pub notifications_enabled: bool,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            selected_field: 0,
            themes: vec![ThemeData::default_theme()],
            theme_index: 0,
            cava_enabled: false,
            cava_size: 40,
            audio_backend: AudioBackend::default(),
            non_stop_mode: false,
            equalizer_enabled: false,
            equalizer_presets: vec![EqualizerPreset {
                name: "Flat".to_string(),
                bands: [0.0; 10],
            }],
            equalizer_preset_index: 0,
            eq_editing: false,
            eq_selected_band: 0,
            eq_renaming: false,
            eq_rename_buffer: String::new(),
            notifications_enabled: false,
        }
    }
}

impl SettingsState {
    /// Current theme name
    pub fn theme_name(&self) -> &str {
        &self.themes[self.theme_index].name
    }

    /// Current theme colors
    pub fn theme_colors(&self) -> &ThemeColors {
        &self.themes[self.theme_index].colors
    }

    /// Current theme data
    pub fn current_theme(&self) -> &ThemeData {
        &self.themes[self.theme_index]
    }

    /// Cycle to next theme
    pub fn next_theme(&mut self) {
        self.theme_index = (self.theme_index + 1) % self.themes.len();
    }

    /// Cycle to previous theme
    pub fn prev_theme(&mut self) {
        self.theme_index = (self.theme_index + self.themes.len() - 1) % self.themes.len();
    }

    /// Set theme by name, returning true if found
    pub fn set_theme_by_name(&mut self, name: &str) -> bool {
        if let Some(idx) = self.themes.iter().position(|t| t.name.eq_ignore_ascii_case(name)) {
            self.theme_index = idx;
            true
        } else {
            self.theme_index = 0; // Fall back to Default
            false
        }
    }

    pub fn equalizer_preset_name(&self) -> &str {
        &self.equalizer_presets[self.equalizer_preset_index].name
    }

    pub fn current_equalizer_preset(&self) -> &EqualizerPreset {
        &self.equalizer_presets[self.equalizer_preset_index]
    }

    pub fn current_equalizer_preset_mut(&mut self) -> &mut EqualizerPreset {
        &mut self.equalizer_presets[self.equalizer_preset_index]
    }

    pub fn next_equalizer_preset(&mut self) {
        self.equalizer_preset_index =
            (self.equalizer_preset_index + 1) % self.equalizer_presets.len();
    }

    pub fn prev_equalizer_preset(&mut self) {
        self.equalizer_preset_index =
            (self.equalizer_preset_index + self.equalizer_presets.len() - 1)
                % self.equalizer_presets.len();
    }

    pub fn set_equalizer_preset_by_name(&mut self, name: &str) -> bool {
        if let Some(idx) = self
            .equalizer_presets
            .iter()
            .position(|p| p.name.eq_ignore_ascii_case(name))
        {
            self.equalizer_preset_index = idx;
            true
        } else {
            self.equalizer_preset_index = 0;
            false
        }
    }
}

/// Notification/alert to display
#[derive(Debug, Clone)]
pub struct Notification {
    pub message: String,
    pub is_error: bool,
    pub created_at: Instant,
}

/// Cached layout rectangles from the last render, used for mouse hit-testing.
/// Automatically updated every frame, so resize and visualiser toggle are handled.
#[derive(Debug, Clone, Default)]
pub struct LayoutAreas {
    pub header: Rect,
    pub content: Rect,
    pub now_playing: Rect,
    /// Left pane for dual-pane pages (Artists tree, Playlists list)
    pub content_left: Option<Rect>,
    /// Right pane for dual-pane pages (Songs list)
    pub content_right: Option<Rect>,
}

/// Complete application state
#[derive(Debug, Default)]
pub struct AppState {
    /// Application configuration
    pub config: Config,
    /// Current page
    pub page: Page,
    /// Now playing information
    pub now_playing: NowPlaying,
    /// Play queue (songs)
    pub queue: Vec<Child>,
    /// Current position in queue
    pub queue_position: Option<usize>,
    /// Artists page state
    pub artists: ArtistsState,
    /// Queue page state
    pub queue_state: QueueState,
    /// Playlists page state
    pub playlists: PlaylistsState,
    /// Radio page state
    pub radio: RadioState,
    /// Server page state (connection settings)
    pub server_state: ServerState,
    /// Settings page state (app preferences)
    pub settings_state: SettingsState,
    /// Current notification
    pub notification: Option<Notification>,
    /// Whether the app should quit
    pub should_quit: bool,
    /// Cava visualizer screen content (rows of styled spans)
    pub cava_screen: Vec<CavaRow>,
    /// Whether the cava binary is available on the system
    pub cava_available: bool,
    /// Cached layout areas from last render (for mouse hit-testing)
    pub layout: LayoutAreas,
    /// Whether currently playing a radio stream (no queue)
    pub playing_radio: bool,
}

/// A row of styled segments from cava's terminal output
#[derive(Debug, Clone, Default)]
pub struct CavaRow {
    pub spans: Vec<CavaSpan>,
}

/// A styled text segment from cava's terminal output
#[derive(Debug, Clone)]
pub struct CavaSpan {
    pub text: String,
    pub fg: CavaColor,
    pub bg: CavaColor,
}

/// Color from cava's terminal output
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum CavaColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let audio_backend = match config.audio_backend.as_str() {
            "ffmpeg" => AudioBackend::Ffmpeg,
            _ => AudioBackend::Mpv,
        };
        let mut state = Self {
            config: config.clone(),
            ..Default::default()
        };
        // Initialize server page with current values
        state.server_state.base_url = config.base_url.clone();
        state.server_state.username = config.username.clone();
        state.server_state.password = config.password.clone();
        // Initialize cava from config
        state.settings_state.cava_enabled = config.cava;
        state.settings_state.cava_size = config.cava_size.clamp(10, 80);
        state.settings_state.audio_backend = audio_backend;
        state.settings_state.non_stop_mode = config.non_stop_mode;
        state.settings_state.equalizer_enabled = config.equalizer_enabled;
        state.settings_state.notifications_enabled = config.notifications_enabled;
        state
    }

    /// Get the currently playing song from the queue
    pub fn current_song(&self) -> Option<&Child> {
        self.queue_position.and_then(|pos| self.queue.get(pos))
    }

    /// Show a notification
    pub fn notify(&mut self, message: impl Into<String>) {
        self.notification = Some(Notification {
            message: message.into(),
            is_error: false,
            created_at: Instant::now(),
        });
    }

    /// Show an error notification
    pub fn notify_error(&mut self, message: impl Into<String>) {
        self.notification = Some(Notification {
            message: message.into(),
            is_error: true,
            created_at: Instant::now(),
        });
    }

    /// Check if notification should be auto-cleared (after 2 seconds)
    pub fn check_notification_timeout(&mut self) {
        if let Some(ref notif) = self.notification {
            if notif.created_at.elapsed().as_secs() >= 2 {
                self.notification = None;
            }
        }
    }

    /// Clear the notification
    pub fn clear_notification(&mut self) {
        self.notification = None;
    }
}

/// Thread-safe shared state
pub type SharedState = Arc<RwLock<AppState>>;

/// Create new shared state
pub fn new_shared_state(config: Config) -> SharedState {
    Arc::new(RwLock::new(AppState::new(config)))
}
