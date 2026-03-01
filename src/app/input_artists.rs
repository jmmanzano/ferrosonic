use crossterm::event::{self, KeyCode};
use tracing::info;

use crate::error::Error;

use super::*;

impl App {
    /// Handle artists page keys
    pub(super) async fn handle_artists_key(&mut self, key: event::KeyEvent) -> Result<(), Error> {
        use crate::ui::pages::artists::{build_tree_items, TreeItem};

        let mut state = self.state.write().await;

        // Handle filter input mode
        if state.artists.filter_active {
            match key.code {
                KeyCode::Esc => {
                    state.artists.filter_active = false;
                    state.artists.filter.clear();
                }
                KeyCode::Enter => {
                    state.artists.filter_active = false;
                }
                KeyCode::Backspace => {
                    state.artists.filter.pop();
                }
                KeyCode::Char(c) => {
                    state.artists.filter.push(c);
                }
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Char('/') => {
                state.artists.filter_active = true;
            }
            KeyCode::Esc => {
                state.artists.filter.clear();
                state.artists.expanded.clear();
                state.artists.selected_index = Some(0);
            }
            KeyCode::Tab => {
                state.artists.focus = (state.artists.focus + 1) % 2;
            }
            KeyCode::Left => {
                state.artists.focus = 0;
            }
            KeyCode::Right => {
                // Move focus to songs (right pane)
                if !state.artists.songs.is_empty() {
                    state.artists.focus = 1;
                    if state.artists.selected_song.is_none() {
                        state.artists.selected_song = Some(0);
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if state.artists.focus == 0 {
                    // Tree navigation
                    let tree_items = build_tree_items(&state);
                    if let Some(sel) = state.artists.selected_index {
                        if sel > 0 {
                            state.artists.selected_index = Some(sel - 1);
                        }
                    } else if !tree_items.is_empty() {
                        state.artists.selected_index = Some(0);
                    }
                    // Preview album songs in right pane
                    let album_id = state
                        .artists
                        .selected_index
                        .and_then(|i| tree_items.get(i))
                        .and_then(|item| match item {
                            TreeItem::Album { album } => Some(album.id.clone()),
                            _ => None,
                        });
                    if let Some(album_id) = album_id {
                        drop(state);
                        if let Some(ref client) = self.subsonic {
                            if let Ok((_album, songs)) = client.get_album(&album_id).await {
                                let mut state = self.state.write().await;
                                state.artists.songs = songs;
                                state.artists.selected_song = Some(0);
                            }
                        }
                        return Ok(());
                    }
                } else {
                    // Song list
                    if let Some(sel) = state.artists.selected_song {
                        if sel > 0 {
                            state.artists.selected_song = Some(sel - 1);
                        }
                    } else if !state.artists.songs.is_empty() {
                        state.artists.selected_song = Some(0);
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if state.artists.focus == 0 {
                    // Tree navigation
                    let tree_items = build_tree_items(&state);
                    let max = tree_items.len().saturating_sub(1);
                    if let Some(sel) = state.artists.selected_index {
                        if sel < max {
                            state.artists.selected_index = Some(sel + 1);
                        }
                    } else if !tree_items.is_empty() {
                        state.artists.selected_index = Some(0);
                    }
                    // Preview album songs in right pane
                    let album_id = state
                        .artists
                        .selected_index
                        .and_then(|i| tree_items.get(i))
                        .and_then(|item| match item {
                            TreeItem::Album { album } => Some(album.id.clone()),
                            _ => None,
                        });
                    if let Some(album_id) = album_id {
                        drop(state);
                        if let Some(ref client) = self.subsonic {
                            if let Ok((_album, songs)) = client.get_album(&album_id).await {
                                let mut state = self.state.write().await;
                                state.artists.songs = songs;
                                state.artists.selected_song = Some(0);
                            }
                        }
                        return Ok(());
                    }
                } else {
                    // Song list
                    let max = state.artists.songs.len().saturating_sub(1);
                    if let Some(sel) = state.artists.selected_song {
                        if sel < max {
                            state.artists.selected_song = Some(sel + 1);
                        }
                    } else if !state.artists.songs.is_empty() {
                        state.artists.selected_song = Some(0);
                    }
                }
            }
            KeyCode::Enter => {
                if state.artists.focus == 0 {
                    // Get current tree item
                    let tree_items = build_tree_items(&state);
                    if let Some(idx) = state.artists.selected_index {
                        if let Some(item) = tree_items.get(idx) {
                            match item {
                                TreeItem::Artist { artist, expanded } => {
                                    let artist_id = artist.id.clone();
                                    let artist_name = artist.name.clone();
                                    let was_expanded = *expanded;

                                    if was_expanded {
                                        state.artists.expanded.remove(&artist_id);
                                    } else if !state.artists.albums_cache.contains_key(&artist_id) {
                                        drop(state);
                                        if let Some(ref client) = self.subsonic {
                                            match client.get_artist(&artist_id).await {
                                                Ok((_artist, albums)) => {
                                                    let mut state = self.state.write().await;
                                                    let count = albums.len();
                                                    state.artists.albums_cache.insert(artist_id.clone(), albums);
                                                    state.artists.expanded.insert(artist_id);
                                                    info!("Loaded {} albums for {}", count, artist_name);
                                                }
                                                Err(e) => {
                                                    let mut state = self.state.write().await;
                                                    state.notify_error(format!("Failed to load: {}", e));
                                                }
                                            }
                                        }
                                        return Ok(());
                                    } else {
                                        state.artists.expanded.insert(artist_id);
                                    }
                                }
                                TreeItem::Album { album } => {
                                    let album_id = album.id.clone();
                                    let album_name = album.name.clone();
                                    drop(state);

                                    if let Some(ref client) = self.subsonic {
                                        match client.get_album(&album_id).await {
                                            Ok((_album, songs)) => {
                                                if songs.is_empty() {
                                                    let mut state = self.state.write().await;
                                                    state.notify_error("Album has no songs");
                                                    return Ok(());
                                                }

                                                let mut state = self.state.write().await;
                                                let count = songs.len();
                                                state.queue.clear();
                                                state.queue.extend(songs.clone());
                                                state.artists.songs = songs;
                                                state.artists.selected_song = Some(0);
                                                state.artists.focus = 1;
                                                state.notify(format!("Playing album: {} ({} songs)", album_name, count));
                                                drop(state);

                                                return self.play_queue_position(0).await;
                                            }
                                            Err(e) => {
                                                let mut state = self.state.write().await;
                                                state.notify_error(format!("Failed to load album: {}", e));
                                            }
                                        }
                                    }
                                    return Ok(());
                                }
                            }
                        }
                    }
                } else {
                    // Play selected song from current position
                    if let Some(idx) = state.artists.selected_song {
                        if idx < state.artists.songs.len() {
                            let songs = state.artists.songs.clone();
                            state.queue.clear();
                            state.queue.extend(songs);
                            drop(state);

                            return self.play_queue_position(idx).await;
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if state.artists.focus == 1 {
                    state.artists.focus = 0;
                }
            }
            KeyCode::Char('e') => {
                if state.artists.focus == 1 {
                    if let Some(idx) = state.artists.selected_song {
                        if let Some(song) = state.artists.songs.get(idx).cloned() {
                            let title = song.title.clone();
                            state.queue.push(song);
                            state.notify(format!("Added to queue: {}", title));
                        }
                    }
                } else if !state.artists.songs.is_empty() {
                    let count = state.artists.songs.len();
                    let songs = state.artists.songs.clone();
                    state.queue.extend(songs);
                    state.notify(format!("Added {} songs to queue", count));
                }
            }
            KeyCode::Char('n') => {
                let insert_pos = state.queue_position.map(|p| p + 1).unwrap_or(0);
                if state.artists.focus == 1 {
                    if let Some(idx) = state.artists.selected_song {
                        if let Some(song) = state.artists.songs.get(idx).cloned() {
                            let title = song.title.clone();
                            state.queue.insert(insert_pos, song);
                            state.notify(format!("Playing next: {}", title));
                        }
                    }
                } else if !state.artists.songs.is_empty() {
                    let count = state.artists.songs.len();
                    let songs: Vec<_> = state.artists.songs.to_vec();
                    for (i, song) in songs.into_iter().enumerate() {
                        state.queue.insert(insert_pos + i, song);
                    }
                    state.notify(format!("Playing {} songs next", count));
                }
            }
            _ => {}
        }

        Ok(())
    }
}
