use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::config::equalizer::{
    delete_preset, rename_preset, save_preset, unique_custom_name, EqualizerPreset,
};
use crate::error::Error;

use super::*;

impl App {
    pub(super) async fn handle_equalizer_key(&mut self, key: event::KeyEvent) -> Result<(), Error> {
        let mut should_save_config = false;
        let mut should_apply = false;
        let mut save_to_disk: Option<EqualizerPreset> = None;

        {
            let mut state = self.state.write().await;

            if state.settings_state.eq_renaming {
                // ── Rename-mode: edit preset name text ────────────────────────────
                match key.code {
                    KeyCode::Esc => {
                        state.settings_state.eq_renaming = false;
                        state.settings_state.eq_rename_buffer.clear();
                        state.notify("EQ preset rename canceled".to_string());
                    }
                    KeyCode::Enter => {
                        let new_name = state.settings_state.eq_rename_buffer.trim().to_string();
                        if new_name.is_empty() {
                            state.notify_error("Preset name cannot be empty".to_string());
                        } else {
                            let idx = state.settings_state.equalizer_preset_index;
                            let old_name = state.settings_state.equalizer_preset_name().to_string();

                            let duplicate_exists = state
                                .settings_state
                                .equalizer_presets
                                .iter()
                                .enumerate()
                                .any(|(preset_idx, preset)| {
                                    preset_idx != idx && preset.name.eq_ignore_ascii_case(&new_name)
                                });

                            if duplicate_exists {
                                state.notify_error(format!(
                                    "A preset named \"{}\" already exists",
                                    new_name
                                ));
                            } else {
                                let mut renamed = state.settings_state.current_equalizer_preset().clone();
                                renamed.name = new_name.clone();

                                match rename_preset(&old_name, &renamed) {
                                    Ok(()) => {
                                        state.settings_state.equalizer_presets[idx] = renamed;
                                        state.settings_state.eq_renaming = false;
                                        state.settings_state.eq_rename_buffer.clear();
                                        should_save_config = true;
                                        state.config.equalizer_preset =
                                            state.settings_state.equalizer_preset_name().to_string();
                                        state.notify(format!(
                                            "Renamed preset: {} -> {}",
                                            old_name, new_name
                                        ));
                                    }
                                    Err(e) => {
                                        state.notify_error(format!("Failed to rename preset: {}", e));
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        state.settings_state.eq_rename_buffer.pop();
                    }
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                            && (c == ' ' || c.is_ascii_graphic()) =>
                    {
                        if state.settings_state.eq_rename_buffer.len() < 48 {
                            state.settings_state.eq_rename_buffer.push(c);
                        }
                    }
                    _ => {}
                }
            } else if state.settings_state.eq_editing {
                // ── Edit-mode: adjust band gains ──────────────────────────────
                match key.code {
                    KeyCode::Esc | KeyCode::Char('e') => {
                        state.settings_state.eq_editing = false;
                        save_to_disk = Some(state.settings_state.current_equalizer_preset().clone());
                        let name = state.settings_state.equalizer_preset_name().to_string();
                        state.notify(format!("EQ preset saved: {}", name));
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if state.settings_state.eq_selected_band > 0 {
                            state.settings_state.eq_selected_band -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if state.settings_state.eq_selected_band < 9 {
                            state.settings_state.eq_selected_band += 1;
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                            2.0f32
                        } else {
                            0.5f32
                        };
                        let band = state.settings_state.eq_selected_band;
                        let preset = state.settings_state.current_equalizer_preset_mut();
                        preset.bands[band] = (preset.bands[band] + step).min(20.0);
                        should_apply = state.settings_state.equalizer_enabled;
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                            2.0f32
                        } else {
                            0.5f32
                        };
                        let band = state.settings_state.eq_selected_band;
                        let preset = state.settings_state.current_equalizer_preset_mut();
                        preset.bands[band] = (preset.bands[band] - step).max(-20.0);
                        should_apply = state.settings_state.equalizer_enabled;
                    }
                    KeyCode::Char('0') => {
                        let band = state.settings_state.eq_selected_band;
                        state.settings_state.current_equalizer_preset_mut().bands[band] = 0.0;
                        should_apply = state.settings_state.equalizer_enabled;
                    }
                    KeyCode::Char('r') => {
                        state.settings_state.current_equalizer_preset_mut().bands = [0.0; 10];
                        should_apply = state.settings_state.equalizer_enabled;
                        state.notify("All bands reset to 0 dB".to_string());
                    }
                    _ => {}
                }
            } else {
                // ── Normal mode: navigate / create / delete presets ───────────
                let mut notify_eq_status = false;
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if state.settings_state.equalizer_preset_index > 0 {
                            state.settings_state.equalizer_preset_index -= 1;
                            should_save_config = true;
                            notify_eq_status = true;
                            should_apply = state.settings_state.equalizer_enabled;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let max =
                            state.settings_state.equalizer_presets.len().saturating_sub(1);
                        if state.settings_state.equalizer_preset_index < max {
                            state.settings_state.equalizer_preset_index += 1;
                            should_save_config = true;
                            notify_eq_status = true;
                            should_apply = state.settings_state.equalizer_enabled;
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        state.settings_state.prev_equalizer_preset();
                        should_save_config = true;
                        notify_eq_status = true;
                        should_apply = state.settings_state.equalizer_enabled;
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        state.settings_state.next_equalizer_preset();
                        should_save_config = true;
                        notify_eq_status = true;
                        should_apply = state.settings_state.equalizer_enabled;
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        state.settings_state.equalizer_enabled =
                            !state.settings_state.equalizer_enabled;
                        should_save_config = true;
                        notify_eq_status = true;
                        should_apply = true;
                    }
                    // Enter edit mode for the selected preset
                    KeyCode::Char('e') => {
                        state.settings_state.eq_editing = true;
                        state.settings_state.eq_selected_band = 0;
                        let name = state.settings_state.equalizer_preset_name().to_string();
                        state.notify(format!(
                            "Editing \"{}\": ↑↓ band  ←/→ ±0.5dB  ⇧←/⇧→ ±2dB  0 zero  r reset  Esc save",
                            name
                        ));
                    }
                    // Enter rename mode for selected preset
                    KeyCode::Char('R') => {
                        state.settings_state.eq_renaming = true;
                        state.settings_state.eq_rename_buffer =
                            state.settings_state.equalizer_preset_name().to_string();
                        state.notify(
                            "Rename preset: type a new name and press Enter (Esc to cancel)"
                                .to_string(),
                        );
                    }
                    // Create new preset as a copy of the current one
                    KeyCode::Char('n') => {
                        let new_name =
                            unique_custom_name(&state.settings_state.equalizer_presets);
                        let bands = state.settings_state.current_equalizer_preset().bands;
                        let new_preset = EqualizerPreset {
                            name: new_name.clone(),
                            bands,
                        };
                        match save_preset(&new_preset) {
                            Ok(()) => {
                                state.settings_state.equalizer_presets.push(new_preset);
                                state.settings_state.equalizer_presets.sort_by(|a, b| {
                                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                                });
                                if let Some(idx) = state
                                    .settings_state
                                    .equalizer_presets
                                    .iter()
                                    .position(|p| p.name == new_name)
                                {
                                    state.settings_state.equalizer_preset_index = idx;
                                }
                                should_save_config = true;
                                should_apply = state.settings_state.equalizer_enabled;
                                state.notify(format!("Created preset: {}", new_name));
                            }
                            Err(e) => {
                                state.notify_error(format!("Failed to create preset: {}", e));
                            }
                        }
                    }
                    // Delete the selected preset (Shift+D)
                    KeyCode::Char('D') => {
                        if state.settings_state.equalizer_presets.len() <= 1 {
                            state.notify_error("Cannot delete the last preset".to_string());
                        } else {
                            let name =
                                state.settings_state.equalizer_preset_name().to_string();
                            match delete_preset(&name) {
                                Ok(()) => {
                                    let idx = state.settings_state.equalizer_preset_index;
                                    state.settings_state.equalizer_presets.remove(idx);
                                    state.settings_state.equalizer_preset_index = idx.min(
                                        state
                                            .settings_state
                                            .equalizer_presets
                                            .len()
                                            .saturating_sub(1),
                                    );
                                    should_save_config = true;
                                    should_apply = state.settings_state.equalizer_enabled;
                                    state.notify(format!("Deleted preset: {}", name));
                                }
                                Err(e) => {
                                    state.notify_error(format!("Failed to delete: {}", e));
                                }
                            }
                        }
                    }
                    _ => {}
                }

                if should_save_config {
                    state.config.equalizer_enabled = state.settings_state.equalizer_enabled;
                    state.config.equalizer_preset =
                        state.settings_state.equalizer_preset_name().to_string();
                    if notify_eq_status {
                        let mode = if state.settings_state.equalizer_enabled {
                            "On"
                        } else {
                            "Off"
                        };
                        let preset = state.settings_state.equalizer_preset_name().to_string();
                        state.notify(format!("EQ: {} ({})", mode, preset));
                    }
                }
            }
        }

        // Save config to disk after normal-mode changes
        if should_save_config {
            let state = self.state.read().await;
            if let Err(e) = state.config.save_default() {
                drop(state);
                let mut state = self.state.write().await;
                state.notify_error(format!("Failed to save config: {}", e));
            }
        }

        // Save individual preset file when exiting edit mode
        if let Some(preset) = save_to_disk {
            if let Err(e) = save_preset(&preset) {
                let mut state = self.state.write().await;
                state.notify_error(format!("Failed to save preset: {}", e));
            }
        }

        if should_apply {
            self.schedule_equalizer_apply_debounced();
        }

        Ok(())
    }
}
