//! Neovim communication: buffer sync, cursor sync, key sending

use super::{CodeEditExt, GodotNeovimPlugin};
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Switch to Neovim buffer for the current file
    /// Creates buffer if not exists, initializes content if new
    /// Returns cursor position from Neovim (for existing buffers)
    pub(super) fn switch_to_neovim_buffer(&mut self) -> Option<(i64, i64)> {
        let Some(ref editor) = self.current_editor else {
            crate::verbose_print!("[godot-neovim] switch_to_neovim_buffer: No current editor");
            return None;
        };

        let Some(ref neovim) = self.neovim else {
            crate::verbose_print!("[godot-neovim] switch_to_neovim_buffer: No neovim");
            return None;
        };

        let Ok(client) = neovim.try_lock() else {
            crate::verbose_print!("[godot-neovim] switch_to_neovim_buffer: Failed to lock");
            return None;
        };

        // Get absolute path for the buffer
        if self.current_script_path.is_empty() {
            crate::verbose_print!("[godot-neovim] switch_to_neovim_buffer: No script path");
            return None;
        }

        use godot::classes::ProjectSettings;
        let abs_path = if self.current_script_path.starts_with("res://") {
            ProjectSettings::singleton()
                .globalize_path(&self.current_script_path)
                .to_string()
        } else {
            self.current_script_path.clone()
        };

        // Get text from Godot and normalize: remove trailing newline to match Neovim's line count
        // Neovim treats trailing newline as implicit (eol option), not as an extra line
        let text = editor.get_text().to_string();
        let trimmed = text.trim_end_matches('\n');
        let lines: Vec<String> = if trimmed.is_empty() {
            vec!["".to_string()]
        } else {
            trimmed
                .split('\n')
                .map(|s| s.trim_end_matches('\r').to_string())
                .collect()
        };
        let godot_line_count = editor.get_line_count();
        crate::verbose_print!(
            "[godot-neovim] Switching to buffer: {} (text {} lines, Godot {} lines)",
            abs_path,
            lines.len(),
            godot_line_count
        );

        // Switch to buffer (creates if not exists)
        let nvim_line_count = lines.len() as i32;
        match client.switch_to_buffer(&abs_path, Some(lines)) {
            Ok(result) => {
                crate::verbose_print!(
                    "[godot-neovim] Buffer switched: bufnr={}, tick={}, is_new={}, cursor=({}, {})",
                    result.bufnr,
                    result.tick,
                    result.is_new,
                    result.cursor.0,
                    result.cursor.1
                );

                // Update sync manager
                self.sync_manager.reset();
                self.sync_manager.set_initial_sync_tick(result.tick);
                self.sync_manager.set_attached(result.attached);
                self.sync_manager.set_line_count(nvim_line_count);

                // Set filetype for syntax highlighting
                let _ = client.command("set filetype=gdscript");

                // Mark buffer as saved in Godot
                drop(client);
                if let Some(ref mut editor) = self.current_editor {
                    editor.tag_saved_version();
                }

                // Return cursor position (convert to 0-indexed line)
                Some((result.cursor.0 - 1, result.cursor.1))
            }
            Err(e) => {
                godot_error!("[godot-neovim] Failed to switch buffer: {}", e);
                None
            }
        }
    }

    /// Sync buffer from Godot editor to Neovim (for ESC from insert mode)
    /// Preserves undo history
    pub(super) fn sync_buffer_to_neovim_keep_undo(&mut self) {
        let Some(ref editor) = self.current_editor else {
            crate::verbose_print!("[godot-neovim] sync_buffer_to_neovim: No current editor");
            return;
        };

        let Some(ref neovim) = self.neovim else {
            crate::verbose_print!("[godot-neovim] sync_buffer_to_neovim: No neovim");
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            crate::verbose_print!("[godot-neovim] sync_buffer_to_neovim: Failed to lock");
            return;
        };

        // Get text from Godot editor
        // Use split('\n') and strip \r to handle both Unix and Windows line endings
        // Keep trailing empty line to match Godot's line count exactly
        let text = editor.get_text().to_string();
        let lines: Vec<String> = text
            .split('\n')
            .map(|s| s.trim_end_matches('\r').to_string())
            .collect();

        crate::verbose_print!("[godot-neovim] Syncing {} lines to Neovim", lines.len());

        // ESC sync: update buffer preserving undo history
        match client.buffer_update(lines) {
            Ok(tick) => {
                crate::verbose_print!("[godot-neovim] Buffer updated (tick={})", tick);

                // Reset sync manager and set initial sync tick to ignore echo
                self.sync_manager.reset();
                self.sync_manager.set_initial_sync_tick(tick);

                // Re-attach to buffer for change notifications
                match client.buf_attach_current() {
                    Ok(true) => {
                        self.sync_manager.set_attached(true);
                        crate::verbose_print!(
                            "[godot-neovim] buf_attach: attached with changedtick={}",
                            tick
                        );
                    }
                    Ok(false) => {
                        crate::verbose_print!("[godot-neovim] buf_attach: returned false");
                    }
                    Err(e) => {
                        crate::verbose_print!("[godot-neovim] buf_attach: error: {}", e);
                    }
                }
            }
            Err(e) => {
                godot_error!("[godot-neovim] Failed to update buffer: {}", e);
            }
        }
    }

    /// Sync buffer from Godot editor to Neovim (initial sync for file open)
    /// This is now a wrapper that calls switch_to_neovim_buffer
    pub(super) fn sync_buffer_to_neovim(&mut self) {
        // Use multi-buffer approach - switch to buffer for this file
        let _ = self.switch_to_neovim_buffer();
    }

    /// Sync cursor position from Godot editor to Neovim
    pub(super) fn sync_cursor_to_neovim(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            crate::verbose_print!("[godot-neovim] sync_cursor_to_neovim: No current editor");
            return;
        };

        let Some(ref neovim) = self.neovim else {
            crate::verbose_print!("[godot-neovim] sync_cursor_to_neovim: No neovim");
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            crate::verbose_print!("[godot-neovim] sync_cursor_to_neovim: Failed to lock");
            return;
        };

        // Get cursor from Godot (0-indexed)
        let line = editor.get_caret_line();
        let col = editor.get_caret_column();

        // Neovim uses 1-indexed lines, 0-indexed columns
        let mut nvim_line = (line + 1) as i64;
        let nvim_col = col as i64;

        // Clamp line to Neovim buffer range (use cached line count for performance)
        let nvim_line_count = self.sync_manager.get_line_count() as i64;
        let clamped = nvim_line_count > 0 && nvim_line > nvim_line_count;
        if clamped {
            nvim_line = nvim_line_count;

            // Skip if we've already synced to this clamped line (prevents loop with different columns)
            if self.last_nvim_synced_line == nvim_line {
                // Still update last_synced_cursor to prevent on_caret_changed from calling us again
                self.last_synced_cursor = (line as i64, col as i64);
                return;
            }

            crate::verbose_print!(
                "[godot-neovim] Clamping line {} to Neovim max {}",
                nvim_line,
                nvim_line_count
            );

            // Update last_synced_cursor to prevent immediate re-trigger
            self.last_synced_cursor = (line as i64, col as i64);
        }

        crate::verbose_print!(
            "[godot-neovim] Syncing cursor to Neovim: line={}, col={}",
            nvim_line,
            nvim_col
        );

        if let Err(e) = client.set_cursor(nvim_line, nvim_col) {
            godot_error!("[godot-neovim] Failed to sync cursor: {}", e);
        }

        // Update tracking
        drop(client);
        // Only track last_nvim_synced_line when clamping (to prevent repeated clamping)
        // Reset to -1 for normal syncs so next clamp will work
        self.last_nvim_synced_line = if clamped { nvim_line } else { -1 };
        let final_line = if clamped { nvim_line - 1 } else { line as i64 };
        self.current_cursor = (final_line, col as i64);
    }

    /// Send keys to Neovim (fire-and-forget - state comes via redraw events)
    /// Returns true always (keys queued for processing)
    /// Note: Keys are processed by Neovim's event loop, state updates come via redraw
    pub(super) fn send_keys(&mut self, keys: &str) -> bool {
        crate::verbose_print!("[godot-neovim] send_keys: {}", keys);

        let Some(ref neovim) = self.neovim else {
            crate::verbose_print!("[godot-neovim] No neovim");
            return false;
        };

        let Ok(client) = neovim.try_lock() else {
            // Queue the key for retry instead of dropping
            crate::verbose_print!("[godot-neovim] Mutex busy, queuing key: {}", keys);
            self.pending_keys.push_back(keys.to_string());
            return false;
        };

        // Send keys asynchronously (fire-and-forget)
        // State updates will come via redraw events (mode_change, grid_cursor_goto)
        if let Err(e) = client.input_async(keys) {
            godot_error!("[godot-neovim] Failed to send keys: {}", e);
            return false;
        }

        crate::verbose_print!("[godot-neovim] Key sent (async): {}", keys);

        // Sync modified flag only after undo/redo operations
        if keys == "u" || keys == "<C-r>" {
            self.pending_modified_sync = true;
        }

        true
    }

    /// Send Escape to Neovim and force mode to normal
    pub(super) fn send_escape(&mut self) {
        use crate::neovim::BufEvent;

        crate::verbose_print!("[godot-neovim] send_escape");

        // Save cursor position BEFORE any sync (buffer sync may trigger events that move cursor)
        let saved_cursor = self
            .current_editor
            .as_ref()
            .map(|editor| (editor.get_caret_line(), editor.get_caret_column()));

        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Send Escape to Neovim
        if let Err(e) = client.input("<Esc>") {
            godot_error!("[godot-neovim] Failed to send Escape: {}", e);
            return;
        }

        // Release lock
        drop(client);

        // Sync buffer from Godot to Neovim (user was typing in Godot)
        // Use keep_undo variant to preserve undo history so 'u' works
        // This will cause nvim_buf_lines_event which we need to handle
        self.sync_buffer_to_neovim_keep_undo();

        // Process any buffer events triggered by sync_buffer_to_neovim
        // to prevent them from moving cursor later
        let buf_events: Vec<BufEvent> = if let Some(ref neovim) = self.neovim {
            if let Ok(client) = neovim.try_lock() {
                client.poll();

                // Drain buffer events (they're echoes of our sync)
                if client.has_buf_events() {
                    let events_arc = client.get_buf_events();
                    let result = if let Ok(mut events_guard) = events_arc.try_lock() {
                        client.clear_buf_events_flag();
                        events_guard.drain(..).collect()
                    } else {
                        Vec::new()
                    };
                    result
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Discard buffer events as echoes (but still update changedtick)
        for event in buf_events {
            match event {
                BufEvent::Lines(buf_lines_event) => {
                    // Ignore content - this is echo from our sync
                    // But still update changedtick to keep sync state valid
                    self.sync_manager
                        .on_nvim_changedtick(buf_lines_event.changedtick);
                    crate::verbose_print!(
                        "[godot-neovim] Ignoring sync echo: lines {}..{} (tick={})",
                        buf_lines_event.first_line,
                        buf_lines_event.last_line,
                        buf_lines_event.changedtick
                    );
                }
                BufEvent::ChangedTick { tick, .. } => {
                    self.sync_manager.on_nvim_changedtick(tick);
                }
                BufEvent::Detach { buf } => {
                    crate::verbose_print!("[godot-neovim] Buffer {} detached", buf);
                    self.sync_manager.set_attached(false);
                }
            }
        }

        // Restore cursor position after handling buffer events
        if let Some((line, col)) = saved_cursor {
            if let Some(ref mut editor) = self.current_editor {
                self.last_synced_cursor = (line as i64, col as i64);
                editor.set_caret_line(line);
                editor.set_caret_column(col);
            }
            self.current_cursor = (line as i64, col as i64);
        }

        // Sync cursor to Neovim
        self.sync_cursor_to_neovim();

        // Force mode to normal (ESC always returns to normal mode)
        self.current_mode = "n".to_string();

        // Clear all pending states (Escape cancels everything)
        self.clear_last_key();
        self.pending_char_op = None;
        self.pending_mark_op = None;
        self.pending_macro_op = None;
        self.selected_register = None;
        self.count_buffer.clear();

        // Clear any visual selection
        self.clear_visual_selection();

        // Display cursor position (convert 0-indexed to 1-indexed for display)
        let display_cursor = (self.current_cursor.0 + 1, self.current_cursor.1);
        self.update_mode_display_with_cursor("n", Some(display_cursor));

        crate::verbose_print!("[godot-neovim] Escaped to normal mode, buffer synced");
    }

    /// Process any pending keys that were queued due to mutex contention
    fn process_pending_keys(&mut self) {
        // Process up to a few keys per frame to avoid blocking
        const MAX_KEYS_PER_FRAME: usize = 5;

        for _ in 0..MAX_KEYS_PER_FRAME {
            let Some(key) = self.pending_keys.pop_front() else {
                break;
            };

            let Some(ref neovim) = self.neovim else {
                // No neovim, discard pending keys
                self.pending_keys.clear();
                break;
            };

            let Ok(client) = neovim.try_lock() else {
                // Still busy, put the key back and try next frame
                self.pending_keys.push_front(key);
                break;
            };

            // Send the queued key
            if let Err(e) = client.input(&key) {
                godot_error!("[godot-neovim] Failed to send queued key '{}': {}", key, e);
            } else {
                crate::verbose_print!("[godot-neovim] Sent queued key: {}", key);
            }
        }
    }

    /// Process pending updates from Neovim redraw events
    pub(super) fn process_neovim_updates(&mut self) {
        use crate::neovim::BufEvent;

        // First, try to send any queued keys
        self.process_pending_keys();

        // Check if we need to sync modified flag after undo/redo
        let needs_modified_sync = self.pending_modified_sync;
        self.pending_modified_sync = false;

        // Collect data from Neovim while holding lock, then release and process
        let (state_from_redraw, buf_events) = {
            let Some(ref neovim) = self.neovim else {
                return;
            };

            let Ok(client) = neovim.try_lock() else {
                // Restore flags if we couldn't get lock
                self.pending_modified_sync = needs_modified_sync;
                return;
            };

            // Poll the runtime to process async events (including redraw)
            client.poll();

            // Collect buffer events
            let buf_events: Vec<BufEvent> = if client.has_buf_events() {
                let events_arc = client.get_buf_events();
                let result = if let Ok(mut events_guard) = events_arc.try_lock() {
                    client.clear_buf_events_flag();
                    events_guard.drain(..).collect()
                } else {
                    Vec::new()
                };
                result
            } else {
                Vec::new()
            };

            // Get state from redraw events (mode_change, grid_cursor_goto)
            // This is non-blocking and doesn't make RPC calls
            let state_from_redraw = client.take_state();
            if let Some((ref mode, cursor)) = state_from_redraw {
                crate::verbose_print!(
                    "[godot-neovim] State from redraw: mode={}, cursor=({}, {})",
                    mode,
                    cursor.0,
                    cursor.1
                );
            }

            (state_from_redraw, buf_events)
        };
        // Lock is now released

        // Process buffer events
        for event in buf_events {
            match event {
                BufEvent::Lines(buf_lines_event) => {
                    if let Some(change) = self.sync_manager.on_nvim_buf_lines(buf_lines_event) {
                        self.apply_nvim_change(&change);
                    }
                }
                BufEvent::ChangedTick { tick, .. } => {
                    self.sync_manager.on_nvim_changedtick(tick);
                }
                BufEvent::Detach { buf } => {
                    crate::verbose_print!("[godot-neovim] Buffer {} detached", buf);
                    self.sync_manager.set_attached(false);
                }
            }
        }

        // Process state update from redraw events
        if let Some((mode, cursor)) = state_from_redraw {
            let old_mode = self.current_mode.clone();
            self.current_mode = mode.clone();
            self.current_cursor = cursor;

            // Update mode display
            let display_cursor = (cursor.0 + 1, cursor.1);
            self.update_mode_display_with_cursor(&mode, Some(display_cursor));

            // Sync cursor to Godot editor
            self.sync_cursor_from_grid(cursor);

            // Handle visual mode selection
            let was_visual = Self::is_visual_mode(&old_mode);
            let is_visual = Self::is_visual_mode(&mode);

            if is_visual {
                if mode == "V" {
                    self.update_visual_line_selection();
                } else {
                    self.update_visual_selection();
                }
            } else if was_visual {
                self.clear_visual_selection();
            }
        }

        // Sync modified flag if pending (after undo/redo)
        if needs_modified_sync {
            self.sync_modified_flag();
        }
    }

    /// Apply a change from Neovim to Godot editor
    fn apply_nvim_change(&mut self, change: &crate::sync::DocumentChange) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        crate::verbose_print!(
            "[godot-neovim] Applying nvim change: lines {}..{} -> {} new lines",
            change.first_line,
            change.last_line,
            change.new_lines.len()
        );

        // Set flag to prevent echo back to Neovim
        self.sync_manager.begin_nvim_change();

        let line_count = editor.get_line_count() as i64;
        let first = change.first_line.max(0) as i32;
        let last = if change.last_line < 0 {
            line_count as i32
        } else {
            change.last_line as i32
        };

        // For full buffer replacement (first=0 and replacing most/all lines),
        // use set_text directly to avoid line count drift issues
        let is_full_replacement = first == 0
            && (last as i64 >= line_count - 1 || last as i64 >= change.new_lines.len() as i64);

        if is_full_replacement && !change.new_lines.is_empty() {
            // Full buffer replacement: use set_text for reliability
            let new_text = change.new_lines.join("\n");
            editor.set_text(&new_text);
            self.sync_manager.end_nvim_change();
            return;
        }

        // Handle different change types
        if change.new_lines.is_empty() {
            // Deletion: remove lines from first to last
            let safe_last = last.min(editor.get_line_count());
            for line in (first..safe_last).rev() {
                if line < editor.get_line_count() {
                    editor.remove_line_at(line);
                }
            }
        } else if first == last {
            // Insertion: insert new lines at first position
            let current_line_count = editor.get_line_count();
            if first >= current_line_count {
                // Appending at end of buffer: use set_text since insert_line_at is out of bounds
                let text = editor.get_text().to_string();
                let new_lines_text = change.new_lines.join("\n");
                let new_text = if text.ends_with('\n') {
                    format!("{}{}", text, new_lines_text)
                } else if text.is_empty() {
                    new_lines_text
                } else {
                    format!("{}\n{}", text, new_lines_text)
                };
                editor.set_text(&new_text);
            } else {
                // Insert within buffer
                for (i, line_text) in change.new_lines.iter().enumerate() {
                    let insert_at = first + i as i32;
                    editor.insert_line_at(insert_at, line_text);
                }
            }
        } else {
            // Partial replacement: delete old lines, insert new lines
            let safe_last = last.min(editor.get_line_count());
            // First, delete old lines (in reverse to maintain indices)
            for line in (first..safe_last).rev() {
                if line < editor.get_line_count() {
                    editor.remove_line_at(line);
                }
            }
            // Then, insert new lines
            for (i, line_text) in change.new_lines.iter().enumerate() {
                let insert_at = first + i as i32;
                if insert_at >= editor.get_line_count() {
                    // Need to append remaining lines
                    let text = editor.get_text().to_string();
                    let remaining_lines: Vec<&str> =
                        change.new_lines[i..].iter().map(|s| s.as_str()).collect();
                    let new_lines_text = remaining_lines.join("\n");
                    let new_text = if text.ends_with('\n') || text.is_empty() {
                        format!("{}{}", text, new_lines_text)
                    } else {
                        format!("{}\n{}", text, new_lines_text)
                    };
                    editor.set_text(&new_text);
                    break;
                }
                editor.insert_line_at(insert_at, line_text);
            }
        }

        self.sync_manager.end_nvim_change();
    }

    /// Sync cursor from Neovim grid position to Godot editor
    pub(super) fn sync_cursor_from_grid(&mut self, cursor: (i64, i64)) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let (row, col) = cursor;

        // Grid coordinates are 0-indexed
        let line = row as i32;
        let column = col as i32;

        let line_count = editor.get_line_count();
        let safe_line = line.min(line_count - 1).max(0);
        let safe_col = column.max(0);

        // Set flag to prevent on_caret_changed from triggering sync_cursor_to_neovim
        // This is needed because set_caret_line and set_caret_column are called separately,
        // which can trigger on_caret_changed with intermediate cursor positions
        self.syncing_from_grid = true;

        // Update last_synced_cursor BEFORE setting caret to prevent
        // caret_changed signal from triggering sync_cursor_to_neovim
        self.last_synced_cursor = (safe_line as i64, safe_col as i64);

        editor.set_caret_line(safe_line);
        editor.set_caret_column(safe_col);

        self.syncing_from_grid = false;
    }

    /// Sync buffer from Neovim to Godot editor
    #[allow(dead_code)]
    pub(super) fn sync_buffer_from_neovim(
        &mut self,
        lines: Vec<String>,
        cursor: Option<(i64, i64)>,
    ) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Join lines and set text
        let text = lines.join("\n");
        let current_text = editor.get_text().to_string();

        // Normalize trailing newlines for comparison to avoid false dirty flags
        // Neovim has implicit trailing newline (eol option), Godot may or may not include it
        let text_normalized = text.trim_end_matches('\n');
        let current_normalized = current_text.trim_end_matches('\n');

        if text_normalized != current_normalized {
            editor.set_text_and_notify(&text);
            crate::verbose_print!(
                "[godot-neovim] Buffer synced from Neovim ({} lines)",
                lines.len()
            );
        }

        // Sync cursor position
        if let Some((line, col)) = cursor {
            // Neovim uses 1-indexed lines, 0-indexed columns
            let target_line = (line - 1).max(0) as i32;
            let target_col = col.max(0) as i32;

            let line_count = editor.get_line_count();
            let safe_line = target_line.min(line_count - 1);

            // Update last_synced_cursor BEFORE setting caret to prevent
            // caret_changed signal from triggering sync_cursor_to_neovim
            self.last_synced_cursor = (safe_line as i64, target_col as i64);

            editor.set_caret_line(safe_line);
            editor.set_caret_column(target_col);
        }
    }

    /// Update cursor position from Godot editor and refresh display
    pub(super) fn update_cursor_from_editor(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line = editor.get_caret_line();
        let col = editor.get_caret_column();

        self.current_cursor = (line as i64, col as i64);

        // Update mode display with current cursor
        let display_cursor = (line as i64 + 1, col as i64);
        self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(display_cursor));
    }

    /// Sync Neovim's modified flag to Godot's dirty flag
    /// Called after undo/redo to handle the case where buffer returns to initial state
    fn sync_modified_flag(&mut self) {
        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Check if Neovim buffer is modified
        let is_modified = client.is_buffer_modified();
        drop(client);

        // If Neovim says buffer is not modified, clear Godot's dirty flag
        if !is_modified {
            if let Some(ref mut editor) = self.current_editor {
                editor.tag_saved_version();
                crate::verbose_print!(
                    "[godot-neovim] Buffer unmodified in Neovim, cleared Godot dirty flag"
                );
            }
        }
    }
}
