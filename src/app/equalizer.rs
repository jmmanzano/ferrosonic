use tracing::warn;
use std::time::{Duration, Instant};

use super::*;

const EQ_APPLY_DEBOUNCE: Duration = Duration::from_secs(1);

impl App {
    pub(super) fn schedule_equalizer_apply_debounced(&mut self) {
        self.pending_equalizer_apply_at = Some(Instant::now() + EQ_APPLY_DEBOUNCE);
    }

    pub(super) async fn maybe_apply_scheduled_equalizer(&mut self) {
        let Some(deadline) = self.pending_equalizer_apply_at else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }

        self.pending_equalizer_apply_at = None;
        self.apply_equalizer_from_state().await;
    }

    pub(super) async fn apply_equalizer_from_state(&mut self) {
        let (enabled, preset, playing_radio) = {
            let state = self.state.read().await;
            (
                state.settings_state.equalizer_enabled,
                state.settings_state.current_equalizer_preset().clone(),
                state.playing_radio,
            )
        };

        let filter = if enabled {
            if self.use_ffmpeg_backend {
                Some(preset.ffmpeg_filter_chain())
            } else {
                Some(preset.mpv_lavfi_filter())
            }
        } else {
            None
        };

        let result = if self.use_ffmpeg_backend && playing_radio {
            self.ffmpeg.set_equalizer_filter_restart_stream(filter)
        } else {
            self.audio_set_equalizer_filter(filter)
        };

        if let Err(e) = result {
            warn!("Failed to apply equalizer: {}", e);
            let mut state = self.state.write().await;
            state.notify_error(format!("Equalizer error: {}", e));
        }
    }
}
