//! Named registers for yank, delete, and paste

use super::{CodeEditExt, GodotNeovimPlugin};
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Check if register is a system clipboard register (+ or *)
    fn is_clipboard_register(register: char) -> bool {
        register == '+' || register == '*'
    }

    /// Check if register is the black hole register (_)
    fn is_blackhole_register(register: char) -> bool {
        register == '_'
    }

    /// Check if register is the yank register (0)
    fn is_yank_register(register: char) -> bool {
        register == '0'
    }

    /// Store content to register (handles special registers)
    fn store_to_register(&mut self, register: char, content: &str, is_yank: bool) {
        if Self::is_blackhole_register(register) {
            // Black hole register - discard
            crate::verbose_print!("[godot-neovim] \"_: Discarded to black hole");
            return;
        }

        if Self::is_clipboard_register(register) {
            // System clipboard
            godot::classes::DisplayServer::singleton().clipboard_set(content);
            crate::verbose_print!("[godot-neovim] \"{}: Stored to system clipboard", register);
        } else {
            // Named register
            self.registers.insert(register, content.to_string());
        }

        // Also store to yank register (0) if this is a yank operation
        if is_yank && !Self::is_yank_register(register) {
            self.registers.insert('0', content.to_string());
        }
    }

    /// Get content from register (handles special registers)
    fn get_from_register(&self, register: char) -> Option<String> {
        if Self::is_blackhole_register(register) {
            // Black hole register - always empty
            return None;
        }

        if Self::is_clipboard_register(register) {
            // System clipboard
            let content = godot::classes::DisplayServer::singleton()
                .clipboard_get()
                .to_string();
            if content.is_empty() {
                None
            } else {
                Some(content)
            }
        } else {
            // Named register (including 0)
            self.registers.get(&register).cloned()
        }
    }

    /// Yank current line to named register
    #[allow(dead_code)]
    pub(super) fn yank_line_to_register(&mut self, register: char) {
        self.yank_lines_to_register(register, 1);
    }

    /// Yank multiple lines to named register
    pub(super) fn yank_lines_to_register(&mut self, register: char, count: i32) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_count = editor.get_line_count();
        let end_line = (line_idx + count).min(line_count);

        // Collect lines
        let mut lines: Vec<String> = Vec::new();
        for i in line_idx..end_line {
            lines.push(editor.get_line(i).to_string());
        }

        // Store with newline (line yank)
        let content = lines.join("\n") + "\n";
        self.store_to_register(register, &content, true);
        crate::verbose_print!(
            "[godot-neovim] \"{}: Yanked {} line(s) from line {} to register",
            register,
            count,
            line_idx + 1
        );
    }

    /// Delete current line and store in named register
    #[allow(dead_code)]
    pub(super) fn delete_line_to_register(&mut self, register: char) {
        self.delete_lines_to_register(register, 1);
    }

    /// Delete multiple lines and store in named register
    pub(super) fn delete_lines_to_register(&mut self, register: char, count: i32) {
        // First phase: collect data with editor borrowed
        let (line_idx, actual_count, content, new_text_opt) = {
            let Some(ref editor) = self.current_editor else {
                return;
            };

            let line_idx = editor.get_caret_line();
            let line_count = editor.get_line_count();
            let end_line = (line_idx + count).min(line_count);
            let actual_count = end_line - line_idx;

            // Collect lines to delete
            let mut deleted_lines: Vec<String> = Vec::new();
            for i in line_idx..end_line {
                deleted_lines.push(editor.get_line(i).to_string());
            }

            // Store with newline (line delete)
            let content = deleted_lines.join("\n") + "\n";

            // Prepare new text if not deleting all lines
            let new_text_opt = if actual_count < line_count {
                let mut lines: Vec<String> = Vec::new();
                for i in 0..line_count {
                    if i < line_idx || i >= end_line {
                        lines.push(editor.get_line(i).to_string());
                    }
                }
                Some(lines.join("\n"))
            } else {
                None
            };

            (line_idx, actual_count, content, new_text_opt)
        };

        // Store to register (editor borrow released)
        self.store_to_register(register, &content, false);

        // Second phase: update editor
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        if let Some(new_text) = new_text_opt {
            editor.set_text_and_notify(&new_text);

            // Adjust cursor position
            let new_line_count = editor.get_line_count();
            let target_line = line_idx.min(new_line_count - 1);
            editor.set_caret_line(target_line);

            // Move to first non-blank
            let target_text = editor.get_line(target_line).to_string();
            let first_non_blank = target_text
                .chars()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0);
            editor.set_caret_column(first_non_blank as i32);
        } else {
            // All lines deleted - just clear
            editor.set_text_and_notify("");
            editor.set_caret_line(0);
            editor.set_caret_column(0);
        }

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] \"{}: Deleted {} line(s) from line {} to register",
            register,
            actual_count,
            line_idx + 1
        );
    }

    /// Paste from named register (after cursor/below line)
    pub(super) fn paste_from_register(&mut self, register: char) {
        let Some(content) = self.get_from_register(register) else {
            crate::verbose_print!("[godot-neovim] \"{}: Register empty", register);
            return;
        };

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Check if it's a line paste (ends with newline)
        if content.ends_with('\n') {
            // Line paste - insert below current line
            let line_idx = editor.get_caret_line();
            let line_count = editor.get_line_count();
            let paste_content = content.trim_end_matches('\n');

            // Build new text with inserted line
            let mut lines: Vec<String> = Vec::new();
            for i in 0..line_count {
                lines.push(editor.get_line(i).to_string());
                if i == line_idx {
                    lines.push(paste_content.to_string());
                }
            }
            editor.set_text_and_notify(&lines.join("\n"));

            // Move cursor to the pasted line
            editor.set_caret_line(line_idx + 1);
            let first_non_blank = paste_content
                .chars()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0);
            editor.set_caret_column(first_non_blank as i32);
        } else {
            // Character paste - insert after cursor
            let line_idx = editor.get_caret_line();
            let col_idx = editor.get_caret_column();
            let line_text = editor.get_line(line_idx).to_string();

            let mut chars: Vec<char> = line_text.chars().collect();
            let insert_pos = ((col_idx + 1) as usize).min(chars.len());
            for (i, c) in content.chars().enumerate() {
                chars.insert(insert_pos + i, c);
            }
            let new_line: String = chars.into_iter().collect();
            editor.set_line(line_idx, &new_line);

            // Move cursor to end of pasted content
            editor.set_caret_column(insert_pos as i32 + content.len() as i32 - 1);
        }

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] \"{}p: Pasted from register", register);
    }

    /// Paste from named register (before cursor/above line)
    pub(super) fn paste_from_register_before(&mut self, register: char) {
        let Some(content) = self.get_from_register(register) else {
            crate::verbose_print!("[godot-neovim] \"{}: Register empty", register);
            return;
        };

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Check if it's a line paste (ends with newline)
        if content.ends_with('\n') {
            // Line paste - insert above current line
            let line_idx = editor.get_caret_line();
            let line_count = editor.get_line_count();
            let paste_content = content.trim_end_matches('\n');

            // Build new text with inserted line
            let mut lines: Vec<String> = Vec::new();
            for i in 0..line_count {
                if i == line_idx {
                    lines.push(paste_content.to_string());
                }
                lines.push(editor.get_line(i).to_string());
            }
            editor.set_text_and_notify(&lines.join("\n"));

            // Move cursor to the pasted line
            editor.set_caret_line(line_idx);
            let first_non_blank = paste_content
                .chars()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0);
            editor.set_caret_column(first_non_blank as i32);
        } else {
            // Character paste - insert before cursor
            let line_idx = editor.get_caret_line();
            let col_idx = editor.get_caret_column();
            let line_text = editor.get_line(line_idx).to_string();

            let mut chars: Vec<char> = line_text.chars().collect();
            let insert_pos = (col_idx as usize).min(chars.len());
            for (i, c) in content.chars().enumerate() {
                chars.insert(insert_pos + i, c);
            }
            let new_line: String = chars.into_iter().collect();
            editor.set_line(line_idx, &new_line);

            // Move cursor to end of pasted content
            editor.set_caret_column(insert_pos as i32 + content.len() as i32 - 1);
        }

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] \"{}P: Pasted from register (before)",
            register
        );
    }

    /// Paste from clipboard and move cursor after pasted text (gp command)
    pub(super) fn paste_and_move_after(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let content = godot::classes::DisplayServer::singleton()
            .clipboard_get()
            .to_string();
        if content.is_empty() {
            return;
        }

        // Check if it's a line paste (ends with newline)
        if content.ends_with('\n') {
            let line_idx = editor.get_caret_line();
            let line_count = editor.get_line_count();
            let paste_lines: Vec<&str> = content.trim_end_matches('\n').lines().collect();

            // Build new text with inserted lines
            let mut lines: Vec<String> = Vec::new();
            for i in 0..line_count {
                lines.push(editor.get_line(i).to_string());
                if i == line_idx {
                    for paste_line in &paste_lines {
                        lines.push(paste_line.to_string());
                    }
                }
            }
            editor.set_text_and_notify(&lines.join("\n"));

            // Move cursor to line after pasted content
            let target_line = line_idx + paste_lines.len() as i32 + 1;
            let max_line = editor.get_line_count() - 1;
            editor.set_caret_line(target_line.min(max_line));
            editor.set_caret_column(0);
        } else {
            // Character paste - insert after cursor and move to end
            let line_idx = editor.get_caret_line();
            let col_idx = editor.get_caret_column();
            let line_text = editor.get_line(line_idx).to_string();

            let mut chars: Vec<char> = line_text.chars().collect();
            let insert_pos = ((col_idx + 1) as usize).min(chars.len());
            for (i, c) in content.chars().enumerate() {
                chars.insert(insert_pos + i, c);
            }
            let new_line: String = chars.into_iter().collect();
            editor.set_line(line_idx, &new_line);

            // Move cursor after pasted content
            editor.set_caret_column(insert_pos as i32 + content.len() as i32);
        }

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] gp: Pasted and moved after");
    }

    /// Paste from clipboard before and move cursor after pasted text (gP command)
    pub(super) fn paste_before_and_move_after(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let content = godot::classes::DisplayServer::singleton()
            .clipboard_get()
            .to_string();
        if content.is_empty() {
            return;
        }

        // Check if it's a line paste (ends with newline)
        if content.ends_with('\n') {
            let line_idx = editor.get_caret_line();
            let line_count = editor.get_line_count();
            let paste_lines: Vec<&str> = content.trim_end_matches('\n').lines().collect();

            // Build new text with inserted lines before current
            let mut lines: Vec<String> = Vec::new();
            for i in 0..line_count {
                if i == line_idx {
                    for paste_line in &paste_lines {
                        lines.push(paste_line.to_string());
                    }
                }
                lines.push(editor.get_line(i).to_string());
            }
            editor.set_text_and_notify(&lines.join("\n"));

            // Move cursor to line after pasted content (which is original line position + paste count)
            let target_line = line_idx + paste_lines.len() as i32;
            editor.set_caret_line(target_line);
            editor.set_caret_column(0);
        } else {
            // Character paste - insert before cursor and move to end
            let line_idx = editor.get_caret_line();
            let col_idx = editor.get_caret_column();
            let line_text = editor.get_line(line_idx).to_string();

            let mut chars: Vec<char> = line_text.chars().collect();
            let insert_pos = (col_idx as usize).min(chars.len());
            for (i, c) in content.chars().enumerate() {
                chars.insert(insert_pos + i, c);
            }
            let new_line: String = chars.into_iter().collect();
            editor.set_line(line_idx, &new_line);

            // Move cursor after pasted content
            editor.set_caret_column(insert_pos as i32 + content.len() as i32);
        }

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] gP: Pasted before and moved after");
    }
}
