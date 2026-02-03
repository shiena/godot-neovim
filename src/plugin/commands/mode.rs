//! Command-line mode management: open/close, history, execute

use super::super::{EditorType, GodotNeovimPlugin};
use godot::classes::Label;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Open command-line mode
    pub(in crate::plugin) fn open_command_line(&mut self) {
        self.clear_pending_input_states();
        self.command_mode = true;
        self.command_buffer = ":".to_string();

        // Show command in mode label with yellow color
        let label = match self.current_editor_type {
            EditorType::Shader => self.shader_mode_label.as_mut(),
            _ => self.mode_label.as_mut(),
        };
        if let Some(label) = label {
            label.set_text(":");
            Self::set_command_mode_color(label);
        }
    }

    /// Set yellow color for command mode
    fn set_command_mode_color(label: &mut Gd<Label>) {
        label.add_theme_color_override("font_color", Color::from_rgb(1.0, 1.0, 0.4));
    }

    /// Close command-line mode
    pub(in crate::plugin) fn close_command_line(&mut self) {
        self.command_mode = false;
        self.command_buffer.clear();

        // Restore mode display (unless showing version)
        if !self.show_version {
            let display_cursor = (self.current_cursor.0 + 1, self.current_cursor.1);
            self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(display_cursor));
        }

        crate::verbose_print!("[godot-neovim] Command-line mode closed");
    }

    /// Update command display in mode label
    pub(in crate::plugin) fn update_command_display(&mut self) {
        let label = match self.current_editor_type {
            EditorType::Shader => self.shader_mode_label.as_mut(),
            _ => self.mode_label.as_mut(),
        };
        if let Some(label) = label {
            label.set_text(&self.command_buffer);
        }
    }

    /// Browse command history (older)
    pub(in crate::plugin) fn command_history_up(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        match self.command_history_index {
            None => {
                // Save current input and start browsing
                self.command_history_temp = self
                    .command_buffer
                    .strip_prefix(':')
                    .unwrap_or("")
                    .to_string();
                self.command_history_index = Some(self.command_history.len() - 1);
            }
            Some(0) => {
                // Already at oldest
                return;
            }
            Some(idx) => {
                self.command_history_index = Some(idx - 1);
            }
        }

        if let Some(idx) = self.command_history_index {
            self.command_buffer = format!(":{}", self.command_history[idx]);
            self.update_command_display();
        }
    }

    /// Browse command history (newer)
    pub(in crate::plugin) fn command_history_down(&mut self) {
        let Some(idx) = self.command_history_index else {
            return;
        };

        if idx >= self.command_history.len() - 1 {
            // Return to current input
            self.command_buffer = format!(":{}", self.command_history_temp);
            self.command_history_index = None;
        } else {
            self.command_history_index = Some(idx + 1);
            self.command_buffer = format!(":{}", self.command_history[idx + 1]);
        }
        self.update_command_display();
    }

    /// @: - Repeat the last Ex command
    pub(in crate::plugin) fn repeat_last_ex_command(&mut self) {
        if let Some(last_cmd) = self.command_history.last().cloned() {
            self.command_buffer = format!(":{}", last_cmd);
            crate::verbose_print!("[godot-neovim] @: Repeating last command: {}", last_cmd);
            self.execute_command();
        } else {
            crate::verbose_print!("[godot-neovim] @: No previous command");
        }
    }

    /// Check if a command starts with a line range specifier
    /// Line ranges: numbers (1,5), special chars (., $), marks ('a, '<, '>), relative (+1, -1)
    fn has_line_range(cmd: &str) -> bool {
        let first_char = cmd.chars().next();
        match first_char {
            // Number: :1,5d, :10d
            Some(c) if c.is_ascii_digit() => true,
            // Current line: :.,$d, :.d
            Some('.') => true,
            // Last line: :$d
            Some('$') => true,
            // Mark: :'<,'>s/old/new/g, :'a,'bd
            Some('\'') => true,
            // Relative: :+1d, :-1d
            Some('+') | Some('-') => true,
            _ => false,
        }
    }

    /// Execute the current command
    pub(in crate::plugin) fn execute_command(&mut self) {
        let command = self.command_buffer.clone();

        // Remove the leading ':'
        let cmd = command.strip_prefix(':').unwrap_or(&command).trim();

        // Save to command history (avoid duplicates of last command)
        if !cmd.is_empty() {
            let cmd_string = cmd.to_string();
            if self.command_history.last() != Some(&cmd_string) {
                self.command_history.push(cmd_string);
            }
        }
        // Reset history browsing
        self.command_history_index = None;
        self.command_history_temp.clear();

        crate::verbose_print!("[godot-neovim] Executing command: {}", cmd);

        match cmd {
            "w" => self.cmd_save(),
            "q" => self.cmd_close(),
            "q!" => self.cmd_close_discard(),
            "qa" | "qall" => self.cmd_close_all(),
            "qa!" | "qall!" => self.cmd_close_all(),
            "wq" | "x" => self.cmd_save_and_close(),
            "wq!" | "x!" => self.cmd_save_and_close(),
            "wa" | "wall" => self.cmd_save_all(),
            "wqa" | "wqall" | "xa" | "xall" | "wqa!" | "wqall!" | "xa!" | "xall!" => {
                self.cmd_save_all();
                self.cmd_close_all();
            }
            "e!" | "edit!" => self.cmd_reload(),
            _ => {
                // Check for :{number} - jump to line (must check before has_line_range)
                // Pure numbers like "100" should use G motion for proper jump list support
                if let Ok(line_num) = cmd.parse::<i32>() {
                    self.cmd_goto_line(line_num);
                }
                // Check for line range commands (e.g., :1,5d, :.,$s/old/new/g)
                // Forward to Neovim for processing (Neovim Master design)
                else if Self::has_line_range(cmd) {
                    self.cmd_forward_to_neovim(cmd);
                }
                // Check for :marks - show marks
                else if cmd == "marks" {
                    self.cmd_show_marks();
                }
                // Check for :registers or :reg - show registers
                else if cmd == "registers" || cmd == "reg" {
                    self.cmd_show_registers();
                }
                // Check for :jumps - show jump list
                else if cmd == "jumps" || cmd == "ju" {
                    self.cmd_show_jumps();
                }
                // Check for :changes - show change list
                else if cmd == "changes" {
                    self.cmd_show_changes();
                }
                // Check for :e[dit] {file} command (or just :e to open quick open)
                else if cmd == "e"
                    || cmd == "edit"
                    || cmd.starts_with("e ")
                    || cmd.starts_with("edit ")
                {
                    let file_path = if cmd == "e" || cmd == "edit" {
                        ""
                    } else if cmd.starts_with("edit ") {
                        cmd.strip_prefix("edit ").unwrap_or("").trim()
                    } else {
                        cmd.strip_prefix("e ").unwrap_or("").trim()
                    };
                    if file_path.is_empty() {
                        // No file path - open quick open dialog immediately
                        self.cmd_edit(file_path);
                    } else {
                        // Defer file open to avoid borrow conflict with on_script_changed
                        self.pending_file_path = Some(file_path.to_string());
                    }
                }
                // Commands forwarded to Neovim for proper undo/register integration
                // (Neovim Master design - see DESIGN_V2.md):
                // - :%s/old/new/g, :s/old/new/g (substitute)
                // - :g/pattern/cmd (global)
                // - :sort
                // - :t{line} (copy line)
                // - :m{line} (move line)
                else if cmd.starts_with("%s/")
                    || cmd.starts_with("s/")
                    || cmd.starts_with("g/")
                    || cmd == "sort"
                    || cmd.starts_with("sort ")
                    || (cmd.starts_with("t") && cmd.len() > 1)
                    || (cmd.starts_with("m") && cmd.len() > 1)
                {
                    self.cmd_forward_to_neovim(cmd);
                }
                // Buffer navigation commands
                else if cmd == "bn" || cmd == "bnext" {
                    self.cmd_buffer_next();
                } else if cmd == "bp" || cmd == "bprev" || cmd == "bprevious" {
                    self.cmd_buffer_prev();
                } else if cmd == "bd" || cmd == "bdelete" {
                    self.cmd_close();
                } else if cmd == "ls" || cmd == "buffers" {
                    self.cmd_list_buffers();
                }
                // :help - open GodotNeovim help
                else if cmd == "help" || cmd == "h" {
                    self.cmd_help();
                }
                // :version - show version in status label
                else if cmd == "version" || cmd == "ver" {
                    self.cmd_version();
                }
                // User-defined commands (start with uppercase) are handled by Neovim
                else if cmd.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    self.cmd_forward_to_neovim(cmd);
                } else {
                    godot_warn!("[godot-neovim] Unknown command: {}", cmd);
                }
            }
        }

        self.close_command_line();
    }
}
