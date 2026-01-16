//! Neovim communication: buffer sync, cursor sync, key sending

use super::{CodeEditExt, GodotNeovimPlugin};
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Sync buffer from Godot editor to Neovim
    /// If clear_undo is true, the sync operation won't be recorded in undo history
    /// Uses Lua functions for proper undo history management
    pub(super) fn sync_buffer_to_neovim_impl(&mut self, clear_undo: bool) {
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
        let text = editor.get_text().to_string();
        let lines: Vec<String> = text.lines().map(String::from).collect();

        crate::verbose_print!("[godot-neovim] Syncing {} lines to Neovim", lines.len());
        if !lines.is_empty() {
            crate::verbose_print!(
                "[godot-neovim] First line: '{}'",
                lines[0].chars().take(50).collect::<String>()
            );
        }

        // Use Lua functions for proper undo history management
        // buffer_register: clears undo history (for initial load)
        // buffer_update: preserves undo history (for ESC from insert mode)
        let result = if clear_undo {
            client.buffer_register(lines)
        } else {
            client.buffer_update(lines)
        };

        match result {
            Ok(tick) => {
                crate::verbose_print!(
                    "[godot-neovim] Buffer synced to Neovim successfully (tick={})",
                    tick
                );

                // Reset sync manager and attach to buffer for change notifications
                self.sync_manager.reset();
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
                godot_error!("[godot-neovim] Failed to sync buffer: {}", e);
            }
        }

        // Set buffer name for LSP compatibility (if path changed)
        if !self.current_script_path.is_empty() {
            // Convert res:// path to absolute path for LSP
            use godot::classes::ProjectSettings;
            let abs_path = if self.current_script_path.starts_with("res://") {
                ProjectSettings::singleton()
                    .globalize_path(&self.current_script_path)
                    .to_string()
            } else {
                self.current_script_path.clone()
            };

            // Try to set buffer name, but don't block on errors (E325 swap file issues)
            match client.set_buffer_name(&abs_path) {
                Ok(()) => {
                    crate::verbose_print!("[godot-neovim] Buffer name set to: {}", abs_path);
                    // Set filetype for syntax highlighting
                    let _ = client.command("set filetype=gdscript");
                }
                Err(e) => {
                    // Log warning but continue - editing will still work
                    crate::verbose_print!("[godot-neovim] Buffer name not set: {}", e);
                    // Still set filetype for syntax highlighting
                    let _ = client.command("set filetype=gdscript");
                }
            }
        }
    }

    /// Sync buffer from Godot editor to Neovim (initial sync, clears undo history)
    pub(super) fn sync_buffer_to_neovim(&mut self) {
        self.sync_buffer_to_neovim_impl(true);
    }

    /// Sync buffer from Godot editor to Neovim (preserves undo history)
    /// Use this when syncing after ESC from insert mode
    pub(super) fn sync_buffer_to_neovim_keep_undo(&mut self) {
        self.sync_buffer_to_neovim_impl(false);
    }

    /// Sync cursor position from Godot editor to Neovim
    pub(super) fn sync_cursor_to_neovim(&mut self) {
        let Some(ref editor) = self.current_editor else {
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

        // Clamp line to Neovim buffer range to handle line count differences
        if let Ok(nvim_line_count) = client.get_line_count() {
            if nvim_line > nvim_line_count {
                crate::verbose_print!(
                    "[godot-neovim] Clamping line {} to Neovim max {}",
                    nvim_line,
                    nvim_line_count
                );
                nvim_line = nvim_line_count;
            }
        }

        crate::verbose_print!(
            "[godot-neovim] Syncing cursor to Neovim: line={}, col={}",
            nvim_line,
            nvim_col
        );

        if let Err(e) = client.set_cursor(nvim_line, nvim_col) {
            godot_error!("[godot-neovim] Failed to sync cursor: {}", e);
        }

        // Update cached cursor position
        drop(client);
        self.current_cursor = (line as i64, col as i64);
    }

    /// Send keys to Neovim and sync cursor
    /// Returns true if command completed, false if operator pending
    /// Note: Buffer changes are handled via nvim_buf_lines_event (event-driven)
    ///       Cursor sync is done synchronously (redraw events are unreliable)
    pub(super) fn send_keys(&mut self, keys: &str) -> bool {
        use crate::neovim::BufEvent;

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

        // Send input to Neovim
        if let Err(e) = client.input(keys) {
            godot_error!("[godot-neovim] Failed to send keys: {}", e);
            return false;
        }
        crate::verbose_print!("[godot-neovim] Key sent successfully");

        // Poll to receive buffer events from Neovim
        client.poll();

        // Query mode and cursor synchronously
        // (redraw events are batched and unreliable for immediate feedback)
        let (mode, blocking) = client.get_mode();
        let old_mode = self.current_mode.clone();
        self.current_mode = mode.clone();

        // Check for operator-pending mode (d, c, y waiting for motion)
        if blocking && mode != "i" && mode != "R" {
            crate::verbose_print!(
                "[godot-neovim] Operator pending (mode={}, blocking={})",
                mode,
                blocking
            );
            return false;
        }

        // Collect buffer events BEFORE getting cursor
        // (buffer changes affect cursor position in Godot)
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

        // Get cursor position
        let cursor = client.get_cursor().unwrap_or((1, 0));
        crate::verbose_print!(
            "[godot-neovim] After key: mode={}, cursor=({}, {})",
            mode,
            cursor.0,
            cursor.1
        );

        // Release lock before updating UI
        drop(client);

        // Apply buffer changes FIRST (before cursor sync)
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

        // Update cursor state (convert to 0-indexed)
        self.current_cursor = (cursor.0 - 1, cursor.1);

        // Sync cursor to Godot editor (after buffer is updated)
        self.sync_cursor_from_grid(self.current_cursor);

        // Update mode display
        self.update_mode_display_with_cursor(&mode, Some(cursor));

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

        // Sync Neovim's modified flag to Godot's dirty flag
        // When Neovim reports buffer as unmodified (e.g., after undo back to initial state),
        // update Godot's saved_version to clear the dirty marker
        self.sync_modified_flag();

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

        // Collect data from Neovim while holding lock, then release and process
        let (state_update, buf_events) = {
            let Some(ref neovim) = self.neovim else {
                return;
            };

            let Ok(client) = neovim.try_lock() else {
                return;
            };

            // Poll the runtime to process async events
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

            // Get state update
            let state_update = client.take_state();

            (state_update, buf_events)
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

        // Process state update (mode, cursor)
        if let Some((mode, cursor)) = state_update {
            crate::verbose_print!(
                "[godot-neovim] Got update: mode={}, cursor=({}, {})",
                mode,
                cursor.0,
                cursor.1
            );

            let old_mode = self.current_mode.clone();
            self.current_mode = mode.clone();
            self.current_cursor = cursor;

            // Update mode display
            // Convert grid cursor (0-indexed) to Neovim cursor (1-indexed for display)
            let display_cursor = (cursor.0 + 1, cursor.1);
            self.update_mode_display_with_cursor(&mode, Some(display_cursor));

            // Sync cursor to Godot editor
            self.sync_cursor_from_grid(cursor);

            // If exiting insert or replace mode, sync buffer from Godot to Neovim
            if (old_mode == "i" && mode != "i") || (old_mode == "R" && mode != "R") {
                self.sync_buffer_to_neovim();
            }

            // Handle visual mode selection
            let was_visual = Self::is_visual_mode(&old_mode);
            let is_visual = Self::is_visual_mode(&mode);

            if is_visual {
                // Update visual selection display
                if mode == "V" {
                    self.update_visual_line_selection();
                } else {
                    self.update_visual_selection();
                }
            } else if was_visual {
                // Exiting visual mode - clear selection
                self.clear_visual_selection();
            }
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
            (change.last_line as i32).min(line_count as i32)
        };

        // Handle different change types
        if change.new_lines.is_empty() {
            // Deletion: remove lines from first to last
            for line in (first..last).rev() {
                if line < editor.get_line_count() {
                    editor.remove_line_at(line);
                }
            }
        } else if first == last {
            // Insertion: insert new lines at first
            for (i, line_text) in change.new_lines.iter().enumerate() {
                let insert_at = first + i as i32;
                editor.insert_line_at(insert_at, line_text);
            }
        } else {
            // Replacement: delete old lines, insert new lines
            // First, delete old lines (in reverse to maintain indices)
            for line in (first..last).rev() {
                if line < editor.get_line_count() {
                    editor.remove_line_at(line);
                }
            }
            // Then, insert new lines
            for (i, line_text) in change.new_lines.iter().enumerate() {
                let insert_at = first + i as i32;
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

        // Update last_synced_cursor BEFORE setting caret to prevent
        // caret_changed signal from triggering sync_cursor_to_neovim
        self.last_synced_cursor = (safe_line as i64, safe_col as i64);

        editor.set_caret_line(safe_line);
        editor.set_caret_column(safe_col);
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
    /// When Neovim reports buffer as unmodified, update Godot's saved_version
    /// to clear the dirty marker (e.g., after undo back to initial state)
    pub(super) fn sync_modified_flag(&mut self) {
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
