//! Editing operations: LSP navigation, documentation, char info
//!
//! Note: Most editing commands (r, ~, >>, <<, etc.) are sent to Neovim
//! (Neovim Master design - see DESIGN_V2.md)

use super::GodotNeovimPlugin;
use godot::classes::{EditorInterface, Os};
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Enter replace mode (R command)
    pub(super) fn enter_replace_mode(&mut self) {
        // Send 'R' to Neovim to enter replace mode
        let completed = self.send_keys("R");
        if completed {
            self.clear_last_key();
        }
        crate::verbose_print!("[godot-neovim] R: Entered replace mode");
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

    /// Insert at column 0 (gI command) - Neovim Master design
    /// Send gI to Neovim and let it handle cursor positioning and mode change
    pub(super) fn insert_at_column_zero(&mut self) {
        // Send gI to Neovim - it will move cursor to column 0 and enter insert mode
        // Cursor position and mode change will be synced back via events
        self.send_keys("gI");
        self.clear_last_key();
        crate::verbose_print!("[godot-neovim] gI: Sent to Neovim");
    }

    /// Insert at last insert position (gi command) - Neovim Master design
    /// Send gi to Neovim and let it handle cursor positioning and mode change
    pub(super) fn insert_at_last_position(&mut self) {
        // Send gi to Neovim - it knows the last insert position
        // Cursor position and mode change will be synced back via events
        self.send_keys("gi");
        self.clear_last_key();
        crate::verbose_print!("[godot-neovim] gi: Sent to Neovim");
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
