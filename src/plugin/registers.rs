//! Named registers for yank, delete, and paste

use super::GodotNeovimPlugin;

impl GodotNeovimPlugin {
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
        self.registers.insert(register, content);
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
        let Some(ref mut editor) = self.current_editor else {
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
        self.registers.insert(register, content);

        // Delete the lines
        if actual_count < line_count {
            // Remove lines by setting text
            let mut lines: Vec<String> = Vec::new();
            for i in 0..line_count {
                if i < line_idx || i >= end_line {
                    lines.push(editor.get_line(i).to_string());
                }
            }
            editor.set_text(&lines.join("\n"));

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
            editor.set_text("");
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
        let Some(content) = self.registers.get(&register).cloned() else {
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
            editor.set_text(&lines.join("\n"));

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
        let Some(content) = self.registers.get(&register).cloned() else {
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
            editor.set_text(&lines.join("\n"));

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
}
