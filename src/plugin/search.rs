//! Search operations: character find, Neovim search

use super::GodotNeovimPlugin;

impl GodotNeovimPlugin {
    /// Find character forward on current line (f/t commands)
    pub(super) fn find_char_forward(&mut self, c: char, till: bool) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        // Search for character after cursor
        for (i, &ch) in chars.iter().enumerate().skip(col_idx + 1) {
            if ch == c {
                let target_col = if till { i - 1 } else { i };
                self.move_cursor_to(line_idx, target_col as i32);

                // Save for ; and ,
                self.last_find_char = Some(c);
                self.last_find_forward = true;
                self.last_find_till = till;

                crate::verbose_print!(
                    "[godot-neovim] {}{}: Found '{}' at col {}",
                    if till { "t" } else { "f" },
                    c,
                    c,
                    target_col
                );
                return;
            }
        }

        crate::verbose_print!("[godot-neovim] f/t: Character '{}' not found", c);
    }

    /// Find character backward on current line (F/T commands)
    pub(super) fn find_char_backward(&mut self, c: char, till: bool) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        // Search for character before cursor
        for i in (0..col_idx).rev() {
            if chars[i] == c {
                let target_col = if till { i + 1 } else { i };
                self.move_cursor_to(line_idx, target_col as i32);

                // Save for ; and ,
                self.last_find_char = Some(c);
                self.last_find_forward = false;
                self.last_find_till = till;

                crate::verbose_print!(
                    "[godot-neovim] {}{}: Found '{}' at col {}",
                    if till { "T" } else { "F" },
                    c,
                    c,
                    target_col
                );
                return;
            }
        }

        crate::verbose_print!("[godot-neovim] F/T: Character '{}' not found", c);
    }

    /// Repeat last f/F/t/T command (; and , commands)
    pub(super) fn repeat_find_char(&mut self, same_direction: bool) {
        let Some(c) = self.last_find_char else {
            crate::verbose_print!("[godot-neovim] ;/,: No previous find");
            return;
        };

        let forward = if same_direction {
            self.last_find_forward
        } else {
            !self.last_find_forward
        };
        let till = self.last_find_till;

        if forward {
            self.find_char_forward(c, till);
        } else {
            self.find_char_backward(c, till);
        }
    }

    /// Execute * or # word search: send to Neovim and sync cursor
    pub(super) fn search_word(&mut self, key: &str) {
        crate::verbose_print!("[godot-neovim] search_word: {}", key);

        // Send * or # to Neovim synchronously and sync cursor
        // Must use synchronous input to ensure search completes before getting cursor
        self.send_search_and_sync_cursor(key);
    }

    /// Execute n/N search: send to Neovim and sync cursor
    pub(super) fn search_next(&mut self, forward: bool) {
        let key = if forward { "n" } else { "N" };
        crate::verbose_print!("[godot-neovim] search_next: {}", key);

        // Send n or N to Neovim synchronously and sync cursor
        // Must use synchronous input to ensure search completes before getting cursor
        self.send_search_and_sync_cursor(key);
    }

    /// Open search mode (/ for forward, ? for backward)
    pub(super) fn open_search_mode(&mut self, forward: bool) {
        self.clear_pending_input_states();
        self.search_mode = true;
        self.search_forward = forward;
        self.search_buffer = if forward {
            "/".to_string()
        } else {
            "?".to_string()
        };

        // Show search prompt in mode label
        if let Some(ref mut label) = self.mode_label {
            label.set_text(&self.search_buffer);
        }

        crate::verbose_print!(
            "[godot-neovim] Search mode opened ({})",
            if forward { "forward" } else { "backward" }
        );
    }

    /// Close search mode
    pub(super) fn close_search_mode(&mut self) {
        self.search_mode = false;
        self.search_buffer.clear();

        // Restore mode display
        let display_cursor = (self.current_cursor.0 + 1, self.current_cursor.1);
        self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(display_cursor));

        crate::verbose_print!("[godot-neovim] Search mode closed");
    }

    /// Update search display in mode label
    pub(super) fn update_search_display(&mut self) {
        if let Some(ref mut label) = self.mode_label {
            label.set_text(&self.search_buffer);
        }
    }

    /// Execute the search: send to Neovim and sync cursor
    pub(super) fn execute_search(&mut self) {
        let search_pattern = self.search_buffer.clone();

        if search_pattern.len() <= 1 {
            // Empty search pattern (just / or ?), close without searching
            self.close_search_mode();
            return;
        }

        crate::verbose_print!("[godot-neovim] Executing search: {}", search_pattern);

        // Send search command to Neovim with Enter synchronously and sync cursor
        let nvim_cmd = format!("{}\r", search_pattern);
        self.send_search_and_sync_cursor(&nvim_cmd);

        self.close_search_mode();
    }

    /// Send search command to Neovim synchronously and sync cursor
    ///
    /// This function uses synchronous input instead of the async channel to ensure
    /// the search command is fully processed by Neovim before getting the cursor position.
    /// Without this, the cursor position returned would be from BEFORE the search.
    fn send_search_and_sync_cursor(&mut self, keys: &str) {
        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            crate::verbose_print!("[godot-neovim] Mutex busy, cannot send search");
            return;
        };

        // Send keys synchronously (waits for RPC acknowledgment)
        if let Err(e) = client.input(keys) {
            crate::verbose_print!("[godot-neovim] Failed to send search keys: {}", e);
            return;
        }

        // Poll to ensure Neovim processes the input and updates cursor
        // This gives Neovim time to execute the search and update state
        client.poll();

        // Get cursor position from Neovim
        match client.get_cursor() {
            Ok((line, col)) => {
                crate::verbose_print!(
                    "[godot-neovim] Search cursor from Neovim: ({}, {})",
                    line,
                    col
                );

                // Drop the lock before accessing editor
                drop(client);

                // Update Godot editor cursor (Neovim uses 1-indexed lines)
                if let Some(ref mut editor) = self.current_editor {
                    // Set flag to prevent on_caret_changed from triggering sync back
                    self.syncing_from_grid = true;
                    self.last_synced_cursor = ((line - 1), col);

                    editor.set_caret_line((line - 1) as i32);
                    editor.set_caret_column(col as i32);

                    // Center the view on cursor
                    editor.center_viewport_to_caret();

                    self.syncing_from_grid = false;
                }

                // Update internal cursor state
                self.current_cursor = (line - 1, col);

                // Update mode display with new cursor position
                let display_cursor = (line, col);
                self.update_mode_display_with_cursor(
                    &self.current_mode.clone(),
                    Some(display_cursor),
                );
            }
            Err(e) => {
                crate::verbose_print!("[godot-neovim] Failed to get cursor from Neovim: {}", e);
            }
        }
    }
}
