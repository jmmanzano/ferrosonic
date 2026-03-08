use tracing::warn;

use super::*;

impl App {
    pub(super) async fn apply_equalizer_from_state(&mut self) {
        let (enabled, preset) = {
            let state = self.state.read().await;
            (
                state.settings_state.equalizer_enabled,
                state.settings_state.current_equalizer_preset().clone(),
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

        if let Err(e) = self.audio_set_equalizer_filter(filter) {
            warn!("Failed to apply equalizer: {}", e);
            let mut state = self.state.write().await;
            state.notify_error(format!("Equalizer error: {}", e));
        }
    }
}
