//! Editing operations: delete, replace, indent, join
//!
//! Note: Undo (u) and Redo (Ctrl+R) are handled by sending to Neovim directly
//! (Neovim Master design - see DESIGN_V2.md)

use super::{CodeEditExt, GodotNeovimPlugin};
use godot::classes::{EditorInterface, Os};
use godot::prelude::*;

impl GodotNeovimPlugin {
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
            self.clear_last_key();
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
        editor.set_text_and_notify(&new_text);

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
        editor.set_text_and_notify(&new_text);

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

    /// Go to definition using LSP (gd command)
    ///
    /// Note: This uses Godot's built-in LSP (port 6005) instead of Neovim LSP.
    /// Rationale: Similar to vscode-neovim which uses IDE's LSP.
    /// - neovim_clean=true by default, so user's Neovim LSP config is not loaded
    /// - Godot LSP is always available without additional user configuration
    /// - File jumping across files is handled by Godot's editor
    pub(super) fn go_to_definition_lsp(&mut self) {
        use godot::classes::ProjectSettings;

        let Some(ref lsp) = self.godot_lsp else {
            self.show_status_message("gd: Enable 'Use Thread' in Editor Settings");
            return;
        };

        let Some(ref editor) = self.current_editor else {
            return;
        };

        // Get current position and buffer content
        let line = editor.get_caret_line() as u32;
        let col = editor.get_caret_column() as u32;
        let text = editor.get_text().to_string();

        // Get absolute file path and convert to URI
        let abs_path = if self.current_script_path.starts_with("res://") {
            ProjectSettings::singleton()
                .globalize_path(&self.current_script_path)
                .to_string()
        } else {
            self.current_script_path.clone()
        };

        // Convert to file:// URI (handle Windows paths)
        let uri = if abs_path.starts_with('/') {
            format!("file://{}", abs_path)
        } else {
            // Windows path: C:/... -> file:///C:/...
            format!("file:///{}", abs_path.replace('\\', "/"))
        };

        // Get project root for LSP initialization
        let project_root = ProjectSettings::singleton()
            .globalize_path("res://")
            .to_string();
        let root_uri = if project_root.starts_with('/') {
            format!("file://{}", project_root)
        } else {
            format!("file:///{}", project_root.replace('\\', "/"))
        };

        crate::verbose_print!(
            "[godot-neovim] gd: Requesting definition at {}:{}:{}",
            uri,
            line,
            col
        );

        // Ensure connected
        if !lsp.is_connected() {
            if let Err(e) = lsp.connect(6005) {
                self.show_status_message(&format!("LSP connect failed: {}", e));
                return;
            }
            crate::verbose_print!("[godot-neovim] gd: Connected to LSP");
        }

        // Ensure initialized
        if !lsp.is_initialized() {
            if let Err(e) = lsp.initialize(&root_uri) {
                self.show_status_message(&format!("LSP init failed: {}", e));
                return;
            }
            crate::verbose_print!("[godot-neovim] gd: LSP initialized");
        }

        // Send didOpen to ensure LSP knows about the file
        if let Err(e) = lsp.did_open(&uri, &text) {
            crate::verbose_print!("[godot-neovim] gd: didOpen warning: {}", e);
            // Continue anyway - file might already be open
        }

        // Request definition
        let result = lsp.goto_definition(&uri, line, col);

        match result {
            Ok(Some(location)) => {
                // Convert URI back to file path (handles URL decoding and Windows paths)
                let path = Self::uri_to_file_path(location.uri.as_str());

                // Normalize path separators for comparison (Windows uses backslash, Godot uses forward slash)
                let path_normalized = path.replace('\\', "/");

                let target_line = location.range.start.line as i64 + 1; // 1-indexed
                let target_col = location.range.start.character as i64;

                crate::verbose_print!(
                    "[godot-neovim] gd: LSP returned {}:{}:{}",
                    path_normalized,
                    target_line,
                    target_col
                );

                // Check if same file or different file
                if path_normalized == self.current_script_path || path_normalized == abs_path {
                    // Same file - just move cursor
                    if let Some(ref mut editor) = self.current_editor {
                        let target_line_i32 = (target_line - 1).max(0) as i32;
                        let target_col_i32 = target_col.max(0) as i32;
                        editor.set_caret_line(target_line_i32);
                        editor.set_caret_column(target_col_i32);
                        self.sync_cursor_to_neovim();
                        crate::verbose_print!(
                            "[godot-neovim] gd: Jumped to line {}, col {}",
                            target_line,
                            target_col
                        );
                    }
                } else {
                    // Different file - open it and jump to position
                    let res_path = ProjectSettings::singleton()
                        .localize_path(&path_normalized)
                        .to_string();
                    let res_path = if res_path.starts_with("res://") {
                        res_path
                    } else {
                        path_normalized.clone()
                    };

                    crate::verbose_print!(
                        "[godot-neovim] gd: Opening different file: {}",
                        res_path
                    );

                    // Queue file open with position
                    self.pending_file_path = Some(res_path);
                    // TODO: Also store line/col for after file opens
                }
            }
            Ok(None) => {
                crate::verbose_print!("[godot-neovim] gd: No definition found");
                self.show_status_message("Definition not found");
            }
            Err(e) => {
                crate::verbose_print!("[godot-neovim] gd: LSP error: {}", e);
                self.show_status_message(&format!("LSP error: {}", e));
            }
        }
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
        let file_name =
            if let Some(mut script_edit) = EditorInterface::singleton().get_script_editor() {
                if let Some(current_script) = script_edit.get_current_script() {
                    let path = current_script.get_path().to_string();
                    if path.is_empty() {
                        "[New File]".to_string()
                    } else {
                        // Extract just the filename from path
                        path.split('/').next_back().unwrap_or(&path).to_string()
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
        label
            .add_theme_color_override("font_color", godot::prelude::Color::from_rgb(1.0, 1.0, 1.0));
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
        self.clear_last_key();
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
            self.clear_last_key();
            crate::verbose_print!(
                "[godot-neovim] gi: No previous insert position, entering insert mode"
            );
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
        self.clear_last_key();
        crate::verbose_print!(
            "[godot-neovim] gi: Insert at last position ({}, {})",
            target_line + 1,
            target_col
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
        let url_chars = |c: char| c.is_alphanumeric() || "/:.-_~?#[]@!$&'()*+,;=%".contains(c);

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
        editor.set_text_and_notify(&lines.join("\n"));

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
        editor.set_text_and_notify(&lines.join("\n"));

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
        let indent: String = line_text
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        let content = line_text.trim_start();

        // Wrap at 80 characters (configurable later)
        let wrap_width = 80;
        let effective_width = wrap_width - indent.len();

        if content.len() <= effective_width {
            crate::verbose_print!(
                "[godot-neovim] gqq: Line {} already short enough",
                line_idx + 1
            );
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

        editor.set_text_and_notify(&new_lines.join("\n"));
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

    /// Convert file:// URI to file path
    /// Handles URL decoding and platform differences:
    /// - Unix: file:///path -> /path
    /// - Windows: file:///C:/path -> C:/path
    fn uri_to_file_path(uri: &str) -> String {
        // First, URL decode the entire URI to handle %3A etc.
        let decoded_uri = Self::url_decode(uri);

        let path = if let Some(p) = decoded_uri.strip_prefix("file:///") {
            // Check if it's a Windows path with drive letter (e.g., C:, D:)
            if p.len() >= 2 && p.chars().nth(1) == Some(':') {
                // Windows path: file:///C:/path -> C:/path
                p.to_string()
            } else {
                // Unix path: file:///path -> /path (restore leading /)
                format!("/{}", p)
            }
        } else if let Some(p) = decoded_uri.strip_prefix("file://") {
            // file://path (less common, but handle it)
            p.to_string()
        } else {
            decoded_uri
        };

        // Final check: if path starts with /X:/ on Windows, remove the leading /
        if path.len() >= 3 {
            let chars: Vec<char> = path.chars().take(4).collect();
            if chars.len() >= 3 && chars[0] == '/' && chars[2] == ':' {
                return path[1..].to_string();
            }
        }

        path
    }

    /// Simple URL decoding for file paths
    fn url_decode(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let mut chars = input.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '%' {
                // Try to read two hex digits
                let hex: String = chars.by_ref().take(2).collect();
                if hex.len() == 2 {
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        result.push(byte as char);
                        continue;
                    }
                }
                // If decoding failed, keep original
                result.push('%');
                result.push_str(&hex);
            } else {
                result.push(c);
            }
        }

        result
    }
}
