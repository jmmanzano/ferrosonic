//! Equalizer page with preset selection and band preview.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::state::AppState;
use crate::config::equalizer::EQ_BANDS_HZ;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let colors = *state.settings_state.theme_colors();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Equalizer ")
        .border_style(Style::default().fg(colors.border_focused));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 8 {
        return;
    }

    let cols = Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).split(inner);

    let mut left_lines = Vec::new();
    for (idx, preset) in state.settings_state.equalizer_presets.iter().enumerate() {
        let line = if idx == state.settings_state.equalizer_preset_index {
            format!("> {}", preset.name)
        } else {
            format!("  {}", preset.name)
        };
        left_lines.push(line);
    }

    let preset_list = Paragraph::new(left_lines.join("\n")).style(Style::default().fg(colors.highlight_fg));
    frame.render_widget(preset_list, cols[0]);

    let preset = state.settings_state.current_equalizer_preset();
    let mode = if state.settings_state.equalizer_enabled {
        "ON"
    } else {
        "OFF"
    };

    let mut right_lines = vec![format!("Status: {}", mode), format!("Preset: {}", preset.name), String::new()];

    for (idx, freq) in EQ_BANDS_HZ.iter().enumerate() {
        let gain = preset.bands[idx];
        let bars = gain_to_bar(gain);
        right_lines.push(format!("{:>5}Hz {:>6.1} dB {}", freq, gain, bars));
    }

    let detail = Paragraph::new(right_lines.join("\n")).style(Style::default().fg(colors.muted));
    frame.render_widget(detail, cols[1]);

    let help = Paragraph::new("Up/Down: select preset  Left/Right: prev/next  Enter: enable/disable").style(
        Style::default()
            .fg(colors.primary)
            .add_modifier(Modifier::BOLD),
    );

    let help_area = Rect::new(
        inner.x,
        inner.y + inner.height.saturating_sub(1),
        inner.width,
        1,
    );
    frame.render_widget(help, help_area);
}

fn gain_to_bar(gain: f32) -> String {
    let gain = gain.clamp(-20.0, 20.0);
    let steps = (((gain + 20.0) / 40.0) * 20.0).round() as usize;
    let mut out = String::with_capacity(20);
    for i in 0..20 {
        if i < steps {
            out.push('#');
        } else {
            out.push('.');
        }
    }
    out
}
