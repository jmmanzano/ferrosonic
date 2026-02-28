//! Application actions and message passing

/// Actions that can be sent to the audio backend
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AudioAction {
    /// Pause playback
    Pause,
    /// Resume playback
    Resume,
    /// Toggle pause state
    TogglePause,
    /// Stop playback
    Stop,
    /// Seek to position (seconds)
    Seek(f64),
    /// Seek relative to current position
    SeekRelative(f64),
    /// Skip to next track
    Next,
    /// Skip to previous track
    Previous,
    /// Set volume (0-100)
    SetVolume(i32),
}
