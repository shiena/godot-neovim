//! Command-line mode and Ex commands

use super::GodotNeovimPlugin;
use godot::classes::{EditorInterface, Input, InputEventKey, ResourceSaver, TabBar};
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Open command-line mode
    pub(super) fn open_command_line(&mut self) {
        self.command_mode = true;
        self.command_buffer = ":".to_string();

        // Show command in mode label
        if let Some(ref mut label) = self.mode_label {
            label.set_text(":");
        }
    }

    /// Close command-line mode
    pub(super) fn close_command_line(&mut self) {
        self.command_mode = false;
        self.command_buffer.clear();

        // Restore mode display
        let display_cursor = (self.current_cursor.0 + 1, self.current_cursor.1);
        self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(display_cursor));
    }

    /// Update command display in mode label
    pub(super) fn update_command_display(&mut self) {
        if let Some(ref mut label) = self.mode_label {
            label.set_text(&self.command_buffer);
        }
    }

    /// Browse command history (older)
    pub(super) fn command_history_up(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        match self.command_history_index {
            None => {
                // Save current input and start browsing
                self.command_history_temp = self.command_buffer.strip_prefix(':').unwrap_or("").to_string();
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
    pub(super) fn command_history_down(&mut self) {
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

    /// Execute the current command
    pub(super) fn execute_command(&mut self) {
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
            "qa" | "qall" => self.cmd_close_all(),
            "wq" | "x" => {
                self.cmd_save();
                self.cmd_close();
            }
            _ => {
                // Check for :{number} - jump to line
                if let Ok(line_num) = cmd.parse::<i32>() {
                    self.cmd_goto_line(line_num);
                }
                // Check for :marks - show marks
                else if cmd == "marks" {
                    self.cmd_show_marks();
                }
                // Check for :registers or :reg - show registers
                else if cmd == "registers" || cmd == "reg" {
                    self.cmd_show_registers();
                }
                // Check for :e[dit] {file} command (or just :e to open quick open)
                else if cmd == "e" || cmd == "edit" || cmd.starts_with("e ") || cmd.starts_with("edit ") {
                    let file_path = if cmd == "e" || cmd == "edit" {
                        ""
                    } else if cmd.starts_with("edit ") {
                        cmd.strip_prefix("edit ").unwrap_or("").trim()
                    } else {
                        cmd.strip_prefix("e ").unwrap_or("").trim()
                    };
                    self.cmd_edit(file_path);
                }
                // Check for substitution command :%s/old/new/g
                else if cmd.starts_with("%s/") || cmd.starts_with("s/") {
                    self.cmd_substitute(cmd);
                } else {
                    godot_warn!("[godot-neovim] Unknown command: {}", cmd);
                }
            }
        }

        self.close_command_line();
    }

    /// :{number} - Jump to specific line number
    pub(super) fn cmd_goto_line(&mut self, line_num: i32) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_count = editor.get_line_count();
        // Convert 1-indexed to 0-indexed, clamp to valid range
        let target_line = (line_num - 1).clamp(0, line_count - 1);

        editor.set_caret_line(target_line);

        // Move to first non-blank character (Vim behavior)
        let line_text = editor.get_line(target_line).to_string();
        let first_non_blank = line_text
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);
        editor.set_caret_column(first_non_blank as i32);

        self.sync_cursor_to_neovim();
        crate::verbose_print!("[godot-neovim] :{}: Jumped to line {}", line_num, target_line + 1);
    }

    /// :marks - Show all marks
    pub(super) fn cmd_show_marks(&self) {
        if self.marks.is_empty() {
            godot_print!("[godot-neovim] :marks - No marks set");
            return;
        }

        godot_print!("[godot-neovim] :marks");
        godot_print!("mark  line  col");

        // Sort marks by character
        let mut marks: Vec<_> = self.marks.iter().collect();
        marks.sort_by_key(|(k, _)| *k);

        for (mark, (line, col)) in marks {
            godot_print!(" {}    {:>4}  {:>3}", mark, line + 1, col);
        }
    }

    /// :registers or :reg - Show all registers
    pub(super) fn cmd_show_registers(&self) {
        if self.registers.is_empty() {
            godot_print!("[godot-neovim] :registers - No registers set");
            return;
        }

        godot_print!("[godot-neovim] :registers");

        // Sort registers by character
        let mut regs: Vec<_> = self.registers.iter().collect();
        regs.sort_by_key(|(k, _)| *k);

        for (reg, content) in regs {
            // Truncate long content and show preview
            let preview = if content.len() > 50 {
                format!("{}...", &content[..47])
            } else {
                content.replace('\n', "^J")
            };
            godot_print!("\"{}   {}", reg, preview);
        }
    }

    /// :e[dit] {file} - Open a file in the script editor
    /// If no file is specified, opens the quick open dialog
    pub(super) fn cmd_edit(&self, file_path: &str) {
        let mut editor = EditorInterface::singleton();

        if file_path.is_empty() {
            // No file specified - open quick open dialog
            let callback = Callable::from_fn("quick_open_callback", |args: &[&Variant]| {
                if let Some(path_var) = args.first() {
                    let path: String = path_var.to::<String>();
                    if !path.is_empty() {
                        // Load and open the selected script
                        let resource =
                            godot::classes::ResourceLoader::singleton().load(&path);
                        if let Some(res) = resource {
                            if let Ok(script) = res.try_cast::<godot::classes::Script>() {
                                let mut ed = EditorInterface::singleton();
                                ed.edit_script(&script);
                                crate::verbose_print!(
                                    "[godot-neovim] :e - Opened script from quick open: {}",
                                    path
                                );
                            }
                        }
                    }
                }
                Variant::nil()
            });

            // Filter for Script types
            let mut base_types: Array<StringName> = Array::new();
            base_types.push(&StringName::from("Script"));
            editor
                .popup_quick_open_ex(&callback)
                .base_types(&base_types)
                .done();
            crate::verbose_print!("[godot-neovim] :e - Opened quick open dialog");
            return;
        }

        // Try to load the resource
        let path = if file_path.starts_with("res://") {
            file_path.to_string()
        } else {
            // Assume relative to res://
            format!("res://{}", file_path)
        };

        // Load the resource
        let resource = godot::classes::ResourceLoader::singleton().load(&path);
        if let Some(res) = resource {
            // Try to cast to Script
            if let Ok(script) = res.try_cast::<godot::classes::Script>() {
                // Use edit_script to open the script
                editor.edit_script(&script);
                crate::verbose_print!("[godot-neovim] :e - Opened script: {}", path);
            } else {
                godot_warn!("[godot-neovim] :e - Not a script file: {}", path);
            }
        } else {
            godot_warn!("[godot-neovim] :e - File not found: {}", path);
        }
    }

    /// :w - Save the current file by simulating Ctrl+S
    pub(super) fn cmd_save(&self) {
        // Simulate Ctrl+S to save (avoids re-entrant borrow issues)
        let mut key_press = InputEventKey::new_gd();
        key_press.set_keycode(Key::S);
        key_press.set_ctrl_pressed(true);
        key_press.set_pressed(true);
        Input::singleton().parse_input_event(&key_press);

        // Release the key (must be a new instance to avoid same-frame warning)
        let mut key_release = InputEventKey::new_gd();
        key_release.set_keycode(Key::S);
        key_release.set_ctrl_pressed(true);
        key_release.set_pressed(false);
        Input::singleton().parse_input_event(&key_release);

        crate::verbose_print!("[godot-neovim] :w - Save triggered (Ctrl+S)");
    }

    /// ZZ - Save and close (using ResourceSaver for synchronous save)
    pub(super) fn cmd_save_and_close(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(mut script_editor) = editor.get_script_editor() {
            if let Some(current_script) = script_editor.get_current_script() {
                let path = current_script.get_path();
                if !path.is_empty() {
                    // Save the script using ResourceSaver (synchronous)
                    let result = ResourceSaver::singleton()
                        .save_ex(&current_script)
                        .path(&path)
                        .done();
                    if result == godot::global::Error::OK {
                        crate::verbose_print!("[godot-neovim] ZZ - Saved: {}", path);
                    } else {
                        godot_warn!("[godot-neovim] ZZ - Failed to save: {}", path);
                    }
                }
            }
        }

        // Now close the tab
        self.cmd_close();
    }

    /// :q - Close the current script tab by simulating Ctrl+W
    pub(super) fn cmd_close(&mut self) {
        // Clear current editor reference before closing to avoid accessing freed instance
        self.current_editor = None;

        // Simulate Ctrl+W key press
        let mut key_press = InputEventKey::new_gd();
        key_press.set_keycode(Key::W);
        key_press.set_ctrl_pressed(true);
        key_press.set_pressed(true);
        Input::singleton().parse_input_event(&key_press);

        // Release the key
        let mut key_release = InputEventKey::new_gd();
        key_release.set_keycode(Key::W);
        key_release.set_ctrl_pressed(true);
        key_release.set_pressed(false);
        Input::singleton().parse_input_event(&key_release);

        crate::verbose_print!("[godot-neovim] :q - Close triggered (Ctrl+W)");
    }

    /// ZQ - Close without saving (discard changes)
    pub(super) fn cmd_close_discard(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(mut script_editor) = editor.get_script_editor() {
            // Reload the script from disk and sync the CodeEdit
            if let Some(mut current_script) = script_editor.get_current_script() {
                let path = current_script.get_path();
                if !path.is_empty() {
                    // Reload the script from disk
                    let _ = current_script.reload();

                    // Also update the CodeEdit to match the reloaded script
                    if let Some(mut code_edit) = self.current_editor.clone() {
                        let source = current_script.get_source_code();
                        code_edit.set_text(&source);
                        // Mark as saved to clear the unsaved state
                        code_edit.tag_saved_version();
                        crate::verbose_print!(
                            "[godot-neovim] ZQ - Synced CodeEdit and tagged as saved: {}",
                            path
                        );
                    }
                }
            }
        }

        // Now close the tab (should not prompt since changes are discarded)
        self.current_editor = None;

        // Simulate Ctrl+W key press
        let mut key_press = InputEventKey::new_gd();
        key_press.set_keycode(Key::W);
        key_press.set_ctrl_pressed(true);
        key_press.set_pressed(true);
        Input::singleton().parse_input_event(&key_press);

        let mut key_release = InputEventKey::new_gd();
        key_release.set_keycode(Key::W);
        key_release.set_ctrl_pressed(true);
        key_release.set_pressed(false);
        Input::singleton().parse_input_event(&key_release);

        crate::verbose_print!("[godot-neovim] ZQ - Close triggered (discard changes)");
    }

    /// :qa/:qall - Close all script tabs
    pub(super) fn cmd_close_all(&mut self) {
        // Clear references before closing to avoid accessing freed instances
        self.current_editor = None;
        self.mode_label = None;

        // Get the number of open scripts
        let editor = EditorInterface::singleton();
        let script_count = if let Some(script_editor) = editor.get_script_editor() {
            script_editor.get_open_scripts().len()
        } else {
            0
        };

        // Close each script by simulating Ctrl+W multiple times
        for _ in 0..script_count {
            let mut key_event = InputEventKey::new_gd();
            key_event.set_keycode(Key::W);
            key_event.set_ctrl_pressed(true);
            key_event.set_pressed(true);
            Input::singleton().parse_input_event(&key_event);
        }

        crate::verbose_print!(
            "[godot-neovim] :qa - Close all triggered ({} scripts)",
            script_count
        );
    }

    /// :s/old/new/g or :%s/old/new/g - Substitute
    pub(super) fn cmd_substitute(&mut self, cmd: &str) {
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

            editor.set_text(&new_text);

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

    /// gt - Go to next script tab by simulating Ctrl+Tab
    pub(super) fn next_script_tab(&self) {
        let mut key_event = InputEventKey::new_gd();
        key_event.set_keycode(Key::TAB);
        key_event.set_ctrl_pressed(true);
        key_event.set_pressed(true);
        Input::singleton().parse_input_event(&key_event);
        crate::verbose_print!("[godot-neovim] gt - Next tab (Ctrl+Tab)");
    }

    /// gT - Go to previous script tab by simulating Ctrl+Shift+Tab
    pub(super) fn prev_script_tab(&self) {
        let mut key_event = InputEventKey::new_gd();
        key_event.set_keycode(Key::TAB);
        key_event.set_ctrl_pressed(true);
        key_event.set_shift_pressed(true);
        key_event.set_pressed(true);
        Input::singleton().parse_input_event(&key_event);
        crate::verbose_print!("[godot-neovim] gT - Previous tab (Ctrl+Shift+Tab)");
    }

    /// Find TabBar in the ScriptEditor hierarchy
    #[allow(dead_code)]
    pub(super) fn find_tab_bar(&self, node: Gd<godot::classes::Control>) -> Option<Gd<TabBar>> {
        // Check if this node is a TabBar
        if let Ok(tab_bar) = node.clone().try_cast::<TabBar>() {
            // Make sure it has tabs (script tabs, not other TabBars)
            if tab_bar.get_tab_count() > 0 {
                crate::verbose_print!(
                    "[godot-neovim] Found TabBar with {} tabs",
                    tab_bar.get_tab_count()
                );
                return Some(tab_bar);
            }
        }

        // Search children
        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Ok(control) = child.try_cast::<godot::classes::Control>() {
                    if let Some(tab_bar) = self.find_tab_bar(control) {
                        return Some(tab_bar);
                    }
                }
            }
        }

        None
    }

    /// Debug: Print node hierarchy to find TabBar
    #[allow(dead_code)]
    pub(super) fn debug_print_hierarchy(&self, node: Gd<godot::classes::Control>, depth: i32) {
        let indent = "  ".repeat(depth as usize);
        let class_name = node.get_class();
        let node_name = node.get_name();
        crate::verbose_print!("{}[{}] {}", indent, class_name, node_name);

        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Ok(control) = child.try_cast::<godot::classes::Control>() {
                    if depth < 5 {
                        // Limit depth to avoid too much output
                        self.debug_print_hierarchy(control, depth + 1);
                    }
                }
            }
        }
    }
}
