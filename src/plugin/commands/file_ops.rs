//! File operations: :w, :wa, :q, :qa, :e, :e!, ZZ, ZQ
//! Also handles forwarding Ex commands to Neovim

use super::super::{EditorType, GodotNeovimPlugin};
use super::{simulate_ctrl_s, simulate_ctrl_shift_alt_s, simulate_ctrl_w};
use godot::classes::{EditorInterface, MenuButton, ResourceSaver};
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Forward an Ex command to Neovim for execution
    /// Used for commands that need Neovim's undo/redo integration:
    /// - :s/old/new/g, :%s/old/new/g (substitute)
    /// - :g/pattern/cmd (global)
    /// - :sort
    /// - :t (copy line)
    /// - :m (move line)
    /// - Line range commands (e.g., :1,5d)
    pub(in crate::plugin) fn cmd_forward_to_neovim(&mut self, cmd: &str) {
        let neovim_ref = match self.current_editor_type {
            EditorType::Shader => self.shader_neovim.as_ref(),
            _ => self.script_neovim.as_ref(),
        };
        let Some(neovim) = neovim_ref else {
            godot_warn!("[godot-neovim] Cannot forward command: Neovim not connected");
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            godot_warn!("[godot-neovim] Cannot forward command: Failed to lock Neovim");
            return;
        };

        // Execute the command in Neovim
        let full_cmd = format!(":{}", cmd);
        crate::verbose_print!("[godot-neovim] Forwarding to Neovim: {}", full_cmd);

        // For :set commands with ?, get the option value directly via Neovim API
        if cmd.starts_with("set ") && cmd.contains('?') {
            // Extract option name from "set optname?" format
            let option_name = cmd
                .strip_prefix("set ")
                .and_then(|s| s.strip_suffix('?'))
                .unwrap_or("");

            if !option_name.is_empty() {
                // Use nvim_get_option_value API for reliable option retrieval
                // Use buf=0 to get the current buffer's option (for buffer-local options like filetype)
                let lua_cmd = format!(
                    "return vim.api.nvim_get_option_value('{}', {{ buf = 0 }})",
                    option_name
                );
                crate::verbose_print!("[godot-neovim] Querying option: {}", lua_cmd);
                match client.execute_lua_with_result(&lua_cmd) {
                    Ok(result) => {
                        let value_str = if result.is_str() {
                            result.as_str().unwrap_or("").to_string()
                        } else if result.is_bool() {
                            result.as_bool().map_or("".to_string(), |b| b.to_string())
                        } else if result.is_i64() {
                            result.as_i64().map_or("".to_string(), |n| n.to_string())
                        } else {
                            format!("{:?}", result)
                        };
                        crate::verbose_print!("[godot-neovim] Option result: {:?}", result);
                        godot_print!("[godot-neovim] {}={}", option_name, value_str);
                    }
                    Err(e) => {
                        godot_warn!(
                            "[godot-neovim] Failed to get option '{}': {}",
                            option_name,
                            e
                        );
                    }
                }
            }
        } else if let Err(e) = client.command(&full_cmd) {
            godot_warn!("[godot-neovim] Neovim command failed: {}", e);
        }
    }

    /// :e[dit] {file} - Open a file in the script editor
    /// If no file is specified, opens the quick open dialog
    pub(in crate::plugin) fn cmd_edit(&self, file_path: &str) {
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

    /// :w - Save the current file using Ctrl+S simulation
    /// This triggers Godot's internal save processing, including EditorPlugin hooks
    pub(in crate::plugin) fn cmd_save(&self) {
        let Some(ref _editor) = self.current_editor else {
            crate::verbose_print!("[godot-neovim] :w - No current editor");
            return;
        };

        simulate_ctrl_s();
        crate::verbose_print!("[godot-neovim] :w - Ctrl+S simulated");
    }

    /// :wa/:wall - Save all open scripts using Ctrl+Shift+Alt+S simulation
    /// This triggers Godot's internal save_all processing, including EditorPlugin hooks
    pub(in crate::plugin) fn cmd_save_all(&self) {
        simulate_ctrl_shift_alt_s();
        crate::verbose_print!("[godot-neovim] :wa - Ctrl+Shift+Alt+S simulated");
    }

    /// :e!/:edit! - Reload current file from disk (discard changes)
    /// Uses Neovim Master design: call Lua reload_buffer to reload and re-attach
    pub(in crate::plugin) fn cmd_reload(&mut self) {
        let neovim_ref = match self.current_editor_type {
            EditorType::Shader => self.shader_neovim.as_ref(),
            _ => self.script_neovim.as_ref(),
        };
        let Some(neovim) = neovim_ref else {
            godot_warn!("[godot-neovim] :e! - Neovim not connected");
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            godot_warn!("[godot-neovim] :e! - Failed to lock Neovim");
            return;
        };

        // Call Lua reload_buffer function which:
        // 1. Executes :e! to reload from disk
        // 2. Re-attaches for notifications
        // 3. Returns the new buffer content
        match client.execute_lua_with_result("return _G.godot_neovim.reload_buffer()") {
            Ok(result) => {
                // Parse result: { lines = [...], tick = number, attached = bool, cursor = {row, col} }
                if let rmpv::Value::Map(map) = result {
                    let mut lines: Vec<String> = Vec::new();
                    let mut tick: i64 = 0;
                    let mut cursor: Option<(i64, i64)> = None;

                    for (key, value) in map {
                        if let rmpv::Value::String(k) = key {
                            match k.as_str() {
                                Some("lines") => {
                                    if let rmpv::Value::Array(arr) = value {
                                        lines = arr
                                            .into_iter()
                                            .filter_map(|v| {
                                                if let rmpv::Value::String(s) = v {
                                                    s.into_str()
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect();
                                    }
                                }
                                Some("tick") => {
                                    if let rmpv::Value::Integer(i) = value {
                                        tick = i.as_i64().unwrap_or(0);
                                    }
                                }
                                Some("cursor") => {
                                    // cursor is {row, col} - row is 1-indexed from Neovim
                                    if let rmpv::Value::Array(arr) = value {
                                        if arr.len() >= 2 {
                                            let row = arr[0].as_i64().unwrap_or(1);
                                            let col = arr[1].as_i64().unwrap_or(0);
                                            cursor = Some((row, col));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    // Apply the reloaded content from Neovim to Godot CodeEdit
                    if let Some(ref mut code_edit) = self.current_editor {
                        let line_count = lines.len() as i32;
                        let text = lines.join("\n");
                        code_edit.set_text(&text);
                        code_edit.tag_saved_version();

                        // Apply cursor position (convert from 1-indexed to 0-indexed line)
                        if let Some((row, col)) = cursor {
                            let line = (row - 1).max(0) as i32;
                            // Convert byte column from Neovim to character column for Godot
                            let line_text = code_edit.get_line(line).to_string();
                            let char_col = Self::byte_col_to_char_col(&line_text, col as i32);
                            code_edit.set_caret_line(line);
                            code_edit.set_caret_column(char_col);
                            crate::verbose_print!(
                                "[godot-neovim] :e! - Set cursor to line={}, col={} (byte_col={})",
                                line,
                                char_col,
                                col
                            );
                        }

                        // Update sync manager with new tick and line count
                        self.sync_manager.set_initial_sync_tick(tick);
                        self.sync_manager.set_line_count(line_count);

                        crate::verbose_print!(
                            "[godot-neovim] :e! - Reloaded {} lines, tick={}",
                            line_count,
                            tick
                        );
                    }
                    // Note: (*) marker may still show until tab switch
                    // This is a Godot limitation - set_text() marks as modified
                }
            }
            Err(e) => {
                godot_warn!("[godot-neovim] :e! - Lua call failed: {}", e);
            }
        }
    }

    /// ZZ/:wq - Save and close (sync CodeEdit content to Script, then save)
    pub(in crate::plugin) fn cmd_save_and_close(&mut self) {
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
    pub(in crate::plugin) fn cmd_close(&mut self) {
        // Disconnect from signals BEFORE closing to avoid
        // accessing freed CodeEdit instance
        self.disconnect_caret_changed_signal();
        self.disconnect_resized_signal();

        // Sync cursor to Neovim BEFORE closing, because on_script_changed
        // is called after the editor is freed and we can't read cursor then
        // First gather cursor data from editor
        let cursor_data = if let Some(ref editor) = self.current_editor {
            if editor.is_instance_valid() {
                let line = editor.get_caret_line() as i64 + 1; // 1-indexed for Neovim
                let char_col = editor.get_caret_column();
                let line_text = editor.get_line(editor.get_caret_line()).to_string();
                let byte_col = Self::char_col_to_byte_col(&line_text, char_col) as i64;
                Some((line, byte_col, char_col))
            } else {
                None
            }
        } else {
            None
        };

        // Now sync to Neovim
        if let Some((line, byte_col, char_col)) = cursor_data {
            let neovim_ref = match self.current_editor_type {
                EditorType::Shader => self.shader_neovim.as_ref(),
                _ => self.script_neovim.as_ref(),
            };
            if let Some(neovim) = neovim_ref {
                if let Ok(client) = neovim.try_lock() {
                    let _ = client.set_cursor(line, byte_col);
                    // Set flag to skip cursor sync in on_script_changed
                    self.cursor_synced_before_close = true;
                    crate::verbose_print!(
                        "[godot-neovim] :q - Synced cursor to Neovim before close: ({}, {}) (char_col={})",
                        line,
                        byte_col,
                        char_col
                    );
                }
            }
        }

        // Handle ShaderEditor differently - close via TabContainer
        if self.current_editor_type == EditorType::Shader {
            self.close_shader_tab();
            return;
        }

        // Don't clear current_editor here - if user cancels the save dialog,
        // the script stays open and we need to keep the reference.
        // When the script actually closes, on_script_changed will handle cleanup.

        simulate_ctrl_w();
        crate::verbose_print!("[godot-neovim] :q - Close triggered (Ctrl+W)");
    }

    /// Close the current shader tab using Ctrl+W (same as ScriptEditor)
    /// ShaderEditor also responds to Ctrl+W when it has focus
    fn close_shader_tab(&mut self) {
        // Disconnect from signals BEFORE clearing editor reference
        // to avoid accessing freed CodeEdit instance
        self.disconnect_caret_changed_signal();
        self.disconnect_resized_signal();
        self.disconnect_gui_input_signal();

        // Delete shader buffer from Neovim before closing
        if !self.current_script_path.is_empty() {
            // Convert res:// path to absolute path for Neovim
            let abs_path = if self.current_script_path.starts_with("res://") {
                godot::classes::ProjectSettings::singleton()
                    .globalize_path(&self.current_script_path)
                    .to_string()
            } else {
                self.current_script_path.clone()
            };
            self.delete_neovim_buffer(&abs_path, EditorType::Shader);
        }

        // Clear current editor after disconnecting signals
        self.current_editor = None;
        self.current_editor_type = EditorType::Unknown;

        // Use Ctrl+W to close the shader tab - Godot's ShaderEditor handles this
        simulate_ctrl_w();

        // Set flag to grab focus on shader editor after close
        // This ensures we focus the remaining shader tab's CodeEdit
        self.focus_shader_after_close = true;

        crate::verbose_print!("[godot-neovim] :q - Shader tab close triggered (Ctrl+W)");
    }

    /// ZQ - Close without saving (discard changes)
    pub(in crate::plugin) fn cmd_close_discard(&mut self) {
        // Disconnect from signals BEFORE closing
        self.disconnect_caret_changed_signal();
        self.disconnect_resized_signal();

        // Reload from disk using Lua function (same as :e!)
        // This ensures we get the actual disk content, not stale Script data
        // First get the text from Neovim, then release lock before modifying editor
        let restored_text: Option<String> = {
            let neovim_ref = match self.current_editor_type {
                EditorType::Shader => self.shader_neovim.as_ref(),
                _ => self.script_neovim.as_ref(),
            };
            if let Some(neovim) = neovim_ref {
                if let Ok(client) = neovim.try_lock() {
                    match client.execute_lua_with_result("return _G.godot_neovim.reload_buffer()") {
                        Ok(result) => {
                            if let rmpv::Value::Map(map) = result {
                                let mut lines: Vec<String> = Vec::new();

                                for (key, value) in map {
                                    if let rmpv::Value::String(k) = key {
                                        if k.as_str() == Some("lines") {
                                            if let rmpv::Value::Array(arr) = value {
                                                lines = arr
                                                    .into_iter()
                                                    .filter_map(|v| {
                                                        if let rmpv::Value::String(s) = v {
                                                            s.into_str()
                                                        } else {
                                                            None
                                                        }
                                                    })
                                                    .collect();
                                            }
                                        }
                                    }
                                }
                                Some(lines.join("\n"))
                            } else {
                                None
                            }
                        }
                        Err(e) => {
                            godot_warn!("[godot-neovim] ZQ - Failed to reload from disk: {}", e);
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        // Now apply restored text to editor (lock is released)
        if let Some(text) = restored_text {
            let line_count = text.lines().count();

            // Apply disk content to CodeEdit before closing
            if let Some(ref mut code_edit) = self.current_editor {
                code_edit.set_text(&text);
                code_edit.tag_saved_version();
                crate::verbose_print!(
                    "[godot-neovim] ZQ - Restored {} lines to CodeEdit",
                    line_count
                );
            }

            // Also update the Script resource to prevent Godot from
            // caching the modified content when reopening
            let editor = EditorInterface::singleton();
            if let Some(mut script_editor) = editor.get_script_editor() {
                if let Some(mut current_script) = script_editor.get_current_script() {
                    current_script.set_source_code(&text);
                    crate::verbose_print!(
                        "[godot-neovim] ZQ - Restored {} lines to Script",
                        line_count
                    );
                }
            }
        }

        // Now close the tab (should not prompt since changes are discarded)
        self.current_editor = None;

        simulate_ctrl_w();
        crate::verbose_print!("[godot-neovim] ZQ - Close triggered (discard changes)");
    }

    /// :qa/:qall - Close all tabs in the current editor type
    /// - ShaderEditor: Close all shader tabs only
    /// - ScriptEditor: Close all script tabs only
    ///
    /// Note: Neovim buffer deletion is handled by on_script_close signal for scripts
    pub(in crate::plugin) fn cmd_close_all(&mut self) {
        // Determine which editor type we're in
        let is_shader_editor = self.current_editor_type == EditorType::Shader;

        // Disconnect from signals BEFORE closing
        self.disconnect_caret_changed_signal();
        self.disconnect_resized_signal();

        // Clear current editor reference since it will be freed
        self.current_editor = None;
        self.current_editor_type = EditorType::Unknown;

        // If in ShaderEditor, close only shader tabs
        if is_shader_editor {
            self.close_all_shader_tabs();
            return;
        }

        // Set flag to skip on_script_changed processing during close all
        // Will be reset by process() when operation completes
        self.closing_all_tabs = true;

        // Close all script tabs
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

    /// Close all shader tabs in ShaderEditor
    /// Called from cmd_close_all when in ShaderEditor
    fn close_all_shader_tabs(&mut self) {
        // Find ShaderEditor's TabContainer and get shader paths for buffer cleanup
        let editor = EditorInterface::singleton();
        let Some(base_control) = editor.get_base_control() else {
            return;
        };

        // Find EditorNode
        let Some(editor_node) = self.find_editor_node_from_control(&base_control) else {
            return;
        };

        // Find ShaderEditor's TabContainer and collect shader paths
        let shader_paths = self.collect_shader_paths(&editor_node);

        if shader_paths.is_empty() {
            crate::verbose_print!("[godot-neovim] :qa - No shader tabs to close");
            return;
        }

        let tab_count = shader_paths.len();
        crate::verbose_print!("[godot-neovim] :qa - Closing {} shader tab(s)", tab_count);

        // Delete shader buffers from Neovim (use Shader type for shader_neovim)
        for path in &shader_paths {
            self.delete_neovim_buffer(path, EditorType::Shader);
        }

        // Close shader tabs using file_menu's "Close" (id=8) repeatedly
        // We can't access context_menu's Close All because it's only populated when shown
        if let Some(mut popup) = self.find_shader_file_menu_popup(&editor_node) {
            // FILE_MENU_CLOSE = 8 in ShaderEditorPlugin (Godot 4.6)
            const SHADER_FILE_MENU_CLOSE: i64 = 8;

            // Queue up close operations for each tab
            for i in 0..tab_count {
                popup.call_deferred(
                    "emit_signal",
                    &[
                        "id_pressed".to_variant(),
                        SHADER_FILE_MENU_CLOSE.to_variant(),
                    ],
                );
                crate::verbose_print!(
                    "[godot-neovim] :qa - queued close for tab {} (id={})",
                    i,
                    SHADER_FILE_MENU_CLOSE
                );
            }
        } else {
            crate::verbose_print!("[godot-neovim] :qa - ShaderEditor file_menu not found");
        }
    }

    /// Find ShaderEditor's file_menu popup (PopupMenu)
    /// The file_menu is a MenuButton with text "File" in ShaderEditor's UI
    fn find_shader_file_menu_popup(
        &self,
        node: &Gd<godot::classes::Node>,
    ) -> Option<Gd<godot::classes::PopupMenu>> {
        let class_name = node.get_class().to_string();

        // Look for HSplitContainer that contains ItemList (this is files_split)
        if class_name == "HSplitContainer" {
            // Check if this is files_split (has ItemList child)
            let has_item_list = (0..node.get_child_count()).any(|i| {
                node.get_child(i)
                    .is_some_and(|c| c.get_class().to_string() == "ItemList")
            });

            if has_item_list {
                // This is files_split, search for file_menu MenuButton within entire subtree
                if let Some(popup) = self.find_file_menu_button(node) {
                    return Some(popup);
                }
            }
        }

        // Recursively search children
        for i in 0..node.get_child_count() {
            if let Some(child) = node.get_child(i) {
                if let Some(found) = self.find_shader_file_menu_popup(&child) {
                    return Some(found);
                }
            }
        }

        None
    }

    /// Find MenuButton with text "File" in a subtree
    fn find_file_menu_button(
        &self,
        node: &Gd<godot::classes::Node>,
    ) -> Option<Gd<godot::classes::PopupMenu>> {
        let class_name = node.get_class().to_string();

        if class_name == "MenuButton" {
            if let Ok(menu_button) = node.clone().try_cast::<MenuButton>() {
                let text = menu_button.get_text().to_string();
                if text == "File" {
                    crate::verbose_print!("[godot-neovim] Found ShaderEditor file_menu");
                    return menu_button.get_popup();
                }
            }
        }

        // Recursively search children
        for i in 0..node.get_child_count() {
            if let Some(child) = node.get_child(i) {
                if let Some(found) = self.find_file_menu_button(&child) {
                    return Some(found);
                }
            }
        }

        None
    }

    /// Find EditorNode from a Control
    fn find_editor_node_from_control(
        &self,
        control: &Gd<godot::classes::Control>,
    ) -> Option<Gd<godot::classes::Node>> {
        let mut current: Gd<godot::classes::Node> = control.clone().upcast();
        loop {
            let class_name = current.get_class().to_string();
            if class_name == "EditorNode" {
                return Some(current);
            }
            current = current.get_parent()?;
        }
    }

    /// Collect all shader paths from ShaderEditor's ItemList
    fn collect_shader_paths(&self, editor_node: &Gd<godot::classes::Node>) -> Vec<String> {
        let mut paths = Vec::new();

        // Search for HSplitContainer containing ItemList with shader paths
        self.find_shader_item_list(editor_node, &mut paths);

        paths
    }

    /// Recursively find ItemList in ShaderEditor and collect shader paths
    fn find_shader_item_list(&self, node: &Gd<godot::classes::Node>, paths: &mut Vec<String>) {
        let class_name = node.get_class().to_string();

        // Look for HSplitContainer (files_split in ShaderEditor)
        if class_name == "HSplitContainer" {
            use godot::classes::ItemList;

            let child_count = node.get_child_count();
            for i in 0..child_count {
                if let Some(child) = node.get_child(i) {
                    if child.get_class().to_string() == "ItemList" {
                        if let Ok(item_list) = child.try_cast::<ItemList>() {
                            // Collect all shader paths from ItemList tooltips
                            let item_count = item_list.get_item_count();
                            for idx in 0..item_count {
                                let tooltip = item_list.get_item_tooltip(idx).to_string();
                                if !tooltip.is_empty()
                                    && (tooltip.ends_with(".gdshader")
                                        || tooltip.ends_with(".shader")
                                        || tooltip.ends_with(".gdshaderinc"))
                                {
                                    // Convert res:// path to absolute path for Neovim
                                    if let Some(abs_path) =
                                        self.convert_res_path_to_absolute(&tooltip)
                                    {
                                        paths.push(abs_path);
                                    }
                                }
                            }
                            return;
                        }
                    }
                }
            }
        }

        // Recursively search children
        let child_count = node.get_child_count();
        for i in 0..child_count {
            if let Some(child) = node.get_child(i) {
                self.find_shader_item_list(&child, paths);
            }
        }
    }

    /// Convert res:// path to absolute path
    fn convert_res_path_to_absolute(&self, res_path: &str) -> Option<String> {
        use godot::classes::ProjectSettings;
        let project_path = ProjectSettings::singleton()
            .globalize_path(res_path)
            .to_string();
        if project_path.is_empty() {
            None
        } else {
            Some(project_path)
        }
    }
}
