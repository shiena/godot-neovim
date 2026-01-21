//! File operations: :w, :wa, :q, :qa, :e, :e!, ZZ, ZQ
//! Also handles forwarding Ex commands to Neovim

use super::super::GodotNeovimPlugin;
use godot::classes::{EditorInterface, Input, InputEventKey, MenuButton, ResourceSaver};
use godot::global::Key;
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
        let Some(ref neovim) = self.neovim else {
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

        if let Err(e) = client.command(&full_cmd) {
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

    /// :w - Save the current file using ScriptEditor's File menu
    /// Uses FILE_MENU_SAVE to ensure cross-platform compatibility (macOS Cmd+S, Windows Ctrl+S)
    pub(in crate::plugin) fn cmd_save(&self) {
        let editor = EditorInterface::singleton();
        let Some(script_editor) = editor.get_script_editor() else {
            crate::verbose_print!("[godot-neovim] :w - Could not find ScriptEditor");
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
                                                // FILE_MENU_SAVE = 5 (from Godot's script_editor_plugin.h)
                                                const FILE_MENU_SAVE: i64 = 5;
                                                popup.call_deferred(
                                                    "emit_signal",
                                                    &[
                                                        "id_pressed".to_variant(),
                                                        FILE_MENU_SAVE.to_variant(),
                                                    ],
                                                );
                                                crate::verbose_print!(
                                                    "[godot-neovim] :w - emit_signal(id_pressed, {})",
                                                    FILE_MENU_SAVE
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

        crate::verbose_print!("[godot-neovim] :w - File menu not found");
    }

    /// :wa/:wall - Save all open scripts
    /// Uses Godot's built-in FILE_MENU_SAVE_ALL to properly update dirty markers
    pub(in crate::plugin) fn cmd_save_all(&self) {
        let editor = EditorInterface::singleton();
        let Some(script_editor) = editor.get_script_editor() else {
            crate::verbose_print!("[godot-neovim] :wa - Could not find ScriptEditor");
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
                                                // FILE_MENU_SAVE_ALL = 7
                                                const FILE_MENU_SAVE_ALL: i64 = 7;
                                                popup.call_deferred(
                                                    "emit_signal",
                                                    &[
                                                        "id_pressed".to_variant(),
                                                        FILE_MENU_SAVE_ALL.to_variant(),
                                                    ],
                                                );
                                                crate::verbose_print!(
                                                    "[godot-neovim] :wa - emit_signal(id_pressed, {})",
                                                    FILE_MENU_SAVE_ALL
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

        crate::verbose_print!("[godot-neovim] :wa - File menu not found");
    }

    /// :e!/:edit! - Reload current file from disk (discard changes)
    /// Uses Neovim Master design: call Lua reload_buffer to reload and re-attach
    pub(in crate::plugin) fn cmd_reload(&mut self) {
        let Some(ref neovim) = self.neovim else {
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
                            let column = col as i32;
                            code_edit.set_caret_line(line);
                            code_edit.set_caret_column(column);
                            crate::verbose_print!(
                                "[godot-neovim] :e! - Set cursor to line={}, col={}",
                                line,
                                column
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
    pub(in crate::plugin) fn cmd_close_discard(&mut self) {
        // Disconnect from signals BEFORE closing
        self.disconnect_caret_changed_signal();
        self.disconnect_resized_signal();

        // Reload from disk using Lua function (same as :e!)
        // This ensures we get the actual disk content, not stale Script data
        if let Some(ref neovim) = self.neovim {
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

                            let text = lines.join("\n");

                            // Apply disk content to CodeEdit before closing
                            if let Some(ref mut code_edit) = self.current_editor {
                                code_edit.set_text(&text);
                                code_edit.tag_saved_version();
                                crate::verbose_print!(
                                    "[godot-neovim] ZQ - Restored {} lines to CodeEdit",
                                    lines.len()
                                );
                            }

                            // Also update the Script resource to prevent Godot from
                            // caching the modified content when reopening
                            let editor = EditorInterface::singleton();
                            if let Some(mut script_editor) = editor.get_script_editor() {
                                if let Some(mut current_script) = script_editor.get_current_script()
                                {
                                    current_script.set_source_code(&text);
                                    crate::verbose_print!(
                                        "[godot-neovim] ZQ - Restored {} lines to Script",
                                        lines.len()
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        godot_warn!("[godot-neovim] ZQ - Failed to reload from disk: {}", e);
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
    pub(in crate::plugin) fn cmd_close_all(&mut self) {
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
}
