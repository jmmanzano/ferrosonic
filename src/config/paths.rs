//! Platform-specific configuration paths

use std::path::PathBuf;

/// Get the default config directory for ferrosonic
pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("ferrosonic"))
}

/// Get the default config file path
pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|p| p.join("config.toml"))
}

/// Get the themes directory path
pub fn themes_dir() -> Option<PathBuf> {
    config_dir().map(|p| p.join("themes"))
}

/// Get the equalizer presets directory path
pub fn equalizer_presets_dir() -> Option<PathBuf> {
    config_dir().map(|p| p.join("equalizer"))
}

/// Get the log file path
#[allow(dead_code)]
pub fn log_file() -> Option<PathBuf> {
    config_dir().map(|p| p.join("ferrosonic.log"))
}

/// Get persisted queue snapshot file path
pub fn queue_file() -> Option<PathBuf> {
    config_dir().map(|p| p.join("queue.json"))
}

/// Get persisted UI state snapshot file path
pub fn ui_state_file() -> Option<PathBuf> {
    config_dir().map(|p| p.join("ui_state.json"))
}

/// Get the MPV socket path
/// On Unix: a regular Unix domain socket in the temp directory.
/// On Windows: a named pipe path (\\.\.\pipe\ferrosonic-mpv).
pub fn mpv_socket_path() -> std::path::PathBuf {
    #[cfg(unix)]
    {
        std::env::temp_dir().join("ferrosonic-mpv.sock")
    }
    #[cfg(windows)]
    {
        // MPV on Windows uses a named pipe for IPC
        std::path::PathBuf::from(r"\\.\pipe\ferrosonic-mpv")
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::env::temp_dir().join("ferrosonic-mpv.sock")
    }
}

/// Ensure the config directory exists
#[allow(dead_code)]
pub fn ensure_config_dir() -> std::io::Result<PathBuf> {
    let dir = config_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine config directory",
        )
    })?;

    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }

    Ok(dir)
}
