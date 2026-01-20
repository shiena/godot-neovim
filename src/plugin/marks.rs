//! Marks and jump list functionality

use super::GodotNeovimPlugin;

impl GodotNeovimPlugin {
    /// Set a mark at current position (m{a-z})
    pub(super) fn set_mark(&mut self, mark: char) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line = editor.get_caret_line();
        let col = editor.get_caret_column();
        self.marks.insert(mark, (line, col));
        crate::verbose_print!(
            "[godot-neovim] m{}: Set mark at line {}, col {}",
            mark,
            line + 1,
            col
        );
    }

    /// Jump to mark line ('{a-z})
    pub(super) fn jump_to_mark_line(&mut self, mark: char) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        let Some((line, _)) = self.marks.get(&mark).copied() else {
            crate::verbose_print!("[godot-neovim] '{}: Mark not set", mark);
            return;
        };

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_count = editor.get_line_count();
        let target_line = line.min(line_count - 1);
        editor.set_caret_line(target_line);

        // Move to first non-blank character (Vim behavior for ')
        let line_text = editor.get_line(target_line).to_string();
        let first_non_blank = line_text
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);
        editor.set_caret_column(first_non_blank as i32);

        self.sync_cursor_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] '{}: Jumped to line {}",
            mark,
            target_line + 1
        );
    }

    /// Jump to exact mark position (`{a-z})
    pub(super) fn jump_to_mark_position(&mut self, mark: char) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        let Some((line, col)) = self.marks.get(&mark).copied() else {
            crate::verbose_print!("[godot-neovim] `{}: Mark not set", mark);
            return;
        };

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_count = editor.get_line_count();
        let target_line = line.min(line_count - 1);
        editor.set_caret_line(target_line);

        let line_length = editor.get_line(target_line).len() as i32;
        let target_col = col.min(line_length.max(0));
        editor.set_caret_column(target_col);

        self.sync_cursor_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] `{}: Jumped to line {}, col {}",
            mark,
            target_line + 1,
            target_col
        );
    }

    /// Add current position to jump list
    pub(super) fn add_to_jump_list(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line = editor.get_caret_line();
        let col = editor.get_caret_column();

        // Don't add duplicate consecutive entries
        if let Some(&last) = self.jump_list.last() {
            if last == (line, col) {
                return;
            }
        }

        // If we're not at the end of the list, truncate
        if self.jump_list_pos < self.jump_list.len() {
            self.jump_list.truncate(self.jump_list_pos);
        }

        self.jump_list.push((line, col));
        self.jump_list_pos = self.jump_list.len();

        // Limit jump list size
        const MAX_JUMP_LIST: usize = 100;
        if self.jump_list.len() > MAX_JUMP_LIST {
            self.jump_list.remove(0);
            self.jump_list_pos = self.jump_list.len();
        }
    }
}
