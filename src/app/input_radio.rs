use crossterm::event::{self, KeyCode};
use crate::error::Error;
use super::*;

impl App {
    pub(super) async fn handle_radio_key(&mut self, key: event::KeyEvent) -> Result<(), Error> {
        let mut state = self.state.write().await;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(sel) = state.radio.selected {
                    if sel > 0 {
                        state.radio.selected = Some(sel - 1);
                    }
                } else if !state.radio.stations.is_empty() {
                    state.radio.selected = Some(0);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = state.radio.stations.len().saturating_sub(1);
                if let Some(sel) = state.radio.selected {
                    if sel < max {
                        state.radio.selected = Some(sel + 1);
                    }
                } else if !state.radio.stations.is_empty() {
                    state.radio.selected = Some(0);
                }
            }
            KeyCode::Enter => {
                if let Some(idx) = state.radio.selected {
                    if let Some(station) = state.radio.stations.get(idx).cloned() {
                        drop(state);
                        return self.play_radio_station(&station).await;
                    }
                }
            }
            KeyCode::Char('s') => {
                // Stop radio playback
                let is_radio = state.playing_radio;
                drop(state);
                if is_radio {
                    let _ = self.stop_playback().await;
                }
                return Ok(());
            }
            _ => {}
        }
        Ok(())
    }
}
