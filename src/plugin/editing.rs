//! Editing operations: undo, redo, delete, replace, indent, join

use super::GodotNeovimPlugin;
use godot::classes::{EditorInterface, Input, Os};
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

    /// Join lines without space (gJ command)
    pub(super) fn join_lines_no_space(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_count = editor.get_line_count();

        if line_idx >= line_count - 1 {
            crate::verbose_print!("[godot-neovim] gJ: Already on last line");
            return;
        }

        let current_line = editor.get_line(line_idx).to_string();
        let next_line = editor.get_line(line_idx + 1).to_string();

        // Join without space (only trim leading whitespace from next line)
        let next_trimmed = next_line.trim_start();
        let new_line = format!("{}{}", current_line, next_trimmed);

        // Position cursor at the join point
        let join_col = current_line.chars().count();

        // Rebuild text without the next line
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
            "[godot-neovim] gJ: Joined lines {} and {} without space",
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

    /// Show character info under cursor (ga command)
    pub(super) fn show_char_info(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        if col_idx >= chars.len() {
            self.show_status_message("NUL");
            return;
        }

        let c = chars[col_idx];
        let code = c as u32;

        // Format: <char> decimal, Hex hex, Oct octal
        let msg = if c.is_control() || c == ' ' {
            // For control characters and space, show descriptive name
            let name = match c {
                ' ' => "Space",
                '\t' => "Tab",
                '\n' => "NL",
                '\r' => "CR",
                _ => "Ctrl",
            };
            format!("<{}> {}, Hex {:02x}, Oct {:03o}", name, code, code, code)
        } else {
            format!("<{}> {}, Hex {:02x}, Oct {:03o}", c, code, code, code)
        };

        self.show_status_message(&msg);
        crate::verbose_print!("[godot-neovim] ga: {}", msg);
    }

    /// Show file info (Ctrl+G command)
    pub(super) fn show_file_info(&mut self) {
        let Some(ref editor) = self.current_editor else {
            self.show_status_message("No file");
            return;
        };

        let current_line = editor.get_caret_line() + 1; // 1-indexed for display
        let total_lines = editor.get_line_count();
        let percent = if total_lines > 0 {
            (current_line * 100) / total_lines
        } else {
            0
        };

        // Try to get the script path
        let file_name = if let Some(mut script_edit) =
            EditorInterface::singleton().get_script_editor()
        {
            if let Some(current_script) = script_edit.get_current_script() {
                let path = current_script.get_path().to_string();
                if path.is_empty() {
                    "[New File]".to_string()
                } else {
                    // Extract just the filename from path
                    path.split('/').last().unwrap_or(&path).to_string()
                }
            } else {
                "[No Script]".to_string()
            }
        } else {
            "[Unknown]".to_string()
        };

        let msg = format!(
            "\"{}\" line {} of {} --{}%--",
            file_name, current_line, total_lines, percent
        );

        self.show_status_message(&msg);
        crate::verbose_print!("[godot-neovim] Ctrl+G: {}", msg);
    }

    /// Show a temporary message in the status line
    pub(super) fn show_status_message(&mut self, msg: &str) {
        let Some(ref mut label) = self.mode_label else {
            return;
        };

        label.set_text(&format!(" {} ", msg));
        // Use white color for info messages
        label.add_theme_color_override("font_color", godot::prelude::Color::from_rgb(1.0, 1.0, 1.0));
    }

    /// Insert at column 0 (gI command)
    pub(super) fn insert_at_column_zero(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Save last insert position
        let line_idx = editor.get_caret_line();
        self.last_insert_position = Some((line_idx, 0));

        // Move cursor to column 0
        editor.set_caret_column(0);

        // Enter insert mode by sending 'i' to Neovim
        self.sync_cursor_to_neovim();
        self.send_keys("i");
        crate::verbose_print!(
            "[godot-neovim] gI: Insert at column 0, line {}",
            line_idx + 1
        );
    }

    /// Insert at last insert position (gi command)
    pub(super) fn insert_at_last_position(&mut self) {
        let Some((line, col)) = self.last_insert_position else {
            // No previous insert position - just enter insert mode
            self.send_keys("i");
            crate::verbose_print!("[godot-neovim] gi: No previous insert position, entering insert mode");
            return;
        };

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Clamp to valid range
        let line_count = editor.get_line_count();
        let target_line = line.min(line_count - 1);
        let line_len = editor.get_line(target_line).len() as i32;
        let target_col = col.min(line_len);

        // Move to last insert position
        editor.set_caret_line(target_line);
        editor.set_caret_column(target_col);

        // Enter insert mode
        self.sync_cursor_to_neovim();
        self.send_keys("i");
        crate::verbose_print!(
            "[godot-neovim] gi: Insert at last position ({}, {})",
            target_line + 1,
            target_col
        );
    }

    /// Save current position as last insert position (called when entering insert mode)
    pub(super) fn save_insert_position(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line = editor.get_caret_line();
        let col = editor.get_caret_column();
        self.last_insert_position = Some((line, col));
    }

    /// Substitute character under cursor (s command)
    pub(super) fn substitute_char(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        // Save insert position
        self.last_insert_position = Some((line_idx, col_idx as i32));

        if col_idx < chars.len() {
            // Delete the character
            let mut new_chars = chars.clone();
            new_chars.remove(col_idx);
            let new_line: String = new_chars.into_iter().collect();
            editor.set_line(line_idx, &new_line);
        }

        // Enter insert mode
        self.sync_buffer_to_neovim();
        self.sync_cursor_to_neovim();
        self.send_keys("i");
        crate::verbose_print!("[godot-neovim] s: Substitute char at col {}", col_idx);
    }

    /// Substitute entire line (S/cc command)
    pub(super) fn substitute_line(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();

        // Preserve indentation
        let indent: String = line_text.chars().take_while(|c| c.is_whitespace()).collect();

        // Save insert position (at end of indent)
        self.last_insert_position = Some((line_idx, indent.len() as i32));

        // Replace line with just the indentation
        editor.set_line(line_idx, &indent);
        editor.set_caret_column(indent.len() as i32);

        // Enter insert mode
        self.sync_buffer_to_neovim();
        self.sync_cursor_to_neovim();
        self.send_keys("i");
        crate::verbose_print!("[godot-neovim] S/cc: Substitute line {}", line_idx + 1);
    }

    /// Delete from cursor to end of line (D command)
    pub(super) fn delete_to_eol(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        if col_idx >= chars.len() {
            return;
        }

        // Delete from cursor to end
        let new_line: String = chars[..col_idx].iter().collect();
        editor.set_line(line_idx, &new_line);

        // Adjust cursor if needed
        let new_len = new_line.chars().count();
        if new_len > 0 {
            editor.set_caret_column((new_len - 1) as i32);
        } else {
            editor.set_caret_column(0);
        }

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] D: Deleted to end of line from col {}", col_idx);
    }

    /// Change from cursor to end of line (C command)
    pub(super) fn change_to_eol(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        // Save insert position
        self.last_insert_position = Some((line_idx, col_idx as i32));

        // Delete from cursor to end
        let new_line: String = chars[..col_idx].iter().collect();
        editor.set_line(line_idx, &new_line);
        editor.set_caret_column(col_idx as i32);

        // Sync buffer and enter insert mode
        self.sync_buffer_to_neovim();
        self.sync_cursor_to_neovim();
        self.send_keys("i");
        crate::verbose_print!("[godot-neovim] C: Changed to end of line from col {}", col_idx);
    }

    /// Yank from cursor to end of line (Y command)
    pub(super) fn yank_to_eol(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        if col_idx >= chars.len() {
            crate::verbose_print!("[godot-neovim] Y: Cursor at end of line, nothing to yank");
            return;
        }

        // Get text from cursor to end of line
        let yanked: String = chars[col_idx..].iter().collect();

        // Copy to system clipboard (no trailing newline - this is a character yank)
        godot::classes::DisplayServer::singleton().clipboard_set(&yanked);

        crate::verbose_print!(
            "[godot-neovim] Y: Yanked {} chars from col {} to EOL",
            yanked.len(),
            col_idx
        );
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
        crate::verbose_print!("[godot-neovim] gf: Queueing file open for '{}'", path);

        // Queue the file path for deferred opening in process()
        // cmd_edit() triggers editor_script_changed signal synchronously
        self.pending_file_path = Some(path);
    }

    /// Open URL or path under cursor in browser (gx command)
    pub(super) fn open_url_under_cursor(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        if col_idx >= chars.len() {
            crate::verbose_print!("[godot-neovim] gx: Cursor at end of line");
            return;
        }

        // Find start and end of URL-like text
        // Valid URL characters: alphanumeric, /:.-_~?#[]@!$&'()*+,;=%
        let url_chars = |c: char| {
            c.is_alphanumeric()
                || "/:.-_~?#[]@!$&'()*+,;=%".contains(c)
        };

        let mut start = col_idx;
        while start > 0 && url_chars(chars[start - 1]) {
            start -= 1;
        }

        let mut end = col_idx;
        while end < chars.len() && url_chars(chars[end]) {
            end += 1;
        }

        if start == end {
            crate::verbose_print!("[godot-neovim] gx: No URL under cursor");
            return;
        }

        let url: String = chars[start..end].iter().collect();

        // Check if it looks like a URL
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://") {
            crate::verbose_print!("[godot-neovim] gx: Opening URL: {}", url);
            let _ = Os::singleton().shell_open(&url);
        } else if url.contains("://") {
            // Other URL schemes
            crate::verbose_print!("[godot-neovim] gx: Opening URI: {}", url);
            let _ = Os::singleton().shell_open(&url);
        } else if url.contains('.') && !url.starts_with('.') {
            // Likely a domain name - add https://
            let full_url = format!("https://{}", url);
            crate::verbose_print!("[godot-neovim] gx: Opening as URL: {}", full_url);
            let _ = Os::singleton().shell_open(&full_url);
        } else {
            crate::verbose_print!("[godot-neovim] gx: Not a valid URL: {}", url);
        }
    }

    /// Fold current line (zc command)
    pub(super) fn fold_current_line(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        if editor.can_fold_line(line_idx) {
            editor.fold_line(line_idx);
            crate::verbose_print!("[godot-neovim] zc: Folded line {}", line_idx + 1);
        } else {
            crate::verbose_print!("[godot-neovim] zc: Cannot fold line {}", line_idx + 1);
        }
    }

    /// Unfold current line (zo command)
    pub(super) fn unfold_current_line(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        if editor.is_line_folded(line_idx) {
            editor.unfold_line(line_idx);
            crate::verbose_print!("[godot-neovim] zo: Unfolded line {}", line_idx + 1);
        } else {
            crate::verbose_print!("[godot-neovim] zo: Line {} not folded", line_idx + 1);
        }
    }

    /// Toggle fold at current line (za command)
    pub(super) fn toggle_fold(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        editor.toggle_foldable_line(line_idx);
        crate::verbose_print!("[godot-neovim] za: Toggled fold at line {}", line_idx + 1);
    }

    /// Fold all lines (zM command)
    pub(super) fn fold_all(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        editor.fold_all_lines();
        crate::verbose_print!("[godot-neovim] zM: Folded all lines");
    }

    /// Unfold all lines (zR command)
    pub(super) fn unfold_all(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        editor.unfold_all_lines();
        crate::verbose_print!("[godot-neovim] zR: Unfolded all lines");
    }

    /// Paste with indent adjustment ([p command - paste before with indent)
    pub(super) fn paste_with_indent_before(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let content = godot::classes::DisplayServer::singleton()
            .clipboard_get()
            .to_string();
        if content.is_empty() {
            return;
        }

        // Get current line's indentation
        let line_idx = editor.get_caret_line();
        let current_line = editor.get_line(line_idx).to_string();
        let current_indent: String = current_line
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();

        // Adjust pasted content's indentation
        let adjusted = Self::adjust_paste_indent(&content, &current_indent);

        // Paste above current line
        let line_count = editor.get_line_count();
        let paste_lines: Vec<&str> = adjusted.trim_end_matches('\n').lines().collect();

        let mut lines: Vec<String> = Vec::new();
        for i in 0..line_count {
            if i == line_idx {
                for paste_line in &paste_lines {
                    lines.push(paste_line.to_string());
                }
            }
            lines.push(editor.get_line(i).to_string());
        }
        editor.set_text(&lines.join("\n"));

        // Move cursor to first pasted line
        editor.set_caret_line(line_idx);
        let first_non_blank = paste_lines
            .first()
            .map(|l| l.chars().position(|c| !c.is_whitespace()).unwrap_or(0))
            .unwrap_or(0);
        editor.set_caret_column(first_non_blank as i32);

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] [p: Pasted with indent before");
    }

    /// Paste with indent adjustment (]p command - paste after with indent)
    pub(super) fn paste_with_indent_after(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let content = godot::classes::DisplayServer::singleton()
            .clipboard_get()
            .to_string();
        if content.is_empty() {
            return;
        }

        // Get current line's indentation
        let line_idx = editor.get_caret_line();
        let current_line = editor.get_line(line_idx).to_string();
        let current_indent: String = current_line
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();

        // Adjust pasted content's indentation
        let adjusted = Self::adjust_paste_indent(&content, &current_indent);

        // Paste below current line
        let line_count = editor.get_line_count();
        let paste_lines: Vec<&str> = adjusted.trim_end_matches('\n').lines().collect();

        let mut lines: Vec<String> = Vec::new();
        for i in 0..line_count {
            lines.push(editor.get_line(i).to_string());
            if i == line_idx {
                for paste_line in &paste_lines {
                    lines.push(paste_line.to_string());
                }
            }
        }
        editor.set_text(&lines.join("\n"));

        // Move cursor to first pasted line
        editor.set_caret_line(line_idx + 1);
        let first_non_blank = paste_lines
            .first()
            .map(|l| l.chars().position(|c| !c.is_whitespace()).unwrap_or(0))
            .unwrap_or(0);
        editor.set_caret_column(first_non_blank as i32);

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] ]p: Pasted with indent after");
    }

    /// Format current line (gqq command) - wrap long lines
    pub(super) fn format_current_line(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();

        // Get indent
        let indent: String = line_text.chars().take_while(|c| c.is_whitespace()).collect();
        let content = line_text.trim_start();

        // Wrap at 80 characters (configurable later)
        let wrap_width = 80;
        let effective_width = wrap_width - indent.len();

        if content.len() <= effective_width {
            crate::verbose_print!("[godot-neovim] gqq: Line {} already short enough", line_idx + 1);
            return;
        }

        // Split into words and wrap
        let words: Vec<&str> = content.split_whitespace().collect();
        let mut lines: Vec<String> = Vec::new();
        let mut current_line = indent.clone();

        for word in words {
            let test_line = if current_line.trim().is_empty() {
                format!("{}{}", current_line, word)
            } else {
                format!("{} {}", current_line, word)
            };

            if test_line.len() > wrap_width && !current_line.trim().is_empty() {
                lines.push(current_line);
                current_line = format!("{}{}", indent, word);
            } else {
                current_line = test_line;
            }
        }

        if !current_line.trim().is_empty() {
            lines.push(current_line);
        }

        if lines.len() <= 1 {
            crate::verbose_print!("[godot-neovim] gqq: No wrapping needed");
            return;
        }

        // Replace current line with wrapped lines
        let full_text = editor.get_text().to_string();
        let all_lines: Vec<&str> = full_text.lines().collect();
        let mut new_lines: Vec<String> = Vec::new();

        for (i, line) in all_lines.iter().enumerate() {
            if i as i32 == line_idx {
                new_lines.extend(lines.clone());
            } else {
                new_lines.push(line.to_string());
            }
        }

        editor.set_text(&new_lines.join("\n"));
        editor.set_caret_line(line_idx);

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] gqq: Wrapped line {} into {} lines",
            line_idx + 1,
            lines.len()
        );
    }

    /// Adjust paste content's indentation to match target indent
    fn adjust_paste_indent(content: &str, target_indent: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return content.to_string();
        }

        // Find the minimum indentation in the pasted content
        let min_indent = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.chars().take_while(|c| c.is_whitespace()).count())
            .min()
            .unwrap_or(0);

        // Adjust each line
        let adjusted: Vec<String> = lines
            .iter()
            .map(|line| {
                if line.trim().is_empty() {
                    String::new()
                } else {
                    let stripped: String = line.chars().skip(min_indent).collect();
                    format!("{}{}", target_indent, stripped)
                }
            })
            .collect();

        if content.ends_with('\n') {
            adjusted.join("\n") + "\n"
        } else {
            adjusted.join("\n")
        }
    }
}
