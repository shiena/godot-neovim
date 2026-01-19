//! Command-line mode and Ex commands

use super::{CodeEditExt, GodotNeovimPlugin};
use godot::classes::{EditorInterface, Input, InputEventKey, ResourceSaver};
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Open command-line mode
    pub(super) fn open_command_line(&mut self) {
        self.clear_pending_input_states();
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

        // Restore mode display (unless showing version)
        if !self.show_version {
            let display_cursor = (self.current_cursor.0 + 1, self.current_cursor.1);
            self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(display_cursor));
        }
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

    /// @: - Repeat the last Ex command
    pub(super) fn repeat_last_ex_command(&mut self) {
        if let Some(last_cmd) = self.command_history.last().cloned() {
            self.command_buffer = format!(":{}", last_cmd);
            crate::verbose_print!("[godot-neovim] @: Repeating last command: {}", last_cmd);
            self.execute_command();
        } else {
            crate::verbose_print!("[godot-neovim] @: No previous command");
        }
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
            "wq" | "x" => self.cmd_save_and_close(),
            "wa" | "wall" => self.cmd_save_all(),
            "wqa" | "wqall" | "xa" | "xall" => {
                self.cmd_save_all();
                self.cmd_close_all();
            }
            "e!" | "edit!" => self.cmd_reload(),
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
                // Check for substitution command :%s/old/new/g
                else if cmd.starts_with("%s/") || cmd.starts_with("s/") {
                    self.cmd_substitute(cmd);
                }
                // Check for global command :g/pattern/cmd
                else if cmd.starts_with("g/") {
                    self.cmd_global(cmd);
                }
                // Check for :sort command
                else if cmd == "sort" || cmd.starts_with("sort ") {
                    self.cmd_sort(cmd);
                }
                // Check for :t (copy line) command
                else if cmd.starts_with("t") && cmd.len() > 1 {
                    let dest = cmd[1..].trim();
                    if let Ok(line_num) = dest.parse::<i32>() {
                        self.cmd_copy_line(line_num);
                    }
                }
                // Check for :m (move line) command
                else if cmd.starts_with("m") && cmd.len() > 1 {
                    let dest = cmd[1..].trim();
                    if let Ok(line_num) = dest.parse::<i32>() {
                        self.cmd_move_line(line_num);
                    }
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
                } else {
                    godot_warn!("[godot-neovim] Unknown command: {}", cmd);
                }
            }
        }

        self.close_command_line();
    }

    /// :{number} - Jump to specific line number (Neovim Master design)
    pub(super) fn cmd_goto_line(&mut self, line_num: i32) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        // Send command to Neovim - it will process and send win_viewport event
        // This ensures cursor sync is consistent with Neovim's state
        self.send_keys(&format!(":{}<CR>", line_num));

        crate::verbose_print!("[godot-neovim] :{}: Sent to Neovim", line_num);
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

    /// :jumps - Show the jump list
    pub(super) fn cmd_show_jumps(&self) {
        godot_print!("[godot-neovim] :jumps");
        godot_print!(" jump line  col");

        if self.jump_list.is_empty() {
            godot_print!("   (empty)");
            return;
        }

        for (i, (line, col)) in self.jump_list.iter().enumerate() {
            let marker = if i == self.jump_list_pos { ">" } else { " " };
            godot_print!("{}{:>4}  {:>4}  {:>3}", marker, i + 1, line + 1, col);
        }

        if self.jump_list_pos >= self.jump_list.len() {
            godot_print!(">          (current)");
        }
    }

    /// :changes - Show the change list (simplified - we don't track changes)
    pub(super) fn cmd_show_changes(&self) {
        godot_print!("[godot-neovim] :changes");
        godot_print!("   (change list not tracked)");
        godot_print!("   Use undo/redo (u/Ctrl+R) for changes");
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
                        let resource = godot::classes::ResourceLoader::singleton().load(&path);
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

        // Emit name_changed to update the script list UI (remove dirty marker)
        if let Some(ref editor) = self.current_editor {
            let mut current: Option<Gd<Node>> = editor.get_parent();
            while let Some(node) = current {
                if node.get_class() == "ScriptTextEditor".into() {
                    let mut script_editor = node;
                    script_editor.emit_signal("name_changed", &[]);
                    break;
                }
                current = node.get_parent();
            }
        }

        crate::verbose_print!("[godot-neovim] :w - Save triggered (Ctrl+S)");
    }

    /// :wa/:wall - Save all open scripts
    pub(super) fn cmd_save_all(&self) {
        let editor = EditorInterface::singleton();
        if let Some(script_editor) = editor.get_script_editor() {
            let open_scripts = script_editor.get_open_scripts();
            let mut saved_count = 0;

            for i in 0..open_scripts.len() {
                if let Some(script_var) = open_scripts.get(i) {
                    if let Ok(script) = script_var.try_cast::<godot::classes::Script>() {
                        let path = script.get_path();
                        if !path.is_empty() {
                            let result = ResourceSaver::singleton()
                                .save_ex(&script)
                                .path(&path)
                                .done();
                            if result == godot::global::Error::OK {
                                saved_count += 1;
                            }
                        }
                    }
                }
            }

            crate::verbose_print!("[godot-neovim] :wa - Saved {} script(s)", saved_count);
        }
    }

    /// :e!/:edit! - Reload current file from disk (discard changes)
    pub(super) fn cmd_reload(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(mut script_editor) = editor.get_script_editor() {
            if let Some(mut current_script) = script_editor.get_current_script() {
                let path = current_script.get_path();
                if !path.is_empty() {
                    // Reload the script from disk
                    let _ = current_script.reload();

                    // Update the CodeEdit to match the reloaded script
                    if let Some(ref mut code_edit) = self.current_editor {
                        let source = current_script.get_source_code();
                        code_edit.set_text(&source);
                        code_edit.tag_saved_version();

                        // Sync to Neovim
                        self.sync_buffer_to_neovim();
                    }

                    crate::verbose_print!("[godot-neovim] :e! - Reloaded: {}", path);
                } else {
                    godot_warn!("[godot-neovim] :e! - No file to reload (new buffer)");
                }
            }
        }
    }

    /// ZZ/:wq - Save and close (sync CodeEdit content to Script, then save)
    pub(super) fn cmd_save_and_close(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(mut script_editor) = editor.get_script_editor() {
            if let Some(mut current_script) = script_editor.get_current_script() {
                let path = current_script.get_path();
                if !path.is_empty() {
                    // Sync CodeEdit content to Script resource before saving
                    if let Some(ref code_editor) = self.current_editor {
                        if code_editor.is_instance_valid() {
                            let text = code_editor.get_text();
                            current_script.set_source_code(&text);
                        }
                    }

                    // Save the script using ResourceSaver (synchronous)
                    let result = ResourceSaver::singleton()
                        .save_ex(&current_script)
                        .path(&path)
                        .done();
                    if result == godot::global::Error::OK {
                        // Mark CodeEdit as saved to clear dirty flag
                        if let Some(ref mut code_editor) = self.current_editor {
                            if code_editor.is_instance_valid() {
                                code_editor.tag_saved_version();
                            }
                        }
                        crate::verbose_print!("[godot-neovim] :wq/ZZ - Saved: {}", path);
                    } else {
                        godot_warn!("[godot-neovim] :wq/ZZ - Failed to save: {}", path);
                    }
                }
            }
        }

        // Now close the tab
        self.cmd_close();
    }

    /// :q - Close the current script tab by simulating Ctrl+W
    pub(super) fn cmd_close(&mut self) {
        // Disconnect from signals BEFORE closing to avoid
        // accessing freed CodeEdit instance
        self.disconnect_caret_changed_signal();
        self.disconnect_resized_signal();

        // Sync cursor to Neovim BEFORE closing, because on_script_changed
        // is called after the editor is freed and we can't read cursor then
        if let Some(ref editor) = self.current_editor {
            if editor.is_instance_valid() {
                let line = editor.get_caret_line() as i64 + 1; // 1-indexed for Neovim
                let col = editor.get_caret_column() as i64;
                if let Some(ref neovim) = self.neovim {
                    if let Ok(client) = neovim.try_lock() {
                        let _ = client.set_cursor(line, col);
                        // Set flag to skip cursor sync in on_script_changed
                        self.cursor_synced_before_close = true;
                        crate::verbose_print!(
                            "[godot-neovim] :q - Synced cursor to Neovim before close: ({}, {})",
                            line,
                            col
                        );
                    }
                }
            }
        }

        // Don't clear current_editor here - if user cancels the save dialog,
        // the script stays open and we need to keep the reference.
        // When the script actually closes, on_script_changed will handle cleanup.

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
        // Disconnect from signals BEFORE closing
        self.disconnect_caret_changed_signal();
        self.disconnect_resized_signal();

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

                    // Also reload Neovim buffer to discard changes there too
                    if let Some(ref neovim) = self.neovim {
                        if let Ok(client) = neovim.try_lock() {
                            // :e! reloads the current buffer from disk
                            let _ = client.command("e!");
                            crate::verbose_print!(
                                "[godot-neovim] ZQ - Reloaded Neovim buffer: {}",
                                path
                            );
                        }
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
    /// Uses the same mechanism as Godot's "Close All" menu option
    /// Note: Neovim buffer deletion is handled by on_script_close signal
    pub(super) fn cmd_close_all(&mut self) {
        use godot::classes::MenuButton;

        // Disconnect from signals BEFORE closing
        self.disconnect_caret_changed_signal();
        self.disconnect_resized_signal();

        // Set flag to skip on_script_changed processing during close all
        // Will be reset by process() when operation completes
        self.closing_all_tabs = true;

        // Clear current editor reference since it will be freed
        self.current_editor = None;

        let editor = EditorInterface::singleton();
        let Some(script_editor) = editor.get_script_editor() else {
            godot_warn!("[godot-neovim] :qa - Could not find ScriptEditor");
            self.closing_all_tabs = false;
            return;
        };

        let script_editor_node: Gd<Node> = script_editor.upcast();

        // Structure: ScriptEditor -> VBoxContainer -> HBoxContainer (menu_hb) -> MenuButton (File)
        let children = script_editor_node.get_children();
        for i in 0..children.len() {
            if let Some(child) = children.get(i) {
                if child.is_class("VBoxContainer") {
                    let vbox_children = child.get_children();
                    for j in 0..vbox_children.len() {
                        if let Some(vbox_child) = vbox_children.get(j) {
                            if vbox_child.is_class("HBoxContainer") {
                                // Found menu_hb, now find MenuButton (File)
                                let hbox_children = vbox_child.get_children();
                                for k in 0..hbox_children.len() {
                                    if let Some(hbox_child) = hbox_children.get(k) {
                                        if hbox_child.is_class("MenuButton") {
                                            let menu_button: Gd<MenuButton> = hbox_child.cast();
                                            if let Some(mut popup) = menu_button.get_popup() {
                                                // FILE_MENU_CLOSE_ALL = 16
                                                const FILE_MENU_CLOSE_ALL: i64 = 16;
                                                // Use call_deferred to avoid borrow conflict:
                                                // emit_signal triggers script_close synchronously,
                                                // which calls on_script_close needing &mut self
                                                popup.call_deferred(
                                                    "emit_signal",
                                                    &[
                                                        "id_pressed".to_variant(),
                                                        FILE_MENU_CLOSE_ALL.to_variant(),
                                                    ],
                                                );
                                                crate::verbose_print!(
                                                    "[godot-neovim] :qa - call_deferred emit_signal(id_pressed, {})",
                                                    FILE_MENU_CLOSE_ALL
                                                );
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        godot_warn!("[godot-neovim] :qa - Could not find File menu in ScriptEditor");
        self.closing_all_tabs = false;
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
    pub(super) fn repeat_substitute(&mut self) {
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

    /// gt - Go to next script tab
    pub(super) fn next_script_tab(&mut self) {
        self.switch_script_tab(1);
    }

    /// gT - Go to previous script tab
    pub(super) fn prev_script_tab(&mut self) {
        self.switch_script_tab(-1);
    }

    /// Switch script tab by offset (1 = next, -1 = previous)
    fn switch_script_tab(&mut self, offset: i32) {
        let mut editor = EditorInterface::singleton();
        let Some(mut script_editor) = editor.get_script_editor() else {
            return;
        };

        let open_scripts = script_editor.get_open_scripts();
        let count = open_scripts.len() as i32;
        if count <= 1 {
            crate::verbose_print!(
                "[godot-neovim] switch_script_tab: only {} script(s) open",
                count
            );
            return;
        }

        // Find current script index
        let current_script = script_editor.get_current_script();
        let current_path = current_script
            .as_ref()
            .map(|s| s.get_path().to_string())
            .unwrap_or_default();

        let mut current_idx: i32 = 0;
        for i in 0..count {
            if let Some(script_var) = open_scripts.get(i as usize) {
                if let Ok(script) = script_var.try_cast::<godot::classes::Script>() {
                    if script.get_path().to_string() == current_path {
                        current_idx = i;
                        break;
                    }
                }
            }
        }

        // Calculate new index with wrapping
        let new_idx = ((current_idx + offset) % count + count) % count;

        // Get the target script and switch using call_deferred
        if let Some(script_var) = open_scripts.get(new_idx as usize) {
            if let Ok(script) = script_var.try_cast::<godot::classes::Script>() {
                let new_path = script.get_path().to_string();
                crate::verbose_print!(
                    "[godot-neovim] Tab switch: {} -> {} ({})",
                    current_idx,
                    new_idx,
                    new_path
                );
                // Use call_deferred to avoid blocking during input handling
                editor.call_deferred("edit_script", &[script.to_variant()]);
            }
        }
    }

    /// Start backward search (? command) - opens Godot's search dialog
    pub(super) fn start_search_backward(&self) {
        // Simulate Ctrl+F to open the search dialog
        let mut key_event = InputEventKey::new_gd();
        key_event.set_keycode(Key::F);
        key_event.set_ctrl_pressed(true);
        key_event.set_pressed(true);
        Input::singleton().parse_input_event(&key_event);

        crate::verbose_print!(
            "[godot-neovim] ?: Opening search dialog (use Find Previous for backward search)"
        );
    }

    /// :g/pattern/cmd - Global command (execute cmd on lines matching pattern)
    pub(super) fn cmd_global(&mut self, cmd: &str) {
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
    pub(super) fn cmd_sort(&mut self, cmd: &str) {
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
    pub(super) fn cmd_copy_line(&mut self, dest_line: i32) {
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
    pub(super) fn cmd_move_line(&mut self, dest_line: i32) {
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

    /// :bn / :bnext - Go to next buffer (script tab)
    pub(super) fn cmd_buffer_next(&mut self) {
        self.next_script_tab();
    }

    /// :bp / :bprev - Go to previous buffer (script tab)
    pub(super) fn cmd_buffer_prev(&mut self) {
        self.prev_script_tab();
    }

    /// :ls / :buffers - List open buffers
    pub(super) fn cmd_list_buffers(&self) {
        let editor = EditorInterface::singleton();
        if let Some(script_editor) = editor.get_script_editor() {
            let open_scripts = script_editor.get_open_scripts();

            godot_print!("[godot-neovim] :ls - Open buffers:");
            for i in 0..open_scripts.len() {
                if let Some(script_var) = open_scripts.get(i) {
                    if let Ok(script) = script_var.try_cast::<godot::classes::Script>() {
                        let path = script.get_path().to_string();
                        let name = path.split('/').next_back().unwrap_or(&path);
                        godot_print!("  {}: {}", i + 1, name);
                    }
                }
            }
        }
    }

    /// :help - Open GodotNeovim help
    pub(super) fn cmd_help(&mut self) {
        use super::{HelpMemberType, HelpQuery};

        self.pending_help_query = Some(HelpQuery {
            class_name: "GodotNeovim".to_string(),
            member_name: None,
            member_type: HelpMemberType::Class,
        });
    }

    /// :version - Show godot-neovim version in status label
    pub(super) fn cmd_version(&mut self) {
        self.show_version = true;
        self.update_version_display();
    }

    /// K - Open documentation for word under cursor
    /// Uses LSP hover to get class/member information for methods, properties, and signals
    /// Note: Actual goto_help() call is deferred to process() to avoid borrow conflicts
    /// (goto_help triggers editor_script_changed signal synchronously)
    pub(super) fn open_documentation(&mut self) {
        use super::{HelpMemberType, HelpQuery};
        use godot::classes::ProjectSettings;

        let Some(ref editor) = self.current_editor else {
            return;
        };

        // Get word under cursor
        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        if col_idx >= chars.len() {
            return;
        }

        // Find word boundaries
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        let mut start = col_idx;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = col_idx;
        while end < chars.len() && is_word_char(chars[end]) {
            end += 1;
        }

        if start == end {
            return;
        }

        let word: String = chars[start..end].iter().collect();
        crate::verbose_print!("[godot-neovim] K: Looking up help for '{}'", word);

        // If word starts with uppercase, assume it's a class name (fast path)
        if word.chars().next().is_some_and(|c| c.is_uppercase()) {
            self.pending_help_query = Some(HelpQuery {
                class_name: word.clone(),
                member_name: None,
                member_type: HelpMemberType::Class,
            });
            crate::verbose_print!("[godot-neovim] K: Queueing class help for '{}'", word);
            return;
        }

        // Try LSP hover to get class/member information
        let Some(ref lsp) = self.godot_lsp else {
            crate::verbose_print!("[godot-neovim] K: LSP not available, skipping '{}'", word);
            return;
        };

        // Get absolute file path and convert to URI
        let abs_path = if self.current_script_path.starts_with("res://") {
            ProjectSettings::singleton()
                .globalize_path(&self.current_script_path)
                .to_string()
        } else {
            self.current_script_path.clone()
        };

        let uri = if abs_path.starts_with('/') {
            format!("file://{}", abs_path)
        } else {
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

        // Ensure LSP is connected and initialized
        if !lsp.is_connected() {
            if let Err(e) = lsp.connect(6005) {
                crate::verbose_print!("[godot-neovim] K: LSP connect failed: {}", e);
                return;
            }
        }

        if !lsp.is_initialized() {
            if let Err(e) = lsp.initialize(&root_uri) {
                crate::verbose_print!("[godot-neovim] K: LSP init failed: {}", e);
                return;
            }
        }

        // Request hover information
        let line = line_idx as u32;
        let col = col_idx as u32;
        let hover_result = lsp.hover(&uri, line, col);

        match hover_result {
            Ok(Some(hover)) => {
                // Parse hover contents to extract class and member information
                if let Some(query) = Self::parse_hover_for_help(&hover, &word) {
                    crate::verbose_print!(
                        "[godot-neovim] K: LSP hover found - class: {}, member: {:?}, type: {:?}",
                        query.class_name,
                        query.member_name,
                        query.member_type
                    );
                    self.pending_help_query = Some(query);
                } else {
                    crate::verbose_print!("[godot-neovim] K: Could not parse hover for '{}'", word);
                }
            }
            Ok(None) => {
                crate::verbose_print!("[godot-neovim] K: No hover info for '{}'", word);
            }
            Err(e) => {
                crate::verbose_print!("[godot-neovim] K: LSP hover error: {}", e);
            }
        }
    }

    /// Parse LSP hover response to extract class/member information for goto_help()
    fn parse_hover_for_help(hover: &lsp_types::Hover, word: &str) -> Option<super::HelpQuery> {
        use super::{HelpMemberType, HelpQuery};
        use lsp_types::{HoverContents, MarkedString, MarkupContent};

        // Extract the hover content as a string
        let content = match &hover.contents {
            HoverContents::Scalar(marked) => match marked {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(ls) => ls.value.clone(),
            },
            HoverContents::Array(arr) => arr
                .iter()
                .map(|m| match m {
                    MarkedString::String(s) => s.as_str(),
                    MarkedString::LanguageString(ls) => ls.value.as_str(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
            HoverContents::Markup(MarkupContent { value, .. }) => value.clone(),
        };

        crate::verbose_print!("[godot-neovim] K: Parsing hover content: {}", content);

        // Parse patterns from Godot LSP hover response:
        // - "var ClassName.property_name" -> property
        // - "const ClassName.CONSTANT_NAME" -> constant
        // - "func method_name(...) -> Type" -> method (need to find class from context)
        // - "signal signal_name(...)" -> signal
        // - "<Native> class ClassName" -> class

        // Pattern: "var ClassName.member" or "const ClassName.MEMBER"
        // Regex: (var|const)\s+(\w+)\.(\w+)
        if let Some(caps) = Self::match_class_member(&content) {
            let (keyword, class_name, member_name) = caps;
            let member_type = match keyword {
                "var" => HelpMemberType::Property,
                "const" => HelpMemberType::Constant,
                _ => HelpMemberType::Property,
            };
            return Some(HelpQuery {
                class_name,
                member_name: Some(member_name),
                member_type,
            });
        }

        // Pattern: "func ClassName.method_name(" - method
        if content.contains("func ") && content.contains('(') {
            // Extract class name from "func ClassName.method_name" pattern
            if let Some((class_name, method_name)) = Self::match_func_class_method(&content) {
                return Some(HelpQuery {
                    class_name,
                    member_name: Some(method_name),
                    member_type: HelpMemberType::Method,
                });
            }
            // Fallback: try to extract class from "Defined in" link
            if let Some(class_name) = Self::extract_class_from_defined_in(&content) {
                return Some(HelpQuery {
                    class_name,
                    member_name: Some(word.to_string()),
                    member_type: HelpMemberType::Method,
                });
            }
        }

        // Pattern: "signal ClassName.signal_name(" or "signal signal_name(" - signal
        if content.contains("signal ") && content.contains('(') {
            // Extract class name from "signal ClassName.signal_name" pattern
            if let Some((class_name, signal_name)) = Self::match_signal_class_member(&content) {
                return Some(HelpQuery {
                    class_name,
                    member_name: Some(signal_name),
                    member_type: HelpMemberType::Signal,
                });
            }
            // Fallback: try to extract class from "Defined in" link
            if let Some(class_name) = Self::extract_class_from_defined_in(&content) {
                return Some(HelpQuery {
                    class_name,
                    member_name: Some(word.to_string()),
                    member_type: HelpMemberType::Signal,
                });
            }
        }

        // Pattern: "<Native> class ClassName" or just class name
        if content.contains("class ") {
            // Try to extract class name after "class "
            for line in content.lines() {
                if let Some(idx) = line.find("class ") {
                    let rest = &line[idx + 6..];
                    let class_name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !class_name.is_empty() {
                        return Some(HelpQuery {
                            class_name,
                            member_name: None,
                            member_type: HelpMemberType::Class,
                        });
                    }
                }
            }
        }

        None
    }

    /// Match "var ClassName.member" or "const ClassName.MEMBER" pattern
    fn match_class_member(content: &str) -> Option<(&'static str, String, String)> {
        for line in content.lines() {
            let line = line.trim();

            // Check for "var ClassName.member"
            if let Some(rest) = line.strip_prefix("var ") {
                if let Some((class, member)) = rest.split_once('.') {
                    let class_name: String = class
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let member_name: String = member
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !class_name.is_empty() && !member_name.is_empty() {
                        return Some(("var", class_name, member_name));
                    }
                }
            }

            // Check for "const ClassName.MEMBER"
            if let Some(rest) = line.strip_prefix("const ") {
                if let Some((class, member)) = rest.split_once('.') {
                    let class_name: String = class
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let member_name: String = member
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !class_name.is_empty() && !member_name.is_empty() {
                        return Some(("const", class_name, member_name));
                    }
                }
            }
        }
        None
    }

    /// Extract class name from "Defined in [path](uri)" pattern
    fn extract_class_from_defined_in(content: &str) -> Option<String> {
        // Look for "Defined in [filename.gd]" and extract class from path
        // Native classes: look for res://... path or builtin class reference

        for line in content.lines() {
            // Pattern: "Defined in [path/ClassName.gd]"
            if line.contains("Defined in") {
                // Extract filename from markdown link [filename](uri) or just [filename]
                if let Some(start) = line.find('[') {
                    if let Some(end) = line[start..].find(']') {
                        let path = &line[start + 1..start + end];
                        // Extract class name from filename (e.g., "node.gd" -> "Node")
                        if let Some(filename) = path.split('/').next_back() {
                            if let Some(name) = filename.strip_suffix(".gd") {
                                // Convert snake_case to PascalCase for class name
                                let class_name = Self::to_pascal_case(name);
                                if !class_name.is_empty() {
                                    return Some(class_name);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fallback: Look for common native class patterns in the content
        // The hover might mention the class in the description
        let native_classes = [
            "Node",
            "Node2D",
            "Node3D",
            "Control",
            "Sprite2D",
            "Sprite3D",
            "Camera2D",
            "Camera3D",
            "CharacterBody2D",
            "CharacterBody3D",
            "RigidBody2D",
            "RigidBody3D",
            "Area2D",
            "Area3D",
            "CollisionShape2D",
            "CollisionShape3D",
            "AnimationPlayer",
            "AudioStreamPlayer",
            "Timer",
            "Label",
            "Button",
            "LineEdit",
            "TextEdit",
            "Panel",
            "Container",
            "HBoxContainer",
            "VBoxContainer",
            "GridContainer",
            "ScrollContainer",
            "TabContainer",
            "Resource",
            "PackedScene",
            "Texture2D",
            "Mesh",
            "Material",
            "Shader",
            "Script",
            "GDScript",
            "Object",
            "RefCounted",
            "Vector2",
            "Vector3",
            "Vector4",
            "Color",
            "Rect2",
            "Transform2D",
            "Transform3D",
            "Basis",
            "Quaternion",
            "AABB",
            "Plane",
            "Array",
            "Dictionary",
            "String",
            "StringName",
            "NodePath",
            "Signal",
            "Callable",
            "PackedByteArray",
            "PackedInt32Array",
            "PackedInt64Array",
            "PackedFloat32Array",
            "PackedFloat64Array",
            "PackedStringArray",
            "PackedVector2Array",
            "PackedVector3Array",
            "PackedColorArray",
            "Input",
            "InputEvent",
            "InputEventKey",
            "InputEventMouse",
            "InputEventMouseButton",
            "InputEventMouseMotion",
            "OS",
            "Engine",
            "ProjectSettings",
            "EditorInterface",
            "EditorPlugin",
            "SceneTree",
            "Viewport",
            "Window",
            "DisplayServer",
            "RenderingServer",
            "PhysicsServer2D",
            "PhysicsServer3D",
            "NavigationServer2D",
            "NavigationServer3D",
            "AudioServer",
            "Time",
            "Performance",
            "Geometry2D",
            "Geometry3D",
            "ResourceLoader",
            "ResourceSaver",
            "FileAccess",
            "DirAccess",
            "JSON",
            "XMLParser",
            "RegEx",
            "Tween",
            "AnimationTree",
            "AnimationNodeStateMachine",
            "AnimationNodeBlendTree",
            "AnimatedSprite2D",
            "AnimatedSprite3D",
            "TileMap",
            "TileSet",
            "CanvasItem",
            "CanvasLayer",
            "ParallaxBackground",
            "ParallaxLayer",
            "PathFollow2D",
            "PathFollow3D",
            "Path2D",
            "Path3D",
            "Curve",
            "Curve2D",
            "Curve3D",
            "Gradient",
            "GradientTexture1D",
            "GradientTexture2D",
            "Image",
            "ImageTexture",
            "AtlasTexture",
            "CompressedTexture2D",
            "Environment",
            "WorldEnvironment",
            "DirectionalLight3D",
            "OmniLight3D",
            "SpotLight3D",
            "Sky",
            "ProceduralSkyMaterial",
            "PhysicalSkyMaterial",
            "PanoramaSkyMaterial",
            "ShaderMaterial",
            "StandardMaterial3D",
            "ORMMaterial3D",
            "BaseMaterial3D",
            "HTTPRequest",
            "HTTPClient",
            "StreamPeer",
            "StreamPeerTCP",
            "StreamPeerTLS",
            "PacketPeer",
            "PacketPeerUDP",
            "TCPServer",
            "UDPServer",
            "WebSocketPeer",
            "MultiplayerAPI",
            "MultiplayerPeer",
            "ENetMultiplayerPeer",
            "WebSocketMultiplayerPeer",
            "Thread",
            "Mutex",
            "Semaphore",
        ];

        for class in native_classes {
            // Check if content mentions this class (case-sensitive)
            if content.contains(class) {
                return Some(class.to_string());
            }
        }

        None
    }

    /// Convert snake_case to PascalCase
    fn to_pascal_case(s: &str) -> String {
        s.split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                }
            })
            .collect()
    }

    /// Extract class name and method name from "func ClassName.method_name(...)" pattern
    fn match_func_class_method(content: &str) -> Option<(String, String)> {
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("func ") {
                if let Some((class_part, method_part)) = rest.split_once('.') {
                    let class_name: String = class_part
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let method_name: String = method_part
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();

                    if !class_name.is_empty() && !method_name.is_empty() {
                        return Some((class_name, method_name));
                    }
                }
            }
        }
        None
    }

    /// Match "signal ClassName.signal_name(" pattern
    fn match_signal_class_member(content: &str) -> Option<(String, String)> {
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("signal ") {
                if let Some((class_part, signal_part)) = rest.split_once('.') {
                    let class_name: String = class_part
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let signal_name: String = signal_part
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();

                    if !class_name.is_empty() && !signal_name.is_empty() {
                        return Some((class_name, signal_name));
                    }
                }
            }
        }
        None
    }
}
