use crossterm::event::{self, KeyCode};

use crate::error::Error;

use super::*;

impl App {
    pub(super) async fn handle_equalizer_key(&mut self, key: event::KeyEvent) -> Result<(), Error> {
        let mut changed = false;
        let mut should_apply = false;

        {
            let mut state = self.state.write().await;

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if state.settings_state.equalizer_preset_index > 0 {
                        state.settings_state.equalizer_preset_index -= 1;
                        changed = true;
                        should_apply = state.settings_state.equalizer_enabled;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = state.settings_state.equalizer_presets.len().saturating_sub(1);
                    if state.settings_state.equalizer_preset_index < max {
                        state.settings_state.equalizer_preset_index += 1;
                        changed = true;
                        should_apply = state.settings_state.equalizer_enabled;
                    }
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    state.settings_state.prev_equalizer_preset();
                    changed = true;
                    should_apply = state.settings_state.equalizer_enabled;
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    state.settings_state.next_equalizer_preset();
                    changed = true;
                    should_apply = state.settings_state.equalizer_enabled;
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    state.settings_state.equalizer_enabled = !state.settings_state.equalizer_enabled;
                    changed = true;
                    should_apply = true;
                }
                _ => {}
            }

            if changed {
                state.config.equalizer_enabled = state.settings_state.equalizer_enabled;
                state.config.equalizer_preset = state.settings_state.equalizer_preset_name().to_string();

                let mode = if state.settings_state.equalizer_enabled {
                    "On"
                } else {
                    "Off"
                };
                let preset = state.settings_state.equalizer_preset_name().to_string();
                state.notify(format!("EQ: {} ({})", mode, preset));
            }
        }

        if changed {
            let state = self.state.read().await;
            if let Err(e) = state.config.save_default() {
                drop(state);
                let mut state = self.state.write().await;
                state.notify_error(format!("Failed to save: {}", e));
            }
        }

        if should_apply {
            self.schedule_equalizer_apply_debounced();
        }

        Ok(())
    }
}
