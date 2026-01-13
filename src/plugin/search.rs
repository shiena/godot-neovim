//! Search operations: word search, character find, find dialog

use super::GodotNeovimPlugin;
use godot::classes::Input;
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Open Godot's find dialog (/ command)
    pub(super) fn open_find_dialog(&self) {
        // Simulate Ctrl+F to open find dialog
        let mut key_event = godot::classes::InputEventKey::new_gd();
        key_event.set_keycode(Key::F);
        key_event.set_ctrl_pressed(true);
        key_event.set_pressed(true);
        Input::singleton().parse_input_event(&key_event);

        crate::verbose_print!("[godot-neovim] /: Opened find dialog");
    }

    /// Get the word under cursor from the Godot editor
    pub(super) fn get_word_under_cursor(&self) -> Option<String> {
        let editor = self.current_editor.as_ref()?;

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();

        if line_text.is_empty() || col_idx >= line_text.chars().count() {
            return None;
        }

        let chars: Vec<char> = line_text.chars().collect();

        // Check if cursor is on a word character
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

        if !is_word_char(chars[col_idx]) {
            return None;
        }

        // Find word start
        let mut start = col_idx;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }

        // Find word end
        let mut end = col_idx;
        while end < chars.len() && is_word_char(chars[end]) {
            end += 1;
        }

        let word: String = chars[start..end].iter().collect();
        if word.is_empty() {
            None
        } else {
            Some(word)
        }
    }

    /// Search forward for word under cursor (* command)
    pub(super) fn search_word_forward(&mut self) {
        // Add to jump list before searching
        self.add_to_jump_list();

        let Some(word) = self.get_word_under_cursor() else {
            crate::verbose_print!("[godot-neovim] *: No word under cursor");
            return;
        };

        // Save for n/N repeat
        self.last_search_word = word.clone();
        self.last_search_forward = true;

        crate::verbose_print!("[godot-neovim] *: Searching forward for '{}'", word);

        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let current_col = editor.get_caret_column();
        let line_count = editor.get_line_count();

        // Search from current position forward
        for line_idx in current_line..line_count {
            let line_text = editor.get_line(line_idx).to_string();
            let search_start = if line_idx == current_line {
                // On current line, search after current word
                (current_col as usize) + 1
            } else {
                0
            };

            if let Some(found) = self.find_word_in_line(&line_text, &word, search_start, true) {
                self.move_cursor_to(line_idx, found as i32);
                return;
            }
        }

        // Wrap around to beginning of file
        for line_idx in 0..=current_line {
            let line_text = editor.get_line(line_idx).to_string();
            let search_end = if line_idx == current_line {
                current_col as usize
            } else {
                line_text.len()
            };

            if let Some(found) = self.find_word_in_line(&line_text, &word, 0, true) {
                if line_idx < current_line || found < search_end {
                    self.move_cursor_to(line_idx, found as i32);
                    return;
                }
            }
        }

        crate::verbose_print!("[godot-neovim] *: No more matches for '{}'", word);
    }

    /// Search backward for word under cursor (# command)
    pub(super) fn search_word_backward(&mut self) {
        // Add to jump list before searching
        self.add_to_jump_list();

        let Some(word) = self.get_word_under_cursor() else {
            crate::verbose_print!("[godot-neovim] #: No word under cursor");
            return;
        };

        // Save for n/N repeat
        self.last_search_word = word.clone();
        self.last_search_forward = false;

        crate::verbose_print!("[godot-neovim] #: Searching backward for '{}'", word);

        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let current_col = editor.get_caret_column() as usize;
        let line_count = editor.get_line_count();

        // Search from current position backward
        for line_idx in (0..=current_line).rev() {
            let line_text = editor.get_line(line_idx).to_string();

            if let Some(found) = self.find_word_in_line_backward(
                &line_text,
                &word,
                current_line,
                line_idx,
                current_col,
            ) {
                self.move_cursor_to(line_idx, found as i32);
                return;
            }
        }

        // Wrap around to end of file
        for line_idx in (current_line..line_count).rev() {
            let line_text = editor.get_line(line_idx).to_string();

            if let Some(found) = self.find_word_in_line(&line_text, &word, 0, false) {
                // Find last occurrence
                let mut last = found;
                let mut search_from = found + 1;
                while let Some(next) = self.find_word_in_line(&line_text, &word, search_from, true)
                {
                    if line_idx == current_line && next >= current_col {
                        break;
                    }
                    last = next;
                    search_from = next + 1;
                }
                self.move_cursor_to(line_idx, last as i32);
                return;
            }
        }

        crate::verbose_print!("[godot-neovim] #: No more matches for '{}'", word);
    }

    /// Repeat search in given direction (n/N commands)
    pub(super) fn repeat_search(&mut self, same_direction: bool) {
        if self.last_search_word.is_empty() {
            crate::verbose_print!("[godot-neovim] n/N: No previous search");
            return;
        }

        let forward = if same_direction {
            self.last_search_forward
        } else {
            !self.last_search_forward
        };

        crate::verbose_print!(
            "[godot-neovim] {}: Repeating search for '{}' {}",
            if same_direction { "n" } else { "N" },
            self.last_search_word,
            if forward { "forward" } else { "backward" }
        );

        let word = self.last_search_word.clone();
        if forward {
            self.search_word_forward_internal(&word);
        } else {
            self.search_word_backward_internal(&word);
        }
    }

    /// Internal search forward (used by * and n)
    pub(super) fn search_word_forward_internal(&mut self, word: &str) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let current_col = editor.get_caret_column();
        let line_count = editor.get_line_count();

        // Search from current position forward
        for line_idx in current_line..line_count {
            let line_text = editor.get_line(line_idx).to_string();
            let search_start = if line_idx == current_line {
                (current_col as usize) + 1
            } else {
                0
            };

            if let Some(found) = self.find_word_in_line(&line_text, word, search_start, true) {
                self.move_cursor_to(line_idx, found as i32);
                return;
            }
        }

        // Wrap around to beginning of file
        for line_idx in 0..=current_line {
            let line_text = editor.get_line(line_idx).to_string();
            let search_end = if line_idx == current_line {
                current_col as usize
            } else {
                line_text.len()
            };

            if let Some(found) = self.find_word_in_line(&line_text, word, 0, true) {
                if line_idx < current_line || found < search_end {
                    self.move_cursor_to(line_idx, found as i32);
                    return;
                }
            }
        }
    }

    /// Internal search backward (used by # and N)
    pub(super) fn search_word_backward_internal(&mut self, word: &str) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let current_col = editor.get_caret_column() as usize;
        let line_count = editor.get_line_count();

        // Search from current position backward
        for line_idx in (0..=current_line).rev() {
            let line_text = editor.get_line(line_idx).to_string();

            if let Some(found) = self.find_word_in_line_backward(
                &line_text,
                word,
                current_line,
                line_idx,
                current_col,
            ) {
                self.move_cursor_to(line_idx, found as i32);
                return;
            }
        }

        // Wrap around to end of file
        for line_idx in (current_line..line_count).rev() {
            let line_text = editor.get_line(line_idx).to_string();

            if let Some(found) = self.find_word_in_line(&line_text, word, 0, false) {
                let mut last = found;
                let mut search_from = found + 1;
                while let Some(next) = self.find_word_in_line(&line_text, word, search_from, true) {
                    if line_idx == current_line && next >= current_col {
                        break;
                    }
                    last = next;
                    search_from = next + 1;
                }
                self.move_cursor_to(line_idx, last as i32);
                return;
            }
        }
    }

    /// Find word boundary match in line starting from given position
    pub(super) fn find_word_in_line(
        &self,
        line: &str,
        word: &str,
        start: usize,
        forward: bool,
    ) -> Option<usize> {
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        let chars: Vec<char> = line.chars().collect();
        let word_chars: Vec<char> = word.chars().collect();
        let word_len = word_chars.len();

        if word_len == 0 || chars.len() < word_len {
            return None;
        }

        let search_range: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(start..=chars.len().saturating_sub(word_len))
        } else {
            Box::new((0..=chars.len().saturating_sub(word_len)).rev())
        };

        for i in search_range {
            // Check if the substring matches
            let mut matches = true;
            for (j, wc) in word_chars.iter().enumerate() {
                if chars[i + j] != *wc {
                    matches = false;
                    break;
                }
            }

            if !matches {
                continue;
            }

            // Check word boundaries
            let before_ok = i == 0 || !is_word_char(chars[i - 1]);
            let after_ok = i + word_len >= chars.len() || !is_word_char(chars[i + word_len]);

            if before_ok && after_ok {
                return Some(i);
            }
        }

        None
    }

    /// Find word in line for backward search, handling current line specially
    pub(super) fn find_word_in_line_backward(
        &self,
        line: &str,
        word: &str,
        current_line: i32,
        line_idx: i32,
        current_col: usize,
    ) -> Option<usize> {
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        let chars: Vec<char> = line.chars().collect();
        let word_chars: Vec<char> = word.chars().collect();
        let word_len = word_chars.len();

        if word_len == 0 || chars.len() < word_len {
            return None;
        }

        // Determine the end position for search
        let end_pos = if line_idx == current_line {
            current_col.saturating_sub(1)
        } else {
            chars.len().saturating_sub(word_len)
        };

        // Search backward from end_pos
        for i in (0..=end_pos.min(chars.len().saturating_sub(word_len))).rev() {
            // Check if the substring matches
            let mut matches = true;
            for (j, wc) in word_chars.iter().enumerate() {
                if chars[i + j] != *wc {
                    matches = false;
                    break;
                }
            }

            if !matches {
                continue;
            }

            // Check word boundaries
            let before_ok = i == 0 || !is_word_char(chars[i - 1]);
            let after_ok = i + word_len >= chars.len() || !is_word_char(chars[i + word_len]);

            if before_ok && after_ok {
                return Some(i);
            }
        }

        None
    }

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
}
