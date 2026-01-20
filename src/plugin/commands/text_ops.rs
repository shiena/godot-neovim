//! Text manipulation: :s, :g, :sort, :t, :m

use super::super::{CodeEditExt, GodotNeovimPlugin};
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Forward line range commands to Neovim (Neovim Master design)
    /// Examples: :1,5d, :.,$s/old/new/g, :'<,'>d
    pub(in crate::plugin) fn cmd_forward_to_neovim(&mut self, cmd: &str) {
        // Send command to Neovim - buffer changes will come back via nvim_buf_lines_event
        self.send_keys(&format!(":{}<CR>", cmd));

        crate::verbose_print!("[godot-neovim] :{}: Forwarded to Neovim", cmd);
    }

    /// :s/old/new/g or :%s/old/new/g - Substitute
    pub(in crate::plugin) fn cmd_substitute(&mut self, cmd: &str) {
        // Parse the substitute command
        // Format: [%]s/pattern/replacement/[g]
        let cmd = cmd.strip_prefix('%').unwrap_or(cmd);
        let cmd = cmd.strip_prefix("s/").unwrap_or(cmd);

        let parts: Vec<&str> = cmd.split('/').collect();
        if parts.len() < 2 {
            godot_warn!("[godot-neovim] Invalid substitute command");
            return;
        }

        let pattern = parts[0];
        let replacement = parts[1];
        let _flags = parts.get(2).unwrap_or(&"");

        // Save for g& command
        self.last_substitute = Some((pattern.to_string(), replacement.to_string()));

        crate::verbose_print!(
            "[godot-neovim] Substitute: '{}' -> '{}'",
            pattern,
            replacement
        );

        // Get current editor and perform replacement
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Get all text, replace, and set back
        let text = editor.get_text().to_string();
        let new_text = text.replace(pattern, replacement);

        if text != new_text {
            // Save cursor position
            let line = editor.get_caret_line();
            let col = editor.get_caret_column();

            editor.set_text_and_notify(&new_text);

            // Restore cursor position (clamped to valid range)
            let max_line = editor.get_line_count() - 1;
            editor.set_caret_line(line.min(max_line));
            editor.set_caret_column(col);

            // Sync to Neovim
            self.sync_buffer_to_neovim();

            crate::verbose_print!("[godot-neovim] Substitution complete");
        } else {
            crate::verbose_print!("[godot-neovim] No matches found for '{}'", pattern);
        }
    }

    /// g& - Repeat last substitution on entire buffer
    pub(in crate::plugin) fn repeat_substitute(&mut self) {
        let Some((ref pattern, ref replacement)) = self.last_substitute.clone() else {
            crate::verbose_print!("[godot-neovim] g&: No previous substitution");
            return;
        };

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        crate::verbose_print!(
            "[godot-neovim] g&: Repeating '{}' -> '{}'",
            pattern,
            replacement
        );

        // Get all text, replace, and set back
        let text = editor.get_text().to_string();
        let new_text = text.replace(pattern, replacement);

        if text != new_text {
            // Save cursor position
            let line = editor.get_caret_line();
            let col = editor.get_caret_column();

            editor.set_text_and_notify(&new_text);

            // Restore cursor position (clamped to valid range)
            let max_line = editor.get_line_count() - 1;
            editor.set_caret_line(line.min(max_line));
            editor.set_caret_column(col);

            // Sync to Neovim
            self.sync_buffer_to_neovim();

            crate::verbose_print!("[godot-neovim] g&: Substitution complete");
        } else {
            crate::verbose_print!("[godot-neovim] g&: No matches found");
        }
    }

    /// :g/pattern/cmd - Global command (execute cmd on lines matching pattern)
    pub(in crate::plugin) fn cmd_global(&mut self, cmd: &str) {
        // Parse: g/pattern/command
        let cmd = cmd.strip_prefix("g/").unwrap_or(cmd);
        let parts: Vec<&str> = cmd.splitn(2, '/').collect();
        if parts.len() < 2 {
            godot_warn!("[godot-neovim] :g - Invalid format. Use :g/pattern/command");
            return;
        }

        let pattern = parts[0];
        let command = parts[1];

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_count = editor.get_line_count();
        let mut matched_lines: Vec<i32> = Vec::new();

        // Find matching lines
        for i in 0..line_count {
            let line_text = editor.get_line(i).to_string();
            if line_text.contains(pattern) {
                matched_lines.push(i);
            }
        }

        if matched_lines.is_empty() {
            crate::verbose_print!("[godot-neovim] :g/{} - No matches", pattern);
            return;
        }

        // Execute command on matching lines (process in reverse to maintain line numbers)
        match command {
            "d" => {
                // Delete matching lines (process in reverse)
                let full_text = editor.get_text().to_string();
                let lines: Vec<&str> = full_text.lines().collect();
                let new_lines: Vec<&str> = lines
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !matched_lines.contains(&(*i as i32)))
                    .map(|(_, l)| *l)
                    .collect();
                editor.set_text_and_notify(&new_lines.join("\n"));
                crate::verbose_print!(
                    "[godot-neovim] :g/{}/d - Deleted {} lines",
                    pattern,
                    matched_lines.len()
                );
            }
            _ => {
                crate::verbose_print!(
                    "[godot-neovim] :g - Found {} matches for '{}'. Command '{}' not yet supported.",
                    matched_lines.len(),
                    pattern,
                    command
                );
            }
        }

        self.sync_buffer_to_neovim();
    }

    /// :sort - Sort lines
    pub(in crate::plugin) fn cmd_sort(&mut self, cmd: &str) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let reverse = cmd.contains('!') || cmd.contains("reverse");
        let unique = cmd.contains('u');

        let full_text = editor.get_text().to_string();
        let mut lines: Vec<&str> = full_text.lines().collect();

        // Sort
        lines.sort();
        if reverse {
            lines.reverse();
        }

        // Remove duplicates if requested
        if unique {
            lines.dedup();
        }

        // Save cursor position
        let line = editor.get_caret_line();
        let col = editor.get_caret_column();

        editor.set_text_and_notify(&lines.join("\n"));

        // Restore cursor
        let max_line = editor.get_line_count() - 1;
        editor.set_caret_line(line.min(max_line));
        editor.set_caret_column(col);

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] :sort{}{} - Sorted {} lines",
            if reverse { "!" } else { "" },
            if unique { " u" } else { "" },
            lines.len()
        );
    }

    /// :t{address} - Copy current line to after {address}
    pub(in crate::plugin) fn cmd_copy_line(&mut self, dest_line: i32) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let line_text = editor.get_line(current_line).to_string();
        let line_count = editor.get_line_count();

        // Insert the line after dest_line (1-indexed in Vim, convert to 0-indexed)
        let insert_after = (dest_line - 1).max(0).min(line_count - 1);

        let full_text = editor.get_text().to_string();
        let lines: Vec<&str> = full_text.lines().collect();

        let mut new_lines: Vec<String> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            new_lines.push(line.to_string());
            if i as i32 == insert_after {
                new_lines.push(line_text.clone());
            }
        }

        editor.set_text_and_notify(&new_lines.join("\n"));

        // Move cursor to the new line
        editor.set_caret_line(insert_after + 1);

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] :t{} - Copied line {} to after line {}",
            dest_line,
            current_line + 1,
            insert_after + 1
        );
    }

    /// :m{address} - Move current line to after {address}
    pub(in crate::plugin) fn cmd_move_line(&mut self, dest_line: i32) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let line_text = editor.get_line(current_line).to_string();
        let line_count = editor.get_line_count();

        // Calculate destination (1-indexed in Vim, convert to 0-indexed)
        let mut insert_after = (dest_line - 1).max(-1).min(line_count - 1);
        if insert_after >= current_line {
            insert_after -= 1; // Account for removed line
        }

        let full_text = editor.get_text().to_string();
        let lines: Vec<&str> = full_text.lines().collect();

        let mut new_lines: Vec<String> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if i as i32 == current_line {
                continue; // Skip the line being moved
            }
            new_lines.push(line.to_string());
            if i as i32 == insert_after || (insert_after < 0 && i == 0) {
                if insert_after < 0 {
                    // Insert at beginning
                    new_lines.insert(0, line_text.clone());
                } else {
                    new_lines.push(line_text.clone());
                }
            }
        }

        // Handle case where inserting at the end
        if insert_after >= lines.len() as i32 - 1 {
            new_lines.push(line_text);
        }

        editor.set_text_and_notify(&new_lines.join("\n"));

        // Move cursor to the new location
        let new_line = if insert_after < 0 {
            0
        } else {
            insert_after + 1
        };
        editor.set_caret_line(new_line.max(0));

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] :m{} - Moved line {} to after line {}",
            dest_line,
            current_line + 1,
            insert_after + 1
        );
    }
}
