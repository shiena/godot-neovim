//! Editing operations: undo, redo, delete, replace, indent, join

use super::GodotNeovimPlugin;
use godot::classes::Input;
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Undo (u command)
    pub(super) fn undo(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Save current cursor position before undo
        let saved_line = editor.get_caret_line();
        let saved_col = editor.get_caret_column();

        editor.undo();

        // Godot's undo may move cursor to old position - restore to near the current position
        // Vim behavior: cursor moves to the line where the change was undone
        // Since we don't know where the change was, keep cursor at saved position if valid
        let line_count = editor.get_line_count();
        let target_line = saved_line.min(line_count - 1);
        let line_length = editor.get_line(target_line).len() as i32;
        let target_col = saved_col.min(line_length.max(0));
        editor.set_caret_line(target_line);
        editor.set_caret_column(target_col);

        crate::verbose_print!(
            "[godot-neovim] u: Undo (cursor kept at line {})",
            target_line + 1
        );

        // Sync buffer to Neovim after undo
        self.sync_buffer_to_neovim();
        self.sync_cursor_to_neovim();
    }

    /// Redo (Ctrl+R command)
    pub(super) fn redo(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Save current cursor position before redo
        let saved_line = editor.get_caret_line();
        let saved_col = editor.get_caret_column();

        editor.redo();

        // Keep cursor at saved position if valid
        let line_count = editor.get_line_count();
        let target_line = saved_line.min(line_count - 1);
        let line_length = editor.get_line(target_line).len() as i32;
        let target_col = saved_col.min(line_length.max(0));
        editor.set_caret_line(target_line);
        editor.set_caret_column(target_col);

        crate::verbose_print!(
            "[godot-neovim] Ctrl+R: Redo (cursor kept at line {})",
            target_line + 1
        );

        // Sync buffer to Neovim after redo
        self.sync_buffer_to_neovim();
        self.sync_cursor_to_neovim();
    }

    /// Delete character under cursor (x command)
    pub(super) fn delete_char_forward(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column();
        let line_text = editor.get_line(line_idx).to_string();

        if (col_idx as usize) < line_text.chars().count() {
            let mut chars: Vec<char> = line_text.chars().collect();
            chars.remove(col_idx as usize);
            let new_line: String = chars.into_iter().collect();

            // Update editor
            editor.set_line(line_idx, &new_line);

            // Adjust cursor if needed
            let new_len = new_line.chars().count();
            if col_idx as usize >= new_len && new_len > 0 {
                editor.set_caret_column((new_len - 1) as i32);
            }

            self.sync_buffer_to_neovim();
            crate::verbose_print!("[godot-neovim] x: Deleted char at col {}", col_idx);
        }
    }

    /// Delete character before cursor (X command)
    pub(super) fn delete_char_backward(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column();

        if col_idx > 0 {
            let line_text = editor.get_line(line_idx).to_string();
            let mut chars: Vec<char> = line_text.chars().collect();
            chars.remove((col_idx - 1) as usize);
            let new_line: String = chars.into_iter().collect();

            // Update editor
            editor.set_line(line_idx, &new_line);
            editor.set_caret_column(col_idx - 1);

            self.sync_buffer_to_neovim();
            crate::verbose_print!("[godot-neovim] X: Deleted char at col {}", col_idx - 1);
        }
    }

    /// Replace character under cursor (r command)
    pub(super) fn replace_char(&mut self, c: char) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column();
        let line_text = editor.get_line(line_idx).to_string();

        if (col_idx as usize) < line_text.chars().count() {
            let mut chars: Vec<char> = line_text.chars().collect();
            chars[col_idx as usize] = c;
            let new_line: String = chars.into_iter().collect();

            editor.set_line(line_idx, &new_line);
            self.sync_buffer_to_neovim();
            crate::verbose_print!("[godot-neovim] r{}: Replaced char at col {}", c, col_idx);
        }
    }

    /// Toggle case of character under cursor (~ command)
    pub(super) fn toggle_case(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column();
        let line_text = editor.get_line(line_idx).to_string();

        if (col_idx as usize) < line_text.chars().count() {
            let mut chars: Vec<char> = line_text.chars().collect();
            let c = chars[col_idx as usize];
            chars[col_idx as usize] = if c.is_uppercase() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c.to_uppercase().next().unwrap_or(c)
            };
            let new_line: String = chars.into_iter().collect();

            editor.set_line(line_idx, &new_line);

            // Move cursor forward (like Vim)
            let line_len = new_line.chars().count();
            if (col_idx as usize) < line_len - 1 {
                editor.set_caret_column(col_idx + 1);
            }

            self.sync_buffer_to_neovim();
            crate::verbose_print!("[godot-neovim] ~: Toggled case at col {}", col_idx);
        }
    }

    /// Enter replace mode (R command)
    pub(super) fn enter_replace_mode(&mut self) {
        // Send 'R' to Neovim to enter replace mode
        let completed = self.send_keys("R");
        if completed {
            self.last_key.clear();
        }
        crate::verbose_print!("[godot-neovim] R: Entered replace mode");
    }

    /// Indent current line (>> command)
    pub(super) fn indent_line(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();

        // Add a tab at the beginning
        let new_line = format!("\t{}", line_text);
        editor.set_line(line_idx, &new_line);

        // Move cursor to first non-blank
        let first_non_blank = new_line
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);
        editor.set_caret_column(first_non_blank as i32);

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] >>: Indented line {}", line_idx + 1);
    }

    /// Unindent current line (<< command)
    pub(super) fn unindent_line(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();

        // Remove leading whitespace (one level)
        let new_line = if let Some(stripped) = line_text.strip_prefix('\t') {
            stripped.to_string()
        } else if let Some(stripped) = line_text.strip_prefix("    ") {
            stripped.to_string()
        } else if let Some(stripped) = line_text.strip_prefix("  ") {
            stripped.to_string()
        } else if let Some(stripped) = line_text.strip_prefix(' ') {
            stripped.to_string()
        } else {
            line_text
        };

        editor.set_line(line_idx, &new_line);

        // Move cursor to first non-blank
        let first_non_blank = new_line
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);
        editor.set_caret_column(first_non_blank as i32);

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] <<: Unindented line {}", line_idx + 1);
    }

    /// Increment or decrement the number under/after cursor (Ctrl+A / Ctrl+X)
    pub(super) fn increment_number(&mut self, delta: i32) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        // Find number at or after cursor
        let mut num_start = None;
        let mut num_end = None;

        // Search for number starting at or after cursor position
        for i in col_idx..chars.len() {
            if chars[i].is_ascii_digit() {
                // Found start of number, check for negative sign before it
                if i > 0 && chars[i - 1] == '-' {
                    num_start = Some(i - 1);
                } else {
                    num_start = Some(i);
                }
                // Find end of number
                for j in i..=chars.len() {
                    if j == chars.len() || !chars[j].is_ascii_digit() {
                        num_end = Some(j);
                        break;
                    }
                }
                break;
            }
        }

        // If no number found after cursor, search from beginning
        if num_start.is_none() {
            for i in 0..col_idx.min(chars.len()) {
                if chars[i].is_ascii_digit() {
                    if i > 0 && chars[i - 1] == '-' {
                        num_start = Some(i - 1);
                    } else {
                        num_start = Some(i);
                    }
                    for j in i..=chars.len() {
                        if j == chars.len() || !chars[j].is_ascii_digit() {
                            num_end = Some(j);
                            break;
                        }
                    }
                    break;
                }
            }
        }

        let (start, end) = match (num_start, num_end) {
            (Some(s), Some(e)) => (s, e),
            _ => {
                crate::verbose_print!("[godot-neovim] Ctrl+A/X: No number found");
                return;
            }
        };

        // Parse the number
        let num_str: String = chars[start..end].iter().collect();
        let Ok(num) = num_str.parse::<i64>() else {
            crate::verbose_print!("[godot-neovim] Ctrl+A/X: Failed to parse number");
            return;
        };

        // Calculate new value
        let new_num = num + delta as i64;
        let new_num_str = new_num.to_string();

        // Build new line
        let prefix: String = chars[..start].iter().collect();
        let suffix: String = chars[end..].iter().collect();
        let new_line = format!("{}{}{}", prefix, new_num_str, suffix);

        editor.set_line(line_idx, &new_line);

        // Position cursor at end of number
        let new_end = start + new_num_str.len();
        editor.set_caret_column((new_end - 1) as i32);

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] Ctrl+{}: {} -> {}",
            if delta > 0 { "A" } else { "X" },
            num,
            new_num
        );
    }

    /// Join current line with next line (J command)
    pub(super) fn join_lines(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_count = editor.get_line_count();

        if line_idx >= line_count - 1 {
            crate::verbose_print!("[godot-neovim] J: Already on last line");
            return;
        }

        let current_line = editor.get_line(line_idx).to_string();
        let next_line = editor.get_line(line_idx + 1).to_string();

        // Join with a space, trimming leading whitespace from next line
        let current_trimmed = current_line.trim_end();
        let next_trimmed = next_line.trim_start();

        let new_line = if current_trimmed.is_empty() {
            next_trimmed.to_string()
        } else if next_trimmed.is_empty() {
            current_trimmed.to_string()
        } else {
            format!("{} {}", current_trimmed, next_trimmed)
        };

        // Position cursor at the join point
        let join_col = current_trimmed.chars().count();

        // Update text
        editor.set_line(line_idx, &new_line);

        // Remove the next line
        // Need to get full text, remove the line, and set it back
        let full_text = editor.get_text().to_string();
        let lines: Vec<&str> = full_text.lines().collect();
        let mut new_lines: Vec<&str> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if i as i32 == line_idx {
                new_lines.push(&new_line);
            } else if i as i32 != line_idx + 1 {
                new_lines.push(line);
            }
        }
        let new_text = new_lines.join("\n");
        editor.set_text(&new_text);

        // Restore cursor position
        editor.set_caret_line(line_idx);
        editor.set_caret_column(join_col as i32);

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] J: Joined lines {} and {}",
            line_idx + 1,
            line_idx + 2
        );
    }

    /// Go to definition (gd command) - uses Godot's built-in
    pub(super) fn go_to_definition(&self) {
        // Simulate F12 or Ctrl+Click to go to definition
        let mut key_event = godot::classes::InputEventKey::new_gd();
        key_event.set_keycode(Key::F12);
        key_event.set_pressed(true);

        Input::singleton().parse_input_event(&key_event);
        crate::verbose_print!("[godot-neovim] gd: Go to definition (F12)");
    }

    /// Go to file under cursor (gf command)
    pub(super) fn go_to_file_under_cursor(&mut self) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();

        // Extract file path from around cursor position
        // Look for patterns like: "res://path/to/file.gd", 'path/file.gd', path/file
        let chars: Vec<char> = line_text.chars().collect();

        if col_idx >= chars.len() {
            crate::verbose_print!("[godot-neovim] gf: Cursor at end of line");
            return;
        }

        // Find start and end of path-like text
        let path_chars = |c: char| {
            c.is_alphanumeric() || c == '/' || c == '.' || c == '_' || c == '-' || c == ':'
        };

        let mut start = col_idx;
        while start > 0 && path_chars(chars[start - 1]) {
            start -= 1;
        }

        let mut end = col_idx;
        while end < chars.len() && path_chars(chars[end]) {
            end += 1;
        }

        if start == end {
            crate::verbose_print!("[godot-neovim] gf: No file path under cursor");
            return;
        }

        let path: String = chars[start..end].iter().collect();
        crate::verbose_print!("[godot-neovim] gf: Extracted path: {}", path);

        // Try to open the file
        self.cmd_edit(&path);
    }
}
