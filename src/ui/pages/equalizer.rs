//! Equalizer page with preset selection and band preview.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
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

    let eq_editing = state.settings_state.eq_editing;
    let eq_renaming = state.settings_state.eq_renaming;
    let eq_selected_band = state.settings_state.eq_selected_band;
    let eq_rename_buffer = &state.settings_state.eq_rename_buffer;

    let cols =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).split(inner);

    // ── Left: preset list ────────────────────────────────────────────────────
    let mut left_lines = Vec::new();
    for (idx, preset) in state.settings_state.equalizer_presets.iter().enumerate() {
        let selected = idx == state.settings_state.equalizer_preset_index;
        let prefix = if selected { "> " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(colors.highlight_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors.muted)
        };
        left_lines.push(Line::from(Span::styled(format!("{}{}", prefix, preset.name), style)));
    }

    let preset_list = Paragraph::new(left_lines);
    frame.render_widget(preset_list, cols[0]);

    // ── Right: band detail ───────────────────────────────────────────────────
    let preset = state.settings_state.current_equalizer_preset();
    let mode = if state.settings_state.equalizer_enabled {
        "ON"
    } else {
        "OFF"
    };

    let mut right_lines: Vec<Line> = vec![Line::from(Span::styled(
        format!("Status: {}  Preset: {}", mode, preset.name),
        Style::default().fg(colors.highlight_fg),
    ))];

    if eq_renaming {
        right_lines.push(Line::from(Span::styled(
            format!("Rename: {}_", eq_rename_buffer),
            Style::default()
                .fg(colors.primary)
                .add_modifier(Modifier::BOLD),
        )));
    }

    right_lines.push(Line::from(""));

    for (idx, freq) in EQ_BANDS_HZ.iter().enumerate() {
        let gain = preset.bands[idx];
        let bars = gain_to_bar(gain);
        let line_text = format!("{:>5}Hz {:>+6.1} dB {}", freq, gain, bars);

        let style = if eq_renaming {
            Style::default().fg(colors.muted)
        } else if eq_editing && idx == eq_selected_band {
            // Highlighted selected band
            Style::default()
                .fg(colors.highlight_fg)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(colors.muted)
        };

        right_lines.push(Line::from(Span::styled(line_text, style)));
    }

    let detail = Paragraph::new(right_lines);
    frame.render_widget(detail, cols[1]);

    // ── Help bar ─────────────────────────────────────────────────────────────
    let help_text = if eq_renaming {
        "Type new name  Enter save rename  Backspace delete char  Esc cancel"
    } else if eq_editing {
        "↑↓ band  ←/→ ±0.5dB  ⇧←/⇧→ ±2dB  0 zero band  r reset all  Esc save & exit"
    } else {
        "↑↓ select  ←/→ prev-next  Enter on/off  e edit bands  R rename  n new  D delete"
    };

    let help = Paragraph::new(help_text).style(
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
