//! Motion commands: scrolling, page movement, cursor positioning

use super::GodotNeovimPlugin;

impl GodotNeovimPlugin {
    /// Handle scroll command sequences (zz, zt, zb)
    pub(super) fn handle_scroll_command(&mut self, keys: &str) -> bool {
        if self.last_key == "z" {
            match keys {
                "z" => {
                    self.center_cursor();
                    self.last_key.clear();
                    return true;
                }
                "t" => {
                    self.scroll_cursor_to_top();
                    self.last_key.clear();
                    return true;
                }
                "b" => {
                    self.scroll_cursor_to_bottom();
                    self.last_key.clear();
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Center cursor on screen (zz command)
    fn center_cursor(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let visible_lines = editor.get_visible_line_count();
        let half_visible = visible_lines / 2;

        let target_first = (current_line - half_visible).max(0);
        editor.set_line_as_first_visible(target_first);

        crate::verbose_print!("[godot-neovim] zz: Centered cursor on line {}", current_line + 1);
    }

    /// Scroll cursor line to top (zt command)
    fn scroll_cursor_to_top(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        editor.set_line_as_first_visible(current_line);

        crate::verbose_print!("[godot-neovim] zt: Cursor line {} at top", current_line + 1);
    }

    /// Scroll cursor line to bottom (zb command)
    fn scroll_cursor_to_bottom(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let visible_lines = editor.get_visible_line_count();

        let target_first = (current_line - visible_lines + 1).max(0);
        editor.set_line_as_first_visible(target_first);

        crate::verbose_print!("[godot-neovim] zb: Cursor line {} at bottom", current_line + 1);
    }

    /// Move cursor to top of visible area (H command)
    pub(super) fn move_cursor_to_visible_top(&mut self) {
        let target_line = {
            let Some(ref mut editor) = self.current_editor else {
                return;
            };
            let first_visible = editor.get_first_visible_line();
            editor.set_caret_line(first_visible);
            editor.set_caret_column(0);
            first_visible
        };

        crate::verbose_print!("[godot-neovim] H: moved to line {}", target_line);

        // Sync to Neovim (non-blocking, errors are logged but ignored)
        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
    }

    /// Move cursor to middle of visible area (M command)
    pub(super) fn move_cursor_to_visible_middle(&mut self) {
        let target_line = {
            let Some(ref mut editor) = self.current_editor else {
                return;
            };
            let first_visible = editor.get_first_visible_line();
            let visible_lines = editor.get_visible_line_count();
            let middle_line = first_visible + visible_lines / 2;
            let line_count = editor.get_line_count();
            let target = middle_line.min(line_count - 1);
            editor.set_caret_line(target);
            editor.set_caret_column(0);
            target
        };

        crate::verbose_print!("[godot-neovim] M: moved to line {}", target_line);

        // Sync to Neovim (non-blocking, errors are logged but ignored)
        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
    }

    /// Move cursor to bottom of visible area (L command)
    pub(super) fn move_cursor_to_visible_bottom(&mut self) {
        let target_line = {
            let Some(ref mut editor) = self.current_editor else {
                return;
            };
            let last_visible = editor.get_last_full_visible_line();
            let line_count = editor.get_line_count();
            let target = last_visible.min(line_count - 1);
            editor.set_caret_line(target);
            editor.set_caret_column(0);
            target
        };

        crate::verbose_print!("[godot-neovim] L: moved to line {}", target_line);

        // Sync to Neovim (non-blocking, errors are logged but ignored)
        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
    }

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

    /// Move to previous paragraph ({ command)
    pub(super) fn move_to_prev_paragraph(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();

        // Skip current empty lines
        let mut line = current_line - 1;
        while line > 0 {
            let text = editor.get_line(line).to_string();
            if text.trim().is_empty() {
                line -= 1;
            } else {
                break;
            }
        }

        // Find previous empty line
        while line > 0 {
            let text = editor.get_line(line).to_string();
            if text.trim().is_empty() {
                break;
            }
            line -= 1;
        }

        self.move_cursor_to(line.max(0), 0);
        crate::verbose_print!("[godot-neovim] {{: Moved to line {}", line + 1);
    }

    /// Move to next paragraph (} command)
    pub(super) fn move_to_next_paragraph(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let line_count = editor.get_line_count();

        // Skip current non-empty lines
        let mut line = current_line + 1;
        while line < line_count {
            let text = editor.get_line(line).to_string();
            if !text.trim().is_empty() {
                line += 1;
            } else {
                break;
            }
        }

        // Skip empty lines
        while line < line_count {
            let text = editor.get_line(line).to_string();
            if text.trim().is_empty() {
                line += 1;
            } else {
                break;
            }
        }

        self.move_cursor_to(line.min(line_count - 1), 0);
        crate::verbose_print!("[godot-neovim] }}: Moved to line {}", line + 1);
    }

    /// Move cursor to specified position and sync with Neovim
    pub(super) fn move_cursor_to(&mut self, line: i32, col: i32) {
        if let Some(ref mut editor) = self.current_editor {
            editor.set_caret_line(line);
            editor.set_caret_column(col);
            crate::verbose_print!("[godot-neovim] Moved cursor to {}:{}", line + 1, col);
        }

        // Update cached cursor position
        self.current_cursor = (line as i64, col as i64);

        // Sync to Neovim
        self.sync_cursor_to_neovim();

        // Update display
        let display_cursor = (line as i64 + 1, col as i64);
        self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(display_cursor));
    }

    /// Move half page down (Ctrl+D command)
    pub(super) fn half_page_down(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let visible_lines = editor.get_visible_line_count();
        let half_page = visible_lines / 2;
        let current_line = editor.get_caret_line();
        let line_count = editor.get_line_count();

        let target_line = (current_line + half_page).min(line_count - 1);
        editor.set_caret_line(target_line);

        // Also scroll the viewport
        let first_visible = editor.get_first_visible_line();
        let new_first = (first_visible + half_page).min(line_count - visible_lines);
        if new_first > first_visible {
            editor.set_line_as_first_visible(new_first.max(0));
        }

        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
        crate::verbose_print!("[godot-neovim] Ctrl+D: Moved to line {}", target_line + 1);
    }

    /// Move half page up (Ctrl+U command)
    pub(super) fn half_page_up(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let visible_lines = editor.get_visible_line_count();
        let half_page = visible_lines / 2;
        let current_line = editor.get_caret_line();

        let target_line = (current_line - half_page).max(0);
        editor.set_caret_line(target_line);

        // Also scroll the viewport
        let first_visible = editor.get_first_visible_line();
        let new_first = (first_visible - half_page).max(0);
        if new_first < first_visible {
            editor.set_line_as_first_visible(new_first);
        }

        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
        crate::verbose_print!("[godot-neovim] Ctrl+U: Moved to line {}", target_line + 1);
    }

    /// Move full page down (Ctrl+F command)
    pub(super) fn page_down(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let visible_lines = editor.get_visible_line_count();
        let current_line = editor.get_caret_line();
        let line_count = editor.get_line_count();

        let target_line = (current_line + visible_lines).min(line_count - 1);
        editor.set_caret_line(target_line);

        // Also scroll the viewport
        let first_visible = editor.get_first_visible_line();
        let new_first = (first_visible + visible_lines).min(line_count - visible_lines);
        if new_first > first_visible {
            editor.set_line_as_first_visible(new_first.max(0));
        }

        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
        crate::verbose_print!("[godot-neovim] Ctrl+F: Moved to line {}", target_line + 1);
    }

    /// Move full page up (Ctrl+B command)
    pub(super) fn page_up(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let visible_lines = editor.get_visible_line_count();
        let current_line = editor.get_caret_line();

        let target_line = (current_line - visible_lines).max(0);
        editor.set_caret_line(target_line);

        // Also scroll the viewport
        let first_visible = editor.get_first_visible_line();
        let new_first = (first_visible - visible_lines).max(0);
        if new_first < first_visible {
            editor.set_line_as_first_visible(new_first);
        }

        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
        crate::verbose_print!("[godot-neovim] Ctrl+B: Moved to line {}", target_line + 1);
    }

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
                    col = editor.get_line(line).len() as i32 - 1;
                }
            }
        }

        crate::verbose_print!("[godot-neovim] %: Matching bracket not found");
    }
}
