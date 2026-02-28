//! Radio page showing internet radio stations

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::app::state::AppState;

pub fn render(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let colors = *state.settings_state.theme_colors();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Radio Stations ({}) ", state.radio.stations.len()))
        .border_style(Style::default().fg(colors.border_focused));

    if state.radio.stations.is_empty() {
        let hint = Paragraph::new("No internet radio stations found. Configure them in your Subsonic server.")
            .style(Style::default().fg(colors.muted))
            .block(block);
        frame.render_widget(hint, area);
        return;
    }

    let is_playing_radio = state.playing_radio;
    let current_song_title = state.now_playing.song.as_ref().map(|s| s.title.clone());

    let items: Vec<ListItem> = state
        .radio
        .stations
        .iter()
        .enumerate()
        .map(|(i, station)| {
            let is_selected = state.radio.selected == Some(i);
            let is_playing = is_playing_radio
                && current_song_title
                    .as_ref()
                    .map(|t| t == &station.name)
                    .unwrap_or(false);

            let indicator = if is_playing { "▶ " } else { "  " };

            let (name_style, url_style) = if is_playing {
                (
                    Style::default()
                        .fg(colors.playing)
                        .add_modifier(Modifier::BOLD),
                    Style::default().fg(colors.playing),
                )
            } else if is_selected {
                (
                    Style::default()
                        .fg(colors.primary)
                        .add_modifier(Modifier::BOLD),
                    Style::default().fg(colors.muted),
                )
            } else {
                (
                    Style::default().fg(colors.song),
                    Style::default().fg(colors.muted),
                )
            };

            let home_page = station
                .home_page_url
                .as_deref()
                .unwrap_or("");
            let url_info = if !home_page.is_empty() {
                format!(" ({})", home_page)
            } else {
                String::new()
            };

            let line = Line::from(vec![
                Span::styled(format!("{:3}. ", i + 1), Style::default().fg(colors.muted)),
                Span::styled(indicator, Style::default().fg(colors.playing)),
                Span::styled(station.name.clone(), name_style),
                Span::styled(url_info, url_style),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(colors.highlight_bg))
        .highlight_symbol("▸ ");

    let mut list_state = ListState::default();
    list_state.select(state.radio.selected);
    frame.render_stateful_widget(list, area, &mut list_state);
    state.radio.scroll_offset = list_state.offset();
}
