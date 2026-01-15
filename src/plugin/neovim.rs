//! Neovim communication: buffer sync, cursor sync, key sending

use super::GodotNeovimPlugin;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Sync buffer from Godot editor to Neovim
    pub(super) fn sync_buffer_to_neovim(&mut self) {
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

        // Set buffer content in Neovim
        if let Err(e) = client.set_buffer_lines(0, -1, lines) {
            godot_error!("[godot-neovim] Failed to sync buffer: {}", e);
        } else {
            // Clear Neovim's modified flag since we synced from Godot
            client.set_buffer_not_modified();
            crate::verbose_print!("[godot-neovim] Buffer synced to Neovim successfully");
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

    /// Send keys to Neovim and update state
    /// Returns true if command completed, false if operator pending
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

        // Send input to Neovim
        if let Err(e) = client.input(keys) {
            godot_error!("[godot-neovim] Failed to send keys: {}", e);
            return false;
        }
        crate::verbose_print!("[godot-neovim] Key sent successfully");

        // Query mode - if blocking (operator-pending or insert mode), handle specially
        let (mode, blocking) = client.get_mode();

        // Track old mode for visual mode transitions before updating
        let old_mode = self.current_mode.clone();

        // Always update current_mode so is_insert_mode() works correctly
        self.current_mode = mode.clone();

        if blocking {
            // Insert/replace mode is "blocking" but we should update mode display
            if mode == "i" || mode == "R" {
                crate::verbose_print!(
                    "[godot-neovim] Entered {} mode",
                    if mode == "i" { "insert" } else { "replace" }
                );
                drop(client);
                self.update_mode_display_with_cursor(&mode, None);
                return true;
            }
            // True operator-pending (like waiting for motion after 'd')
            crate::verbose_print!("[godot-neovim] Operator pending, skipping sync");
            return false;
        }

        // Query cursor
        let cursor = client.get_cursor().unwrap_or((1, 0));

        // Get buffer content from Neovim
        let buffer_lines = client.get_buffer_lines(0, -1).ok();

        crate::verbose_print!(
            "[godot-neovim] After key: mode={}, cursor=({}, {}), lines={:?}",
            mode,
            cursor.0,
            cursor.1,
            buffer_lines.as_ref().map(|l| l.len())
        );

        // Release lock before updating UI
        drop(client);

        // Update cursor state
        self.current_cursor = (cursor.0 - 1, cursor.1); // Convert to 0-indexed

        // Sync buffer from Neovim to Godot
        if let Some(lines) = buffer_lines {
            self.sync_buffer_from_neovim(lines, Some(cursor));
        }

        // Update mode display
        self.update_mode_display_with_cursor(&mode, Some(cursor));

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

        true
    }

    /// Send keys to Neovim in insert mode and sync buffer (strict mode)
    pub(super) fn send_keys_insert_mode(&mut self, keys: &str) {
        crate::verbose_print!("[godot-neovim] send_keys_insert_mode: {}", keys);

        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            godot_warn!("[godot-neovim] Mutex busy, dropping key: {}", keys);
            return;
        };

        // Send input to Neovim
        if let Err(e) = client.input(keys) {
            godot_error!("[godot-neovim] Failed to send keys: {}", e);
            return;
        }

        // Get buffer and cursor from Neovim
        let lines = client.get_buffer_lines(0, -1).unwrap_or_default();
        let cursor = client.get_cursor().ok();

        // Release lock before syncing
        drop(client);

        // Sync buffer from Neovim to Godot
        self.sync_buffer_from_neovim(lines, cursor);
    }

    /// Send Escape to Neovim and force mode to normal
    pub(super) fn send_escape(&mut self) {
        crate::verbose_print!("[godot-neovim] send_escape");

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

        // Sync buffer and cursor from Godot to Neovim (user was typing in Godot)
        self.sync_buffer_to_neovim();
        self.sync_cursor_to_neovim();

        // Force mode to normal (ESC always returns to normal mode)
        self.current_mode = "n".to_string();

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
        // First, try to send any queued keys
        self.process_pending_keys();

        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Poll the runtime to process async events
        client.poll();

        // Check if there are pending updates
        if let Some((mode, cursor)) = client.take_state() {
            crate::verbose_print!(
                "[godot-neovim] Got update: mode={}, cursor=({}, {})",
                mode,
                cursor.0,
                cursor.1
            );

            // Release lock before updating UI
            drop(client);

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
            editor.set_text(&text);
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
}
