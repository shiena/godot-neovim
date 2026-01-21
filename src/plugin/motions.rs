//! Motion commands: scrolling, page movement, cursor positioning

use super::GodotNeovimPlugin;

impl GodotNeovimPlugin {
    /// Handle scroll and fold command sequences (za, zo, zc, zM, zR)
    /// Note: zz, zt, zb are now handled by Neovim via win_viewport events
    pub(super) fn handle_scroll_command(&mut self, keys: &str) -> bool {
        if self.last_key == "z" {
            match keys {
                // zz, zt, zb are handled by Neovim - just clear last_key but don't handle locally
                "z" | "t" | "b" => {
                    self.clear_last_key();
                    return false; // Let Neovim handle via win_viewport
                }
                "a" => {
                    self.toggle_fold();
                    self.clear_last_key();
                    return true;
                }
                "o" => {
                    self.unfold_current_line();
                    self.clear_last_key();
                    return true;
                }
                "c" => {
                    self.fold_current_line();
                    self.clear_last_key();
                    return true;
                }
                "M" => {
                    self.fold_all();
                    self.clear_last_key();
                    return true;
                }
                "R" => {
                    self.unfold_all();
                    self.clear_last_key();
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    // Note: zz, zt, zb, H, M, L are now handled by Neovim via win_viewport events
    // Local implementations have been removed

    /// Scroll viewport up (Ctrl+Y command)
    pub(super) fn scroll_viewport_up(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let first_visible = editor.get_first_visible_line();
        if first_visible > 0 {
            editor.set_line_as_first_visible(first_visible - 1);
        }

        crate::verbose_print!("[godot-neovim] Ctrl+Y: Scrolled viewport up");
    }

    /// Scroll viewport down (Ctrl+E command)
    pub(super) fn scroll_viewport_down(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let first_visible = editor.get_first_visible_line();
        let line_count = editor.get_line_count();
        let visible_lines = editor.get_visible_line_count();

        if first_visible < line_count - visible_lines {
            editor.set_line_as_first_visible(first_visible + 1);
        }

        crate::verbose_print!("[godot-neovim] Ctrl+E: Scrolled viewport down");
    }

    /// Move to start of line (0 command)
    pub(super) fn move_to_line_start(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line = editor.get_caret_line();
        self.move_cursor_to(line, 0);
        crate::verbose_print!("[godot-neovim] 0: Moved to start of line");
    }

    /// Move to first non-blank character (^ command)
    pub(super) fn move_to_first_non_blank(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();

        let first_non_blank = line_text
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);

        self.move_cursor_to(line_idx, first_non_blank as i32);
        crate::verbose_print!(
            "[godot-neovim] ^: Moved to first non-blank at col {}",
            first_non_blank
        );
    }

    /// Move to end of line ($ command)
    pub(super) fn move_to_line_end(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();
        let line_len = line_text.chars().count();

        // Vim's $ goes to last character, not past it
        let target_col = if line_len > 0 { line_len - 1 } else { 0 };
        self.move_cursor_to(line_idx, target_col as i32);
        crate::verbose_print!(
            "[godot-neovim] $: Moved to end of line at col {}",
            target_col
        );
    }

    /// Move to end of previous word (ge command)
    pub(super) fn move_to_word_end_backward(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let mut line = editor.get_caret_line();
        let mut col = editor.get_caret_column() as usize;

        // Get current line text
        let mut line_text = editor.get_line(line).to_string();
        let mut chars: Vec<char> = line_text.chars().collect();

        // If we're at or past the end of line, move to last character
        if col > 0 && col >= chars.len() {
            col = chars.len() - 1;
        }

        // Move back one position to start search
        if col > 0 {
            col -= 1;
        } else if line > 0 {
            // Move to previous line
            line -= 1;
            line_text = editor.get_line(line).to_string();
            chars = line_text.chars().collect();
            col = if chars.is_empty() { 0 } else { chars.len() - 1 };
        }

        // Skip whitespace going backward
        loop {
            if col < chars.len() && !chars[col].is_whitespace() {
                break;
            }
            if col > 0 {
                col -= 1;
            } else if line > 0 {
                line -= 1;
                line_text = editor.get_line(line).to_string();
                chars = line_text.chars().collect();
                col = if chars.is_empty() { 0 } else { chars.len() - 1 };
            } else {
                // At beginning of document
                self.move_cursor_to(0, 0);
                crate::verbose_print!("[godot-neovim] ge: At start of document");
                return;
            }
        }

        // We're now on a non-whitespace char - this is the end of the previous word
        self.move_cursor_to(line, col as i32);
        crate::verbose_print!(
            "[godot-neovim] ge: Moved to word end at {}:{}",
            line + 1,
            col
        );
    }

    /// Move cursor to specified position (Godot local only, does not sync to Neovim)
    /// Caller is responsible for sending the corresponding key to Neovim
    pub(super) fn move_cursor_to(&mut self, line: i32, col: i32) {
        // Set flag to prevent on_caret_changed from triggering sync_cursor_to_neovim
        self.syncing_from_grid = true;

        // Update last_synced_cursor BEFORE setting caret to prevent
        // caret_changed signal from triggering extra sync_cursor_to_neovim
        self.last_synced_cursor = (line as i64, col as i64);

        if let Some(ref mut editor) = self.current_editor {
            editor.set_caret_line(line);
            editor.set_caret_column(col);
            crate::verbose_print!("[godot-neovim] Moved cursor to {}:{}", line + 1, col);
        }

        // Update cached cursor position
        self.current_cursor = (line as i64, col as i64);

        // Clear flag
        self.syncing_from_grid = false;

        // Update display
        let display_cursor = (line as i64 + 1, col as i64);
        self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(display_cursor));
    }

    // Note: half_page_down (Ctrl+D), half_page_up (Ctrl+U), page_down (Ctrl+F),
    // and page_up (Ctrl+B) are now handled by Neovim via win_viewport events
    // for proper viewport synchronization

    /// Jump to matching bracket (% command)
    pub(super) fn jump_to_matching_bracket(&mut self) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        if col_idx >= chars.len() {
            return;
        }

        let current_char = chars[col_idx];
        let (target_char, search_forward) = match current_char {
            '(' => (')', true),
            ')' => ('(', false),
            '[' => (']', true),
            ']' => ('[', false),
            '{' => ('}', true),
            '}' => ('{', false),
            '<' => ('>', true),
            '>' => ('<', false),
            _ => {
                crate::verbose_print!("[godot-neovim] %: Not on a bracket");
                return;
            }
        };

        let line_count = editor.get_line_count();
        let mut depth = 1;

        if search_forward {
            // Search forward
            let mut line = line_idx;
            let mut col = col_idx + 1;

            while line < line_count {
                let text = editor.get_line(line).to_string();
                let line_chars: Vec<char> = text.chars().collect();

                while col < line_chars.len() {
                    if line_chars[col] == current_char {
                        depth += 1;
                    } else if line_chars[col] == target_char {
                        depth -= 1;
                        if depth == 0 {
                            self.move_cursor_to(line, col as i32);
                            crate::verbose_print!("[godot-neovim] %: Jump to {}:{}", line + 1, col);
                            return;
                        }
                    }
                    col += 1;
                }
                line += 1;
                col = 0;
            }
        } else {
            // Search backward
            let mut line = line_idx;
            let mut col = col_idx as i32 - 1;

            while line >= 0 {
                let text = editor.get_line(line).to_string();
                let line_chars: Vec<char> = text.chars().collect();

                if col < 0 {
                    col = line_chars.len() as i32 - 1;
                }

                while col >= 0 {
                    if line_chars[col as usize] == current_char {
                        depth += 1;
                    } else if line_chars[col as usize] == target_char {
                        depth -= 1;
                        if depth == 0 {
                            self.move_cursor_to(line, col);
                            crate::verbose_print!("[godot-neovim] %: Jump to {}:{}", line + 1, col);
                            return;
                        }
                    }
                    col -= 1;
                }
                line -= 1;
                if line >= 0 {
                    // Use chars().count() for character count, not byte length
                    let line_text = editor.get_line(line).to_string();
                    col = line_text.chars().count() as i32 - 1;
                }
            }
        }

        crate::verbose_print!("[godot-neovim] %: Matching bracket not found");
    }

    /// Move down by display line (gj command)
    /// If the current line is wrapped, moves to the next wrap segment.
    /// Otherwise, moves to the next logical line.
    pub(super) fn move_display_line_down(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let current_col = editor.get_caret_column();
        let line_count = editor.get_line_count();
        let wrap_count = editor.get_line_wrap_count(current_line);
        let current_wrap_index = editor.get_caret_wrap_index();

        if current_wrap_index < wrap_count {
            // Move to next wrap segment on same line
            // Get the wrapped text to find the start of next wrap segment
            let wrapped_text = editor.get_line_wrapped_text(current_line);
            if let Some(next_segment) = wrapped_text.get((current_wrap_index + 1) as usize) {
                // Calculate column offset for the next wrap segment
                let mut col_offset = 0i32;
                for i in 0..=current_wrap_index {
                    if let Some(seg) = wrapped_text.get(i as usize) {
                        col_offset += seg.len() as i32;
                    }
                }
                // Try to maintain similar column position in the wrap
                let target_col =
                    col_offset + (current_col - col_offset + next_segment.len() as i32).min(0);
                let target_col = target_col.max(col_offset);
                editor.set_caret_column(target_col);
            }
        } else {
            // Move to next logical line
            let target_line = (current_line + 1).min(line_count - 1);
            editor.set_caret_line(target_line);
        }

        self.sync_cursor_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] gj: wrap_count={}, wrap_index={}",
            wrap_count,
            current_wrap_index
        );
    }

    /// Move up by display line (gk command)
    /// If on a wrapped segment, moves to the previous wrap segment.
    /// Otherwise, moves to the previous logical line (last wrap segment).
    pub(super) fn move_display_line_up(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let current_wrap_index = editor.get_caret_wrap_index();

        if current_wrap_index > 0 {
            // Move to previous wrap segment on same line
            let wrapped_text = editor.get_line_wrapped_text(current_line);
            let mut col_offset = 0i32;
            for i in 0..(current_wrap_index - 1) {
                if let Some(seg) = wrapped_text.get(i as usize) {
                    col_offset += seg.len() as i32;
                }
            }
            editor.set_caret_column(col_offset);
        } else {
            // Move to previous logical line (at its last wrap segment if wrapped)
            let target_line = (current_line - 1).max(0);
            editor.set_caret_line(target_line);
            // Move to last wrap segment of previous line
            let prev_wrap_count = editor.get_line_wrap_count(target_line);
            if prev_wrap_count > 0 {
                let wrapped_text = editor.get_line_wrapped_text(target_line);
                let mut col_offset = 0i32;
                for i in 0..prev_wrap_count {
                    if let Some(seg) = wrapped_text.get(i as usize) {
                        col_offset += seg.len() as i32;
                    }
                }
                editor.set_caret_column(col_offset);
            }
        }

        self.sync_cursor_to_neovim();
        crate::verbose_print!("[godot-neovim] gk: wrap_index={}", current_wrap_index);
    }

    /// Move to start of display line (g0 command)
    /// If on a wrapped segment, moves to the start of that segment.
    pub(super) fn move_to_display_line_start(&mut self) {
        let (current_wrap_index, target_col) = {
            let Some(ref mut editor) = self.current_editor else {
                return;
            };

            let current_line = editor.get_caret_line();
            let current_wrap_index = editor.get_caret_wrap_index();

            let target_col = if current_wrap_index == 0 {
                // First wrap segment - move to column 0
                0
            } else {
                // Calculate column offset for this wrap segment
                let wrapped_text = editor.get_line_wrapped_text(current_line);
                let mut col_offset = 0i32;
                for i in 0..current_wrap_index {
                    if let Some(seg) = wrapped_text.get(i as usize) {
                        col_offset += seg.len() as i32;
                    }
                }
                col_offset
            };
            editor.set_caret_column(target_col);
            (current_wrap_index, target_col)
        };

        self.sync_cursor_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] g0: wrap_index={}, col={}",
            current_wrap_index,
            target_col
        );
    }

    /// Move to end of display line (g$ command)
    /// If on a wrapped segment, moves to the end of that segment.
    pub(super) fn move_to_display_line_end(&mut self) {
        let (current_wrap_index, target_col) = {
            let Some(ref mut editor) = self.current_editor else {
                return;
            };

            let current_line = editor.get_caret_line();
            let current_wrap_index = editor.get_caret_wrap_index();
            let wrap_count = editor.get_line_wrap_count(current_line);

            let wrapped_text = editor.get_line_wrapped_text(current_line);
            let mut col_offset = 0i32;
            let mut target_col = 0i32;

            for i in 0..=current_wrap_index {
                if let Some(seg) = wrapped_text.get(i as usize) {
                    if i == current_wrap_index {
                        // This is our segment - move to end (minus 1 for Vim behavior)
                        target_col = if current_wrap_index < wrap_count {
                            // Not the last segment - go to last char of segment
                            col_offset + seg.len() as i32 - 1
                        } else {
                            // Last segment - go to actual end of line
                            let line_len = editor.get_line(current_line).len() as i32;
                            (line_len - 1).max(0)
                        };
                        break;
                    }
                    col_offset += seg.len() as i32;
                }
            }
            editor.set_caret_column(target_col.max(0));
            (current_wrap_index, target_col)
        };

        self.sync_cursor_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] g$: wrap_index={}, col={}",
            current_wrap_index,
            target_col
        );
    }

    /// Move to first non-blank of display line (g^ command)
    pub(super) fn move_to_display_line_first_non_blank(&mut self) {
        let (current_wrap_index, target_col) = {
            let Some(ref mut editor) = self.current_editor else {
                return;
            };

            let current_line = editor.get_caret_line();
            let current_wrap_index = editor.get_caret_wrap_index();

            let wrapped_text = editor.get_line_wrapped_text(current_line);
            let mut col_offset = 0i32;
            let mut target_col = 0i32;

            for i in 0..=current_wrap_index {
                if let Some(seg) = wrapped_text.get(i as usize) {
                    if i == current_wrap_index {
                        // Find first non-whitespace in this segment
                        let seg_str = seg.to_string();
                        let mut found = false;
                        for (j, c) in seg_str.chars().enumerate() {
                            if !c.is_whitespace() {
                                target_col = col_offset + j as i32;
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            // All whitespace - just go to start
                            target_col = col_offset;
                        }
                        break;
                    }
                    col_offset += seg.len() as i32;
                }
            }
            editor.set_caret_column(target_col);
            (current_wrap_index, target_col)
        };

        self.sync_cursor_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] g^: wrap_index={}, col={}",
            current_wrap_index,
            target_col
        );
    }
}
