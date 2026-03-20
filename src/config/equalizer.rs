//! Equalizer presets loading and filter generation.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::config::paths;

/// Fixed center frequencies for a 10-band equalizer.
pub const EQ_BANDS_HZ: [u32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
/// Fixed preamp applied before band filters to keep headroom and reduce clipping.
pub const EQ_PREAMP_DB: f32 = -5.0;

/// A single equalizer preset loaded from JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqualizerPreset {
    pub name: String,
    pub bands: [f32; 10],
}

impl EqualizerPreset {
    /// Validate and clamp gain values to a sane range in dB.
    pub fn normalized(mut self) -> Self {
        for gain in &mut self.bands {
            *gain = gain.clamp(-20.0, 20.0);
        }
        self
    }

    /// Build an FFmpeg-compatible 10-band equalizer chain.
    pub fn ffmpeg_filter_chain(&self) -> String {
        let mut filters = Vec::with_capacity(EQ_BANDS_HZ.len() + 1);
        filters.push(format!("volume={:.1}dB", EQ_PREAMP_DB));
        filters.extend(
            EQ_BANDS_HZ
                .iter()
                .zip(self.bands.iter())
                .map(|(freq, gain)| format!("equalizer=f={}:t=q:w=1.0:g={:.2}", freq, gain)),
        );
        filters.join(",")
    }

    /// Build a MPV lavfi filter expression for the same EQ curve.
    pub fn mpv_lavfi_filter(&self) -> String {
        format!("lavfi=[{}]", self.ffmpeg_filter_chain())
    }
}

pub fn default_presets() -> Vec<EqualizerPreset> {
    vec![
        EqualizerPreset {
            name: "reference".to_string(),
            bands: [1.0, 2.0, 1.0, 0.0, -1.0, 0.0, 1.0, 2.0, 2.0, 1.0],
        },
        EqualizerPreset {
            name: "modern_rock_indie".to_string(),
            bands: [3.0, 4.0, 3.0, 0.0, -2.0, 0.0, 3.0, 4.0, 3.0, 2.0],
        },
        EqualizerPreset {
            name: "warm_listening".to_string(),
            bands: [2.0, 3.0, 3.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 1.0],
        },
        EqualizerPreset {
            name: "bass_natural".to_string(),
            bands: [3.0, 4.0, 3.0, 1.0, 0.0, -1.0, -1.0, 0.0, 0.0, 0.0],
        },
        EqualizerPreset {
            name: "bass_punch".to_string(),
            bands: [4.0, 5.0, 4.0, 1.0, -1.0, -2.0, -1.0, 1.0, 1.0, 0.0],
        },
        EqualizerPreset {
            name: "vocal_clarity".to_string(),
            bands: [-2.0, -1.0, 0.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 0.0],
        },
        EqualizerPreset {
            name: "bright_detail".to_string(),
            bands: [-1.0, -1.0, 0.0, 0.0, 0.0, 1.0, 3.0, 4.0, 4.0, 3.0],
        },
        EqualizerPreset {
            name: "loudness".to_string(),
            bands: [4.0, 3.0, 2.0, 0.0, -1.0, 0.0, 2.0, 3.0, 4.0, 4.0],
        },
        EqualizerPreset {
            name: "soft_speakers".to_string(),
            bands: [5.0, 4.0, 2.0, 0.0, -1.0, -1.0, 1.0, 3.0, 4.0, 5.0],
        },
        EqualizerPreset {
            name: "night_listening".to_string(),
            bands: [2.0, 2.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0],
        },
    ]
}

/// Ensure default preset files exist in the user presets directory.
pub fn seed_default_presets() {
    let Some(dir) = paths::equalizer_presets_dir() else {
        return;
    };

    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }

    // Remove legacy built-in preset files so the new built-ins replace them.
    let legacy_default_names = [
        "Flat",
        "Rock",
        "Pop",
        "Clasica",
        "Auditorio",
        "Radio",
        "Indie",
        "Jazz",
        "Vocal",
    ];
    for legacy_name in legacy_default_names {
        let filename = sanitize_filename(legacy_name);
        let path = dir.join(format!("{}.json", filename));
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }

    for preset in default_presets() {
        let filename = sanitize_filename(&preset.name);
        let path = dir.join(format!("{}.json", filename));
        if path.exists() {
            continue;
        }

        if let Ok(data) = serde_json::to_string_pretty(&preset) {
            let _ = std::fs::write(path, data);
        }
    }
}

/// Load all available equalizer presets from JSON files.
/// Invalid files are ignored and reported to logs.
pub fn load_presets() -> Vec<EqualizerPreset> {
    let Some(dir) = paths::equalizer_presets_dir() else {
        return default_presets();
    };

    if !dir.exists() {
        return default_presets();
    }

    let mut presets = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_json_file(&path) {
                continue;
            }

            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<EqualizerPreset>(&content) {
                    Ok(preset) => presets.push(preset.normalized()),
                    Err(e) => warn!("Invalid EQ preset {}: {}", path.display(), e),
                },
                Err(e) => warn!("Failed to read EQ preset {}: {}", path.display(), e),
            }
        }
    }

    if presets.is_empty() {
        return default_presets();
    }

    presets.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    presets
}

fn sanitize_filename(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c == ' ' || c == '-' || c == '_' {
            out.push('_');
        }
    }
    if out.is_empty() {
        "preset".to_string()
    } else {
        out
    }
}

fn is_json_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

/// Save a single preset to disk (creates or overwrites the JSON file).
pub fn save_preset(preset: &EqualizerPreset) -> Result<(), String> {
    let dir = paths::equalizer_presets_dir()
        .ok_or_else(|| "Cannot determine presets directory".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let filename = sanitize_filename(&preset.name);
    let path = dir.join(format!("{}.json", filename));
    let data = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
    std::fs::write(path, data).map_err(|e| e.to_string())
}

/// Rename a preset file by writing the new preset and removing the old file if needed.
pub fn rename_preset(old_name: &str, new_preset: &EqualizerPreset) -> Result<(), String> {
    let dir = paths::equalizer_presets_dir()
        .ok_or_else(|| "Cannot determine presets directory".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let old_path = dir.join(format!("{}.json", sanitize_filename(old_name)));
    let new_path = dir.join(format!("{}.json", sanitize_filename(&new_preset.name)));

    let data = serde_json::to_string_pretty(new_preset).map_err(|e| e.to_string())?;
    std::fs::write(&new_path, data).map_err(|e| e.to_string())?;

    if old_path != new_path && old_path.exists() {
        std::fs::remove_file(old_path).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Delete the JSON file for a preset by its name.
pub fn delete_preset(name: &str) -> Result<(), String> {
    let dir = paths::equalizer_presets_dir()
        .ok_or_else(|| "Cannot determine presets directory".to_string())?;
    let filename = sanitize_filename(name);
    let path = dir.join(format!("{}.json", filename));
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    } else {
        // Preset may be a built-in without a file; treat as success
        Ok(())
    }
}

/// Generate a unique name for a new custom preset that does not clash with existing ones.
pub fn unique_custom_name(existing: &[EqualizerPreset]) -> String {
    let names: std::collections::HashSet<String> =
        existing.iter().map(|p| p.name.to_lowercase()).collect();
    for i in 1u32.. {
        let candidate = format!("custom_{}", i);
        if !names.contains(&candidate) {
            return candidate;
        }
    }
    "custom".to_string()
}
