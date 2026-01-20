//! File operations: :w, :wa, :q, :qa, :e, :e!, ZZ, ZQ

use super::super::GodotNeovimPlugin;
use godot::classes::{EditorInterface, Input, InputEventKey, MenuButton, ResourceSaver};
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
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

    /// :w - Save the current file by simulating Ctrl+S
    pub(in crate::plugin) fn cmd_save(&self) {
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
    pub(in crate::plugin) fn cmd_reload(&mut self) {
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
