//! Editor management: finding CodeEdit, script change handling

use super::{EditorType, GodotNeovimPlugin};
use godot::classes::{CodeEdit, Control, EditorInterface, Resource, Window};
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Check if current CodeEdit is in a float window and connect to gui_input signal
    pub(super) fn update_float_window_connection(&mut self) {
        let Some(ref editor) = self.current_editor else {
            crate::verbose_print!("[godot-neovim] update_float_window: no current_editor");
            return;
        };

        // Get the window containing the CodeEdit
        let Some(window) = editor.get_window() else {
            crate::verbose_print!("[godot-neovim] update_float_window: editor has no window");
            return;
        };

        // Check if this is a float window (not the main window)
        // Float windows are children of EditorNode, main window is the root window
        let main_window = EditorInterface::singleton()
            .get_base_control()
            .and_then(|c| c.get_window());

        let is_float_window = match main_window {
            Some(ref main) => {
                let is_float = window.instance_id() != main.instance_id();
                crate::verbose_print!(
                    "[godot-neovim] update_float_window: editor_window={} (id={}), main_window={} (id={}), is_float={}",
                    window.get_name(),
                    window.instance_id(),
                    main.get_name(),
                    main.instance_id(),
                    is_float
                );
                is_float
            }
            None => {
                crate::verbose_print!("[godot-neovim] update_float_window: no main window found");
                false
            }
        };

        // Always connect gui_input signal - it will check is_in_float_window() internally
        // This ensures we catch input in float windows
        self.connect_gui_input_signal();

        if is_float_window {
            crate::verbose_print!(
                "[godot-neovim] CodeEdit is in float window: {}",
                window.get_name()
            );
        }
    }

    /// Delete a buffer from Neovim by path
    pub(super) fn delete_neovim_buffer(&self, path: &str) {
        // Use script_neovim for ScriptEditor paths (on_script_close callback)
        // TODO: Consider using path extension to determine which Neovim instance
        let Some(neovim) = self.script_neovim.as_ref() else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Use bwipeout to completely remove buffer (including undo history)
        // This matches vscode-neovim's behavior with force=true
        let cmd = format!("silent! bwipeout! {}", path);
        if let Err(e) = client.command(&cmd) {
            crate::verbose_print!("[godot-neovim] Failed to delete buffer {}: {}", path, e);
        } else {
            crate::verbose_print!("[godot-neovim] Deleted buffer from Neovim: {}", path);
        }
    }

    /// Trigger script change handling via deferred call
    pub(super) fn handle_script_changed(&mut self) {
        // Increment switch ID and store as pending
        // This allows detecting and skipping stale deferred operations
        // when rapid tab switching occurs (ref: vscode-neovim commit 0520846)
        self.script_switch_id = self.script_switch_id.wrapping_add(1);
        self.pending_switch_id = self.script_switch_id;

        // Use call_deferred to ensure Godot has fully switched to the new script
        // before we try to find the CodeEdit and sync buffer
        self.base_mut()
            .call_deferred("handle_script_changed_deferred", &[]);
    }

    /// Reposition mode label to current editor's status bar
    /// Each editor type (Script, Shader) has its own independent label
    pub(super) fn reposition_mode_label(&mut self) {
        // Get the appropriate label based on current editor type
        let (label_ref, label_field_is_shader) = match self.current_editor_type {
            EditorType::Shader => (&self.shader_mode_label, true),
            _ => (&self.mode_label, false),
        };

        // Check if label is still valid (may have been freed with previous status bar)
        let label_valid = label_ref
            .as_ref()
            .is_some_and(|label| label.is_instance_valid());

        if !label_valid {
            // Label was freed, clear and create a new one
            if label_field_is_shader {
                self.shader_mode_label = None;
            } else {
                self.mode_label = None;
            }
            self.create_mode_label();
            return;
        }

        // Get the label again after potential creation
        let label = if label_field_is_shader {
            self.shader_mode_label.as_ref()
        } else {
            self.mode_label.as_ref()
        };

        let Some(label) = label else {
            return;
        };

        // Check if label needs to be moved to status bar
        if let Some(code_edit) = &self.current_editor {
            // Verify the CodeEdit is still valid (may have been freed when script closed)
            if !code_edit.is_instance_valid() {
                self.current_editor = None;
                return;
            }
            if let Some(mut status_bar) = self.find_status_bar(code_edit.clone().upcast()) {
                // Check if already in this status bar
                if let Some(parent) = label.get_parent() {
                    if parent.instance_id() == status_bar.instance_id() {
                        return; // Already in correct position
                    }

                    // Check if windows are different (float/undock case)
                    // Moving nodes between windows causes Windows display server errors
                    let label_window = label.get_window();
                    let status_bar_window = status_bar.get_window();
                    let windows_differ = match (&label_window, &status_bar_window) {
                        (Some(lw), Some(sw)) => lw.instance_id() != sw.instance_id(),
                        _ => false,
                    };

                    if windows_differ {
                        // Different windows - recreate label instead of moving
                        // This avoids transient_parent errors on Windows
                        crate::verbose_print!(
                            "[godot-neovim] Mode label in different window, recreating"
                        );
                        if label_field_is_shader {
                            if let Some(mut old_label) = self.shader_mode_label.take() {
                                old_label.queue_free();
                            }
                        } else if let Some(mut old_label) = self.mode_label.take() {
                            old_label.queue_free();
                        }
                        self.create_mode_label();
                        return;
                    }

                    // Same window - just move the label
                    parent.clone().remove_child(label);
                }

                // Add to status bar
                status_bar.add_child(label);
                status_bar.move_child(label, 0);
                crate::verbose_print!("[godot-neovim] Mode label moved to status bar");
            }
        }
    }

    /// Find and set current CodeEdit reference
    pub(super) fn find_current_code_edit(&mut self) {
        // Clear the reference first to avoid use-after-free when script is closed
        self.current_editor = None;
        self.current_editor_type = EditorType::Unknown;

        let editor = EditorInterface::singleton();

        // First, try to get the focused CodeEdit directly via gui_get_focus_owner
        // This works for both docked and floating windows
        if let Some(code_edit) = self.get_focused_code_edit_direct() {
            // Check if this CodeEdit is in ShaderEditor
            if self.is_code_edit_in_shader_editor(&code_edit) {
                // ShaderEditor detected - enable Neovim integration for shaders
                crate::verbose_print!(
                    "[godot-neovim] Found focused CodeEdit in ShaderEditor - enabling Neovim integration"
                );
                self.current_editor = Some(code_edit.clone());
                self.current_editor_type = EditorType::Shader;

                // Try to get shader path from the editor hierarchy
                if let Some(path) = self.get_shader_path_from_code_edit(&code_edit) {
                    crate::verbose_print!("[godot-neovim] Shader path: {}", path);
                    self.current_script_path = path;
                }
            } else {
                crate::verbose_print!("[godot-neovim] Found focused CodeEdit (direct)");
                self.current_editor = Some(code_edit);
                self.current_editor_type = EditorType::Script;
            }
        }

        // If not found via direct focus, try ScriptEditor
        if self.current_editor.is_none() && self.current_editor_type != EditorType::Shader {
            if let Some(script_editor) = editor.get_script_editor() {
                // Try to find the currently focused CodeEdit by traversing ScriptEditor
                if let Some(code_edit) =
                    self.find_focused_code_edit(script_editor.clone().upcast::<Control>())
                {
                    crate::verbose_print!("[godot-neovim] Found focused CodeEdit in ScriptEditor");
                    self.current_editor = Some(code_edit);
                    self.current_editor_type = EditorType::Script;
                } else if let Some(code_edit) =
                    self.find_visible_code_edit_safe(script_editor.upcast::<Control>())
                {
                    // Fallback: find visible CodeEdit, but verify it's the active one
                    crate::verbose_print!(
                        "[godot-neovim] Found visible CodeEdit in ScriptEditor (safe fallback)"
                    );
                    self.current_editor = Some(code_edit);
                    self.current_editor_type = EditorType::Script;
                }
            }
        }

        // Connect signals and reset state if editor was found
        if self.current_editor.is_some() {
            self.connect_caret_changed_signal();
            self.connect_resized_signal();
            self.update_float_window_connection();

            // Clear any restored selection and disable selecting
            // Godot may restore previous selection state when reopening files
            if let Some(ref mut ed) = self.current_editor {
                ed.deselect();
                ed.set_selecting_enabled(false);
            }
        }
    }

    /// Check if a ShaderEditor is currently focused (even if not syncing)
    pub(super) fn is_shader_editor_focused(&self) -> bool {
        self.current_editor_type == EditorType::Shader
    }

    /// Try to get focused CodeEdit directly from any window's viewport
    /// This works for floating windows where the CodeEdit is not a child of ScriptEditor
    fn get_focused_code_edit_direct(&self) -> Option<Gd<CodeEdit>> {
        let editor = EditorInterface::singleton();
        let base_control = editor.get_base_control()?;

        // First try the main viewport
        let main_viewport = base_control.get_viewport()?;
        if let Some(focus_owner) = main_viewport.gui_get_focus_owner() {
            if let Ok(code_edit) = focus_owner.try_cast::<CodeEdit>() {
                return Some(code_edit);
            }
        }

        // Float windows are managed by EditorNode, not as children of root window.
        // We need to find EditorNode by traversing up from base_control
        // and then search its children for WindowWrapper/Window nodes.
        let editor_node = self.find_editor_node(base_control.clone().upcast())?;
        crate::verbose_print!(
            "[godot-neovim] Found EditorNode: {} (children={})",
            editor_node.get_name(),
            editor_node.get_child_count()
        );
        self.find_focused_code_edit_in_subwindows(editor_node)
    }

    /// Find EditorNode by traversing up the node hierarchy
    fn find_editor_node(&self, node: Gd<Node>) -> Option<Gd<Node>> {
        let mut current = node;
        loop {
            let class_name = current.get_class().to_string();
            if class_name == "EditorNode" {
                return Some(current);
            }
            current = current.get_parent()?;
        }
    }

    /// Recursively search for focused CodeEdit in all subwindows
    fn find_focused_code_edit_in_subwindows(&self, node: Gd<Node>) -> Option<Gd<CodeEdit>> {
        // If this is a Window, check its focus owner or search for CodeEdit directly
        if let Ok(window) = node.clone().try_cast::<Window>() {
            let window_has_focus = window.has_focus();

            // First try gui_get_focus_owner
            if let Some(focus_owner) = window.gui_get_focus_owner() {
                if let Ok(code_edit) = focus_owner.try_cast::<CodeEdit>() {
                    return Some(code_edit);
                }
            }

            // If window has focus (OS-level), search for any CodeEdit in this window
            // Float windows won't report gui_get_focus_owner correctly when called from main window
            if window_has_focus {
                if let Some(code_edit) = self.find_first_code_edit_in_node(window.upcast()) {
                    return Some(code_edit);
                }
            }
        }

        // Search children for more windows
        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Some(code_edit) = self.find_focused_code_edit_in_subwindows(child) {
                    return Some(code_edit);
                }
            }
        }

        None
    }

    /// Recursively find the first CodeEdit in a node tree (for focused windows)
    fn find_first_code_edit_in_node(&self, node: Gd<Node>) -> Option<Gd<CodeEdit>> {
        // Check if this node is a CodeEdit
        if let Ok(code_edit) = node.clone().try_cast::<CodeEdit>() {
            return Some(code_edit);
        }

        // Search children
        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Some(code_edit) = self.find_first_code_edit_in_node(child) {
                    return Some(code_edit);
                }
            }
        }

        None
    }

    /// Recursively find focused CodeEdit
    pub(super) fn find_focused_code_edit(&self, node: Gd<Control>) -> Option<Gd<CodeEdit>> {
        // Check if this node is a focused CodeEdit
        if let Ok(code_edit) = node.clone().try_cast::<CodeEdit>() {
            if code_edit.has_focus() {
                return Some(code_edit);
            }
        }

        // Search children
        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Ok(control) = child.try_cast::<Control>() {
                    if let Some(code_edit) = self.find_focused_code_edit(control) {
                        return Some(code_edit);
                    }
                }
            }
        }

        None
    }

    /// Recursively find visible CodeEdit (legacy - use find_visible_code_edit_safe instead)
    pub(super) fn find_visible_code_edit(&self, node: Gd<Control>) -> Option<Gd<CodeEdit>> {
        // Check if this node is a visible CodeEdit
        if let Ok(code_edit) = node.clone().try_cast::<CodeEdit>() {
            if code_edit.is_visible_in_tree() {
                return Some(code_edit);
            }
        }

        // Search children
        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Ok(control) = child.try_cast::<Control>() {
                    if let Some(code_edit) = self.find_visible_code_edit(control) {
                        return Some(code_edit);
                    }
                }
            }
        }

        None
    }

    /// Safely find visible CodeEdit within ScriptEditor only
    /// This version verifies the CodeEdit matches the current script to avoid
    /// returning the wrong editor when multiple editors are open (issue #40)
    pub(super) fn find_visible_code_edit_safe(&self, node: Gd<Control>) -> Option<Gd<CodeEdit>> {
        let editor = EditorInterface::singleton();
        let mut script_editor = editor.get_script_editor()?;
        let current_script = script_editor.get_current_script()?;
        let expected_path = current_script.get_path().to_string();

        if expected_path.is_empty() {
            return None;
        }

        // Find the CodeEdit
        let code_edit = self.find_visible_code_edit(node)?;

        // Verify: the CodeEdit should be in a visible tab that corresponds to current script
        // Get the first line and compare with script content
        let editor_first_line = if code_edit.get_line_count() > 0 {
            code_edit.get_line(0).to_string()
        } else {
            String::new()
        };

        let script_source = current_script.get_source_code().to_string();
        let script_first_line = script_source.lines().next().unwrap_or("");

        // If content matches, this is likely the correct CodeEdit
        if editor_first_line.trim() == script_first_line.trim() {
            crate::verbose_print!(
                "[godot-neovim] find_visible_code_edit_safe: content matches for '{}'",
                expected_path
            );
            Some(code_edit)
        } else {
            crate::verbose_print!(
                "[godot-neovim] find_visible_code_edit_safe: content mismatch, skipping"
            );
            None
        }
    }

    /// Check if a CodeEdit is inside ShaderEditor hierarchy
    /// Returns true if the CodeEdit's ancestor contains "ShaderEditor" or "TextShaderEditor"
    fn is_code_edit_in_shader_editor(&self, code_edit: &Gd<CodeEdit>) -> bool {
        let mut current: Option<Gd<godot::classes::Node>> = code_edit.get_parent();

        while let Some(node) = current {
            let class_name = node.get_class().to_string();
            // Check for shader-related class names
            // TextShaderEditor, ShaderTextEditor, ShaderEditor
            if class_name.contains("Shader") {
                crate::verbose_print!(
                    "[godot-neovim] CodeEdit is in shader hierarchy: {}",
                    class_name
                );
                return true;
            }
            current = node.get_parent();
        }
        false
    }

    /// Get shader resource path from CodeEdit by traversing parent hierarchy
    /// Returns the shader file path (res://...) if found
    fn get_shader_path_from_code_edit(&self, code_edit: &Gd<CodeEdit>) -> Option<String> {
        let mut current: Option<Gd<godot::classes::Node>> = code_edit.get_parent();
        let mut depth = 0;

        while let Some(mut node) = current {
            let class_name = node.get_class().to_string();
            let node_name = node.get_name().to_string();
            crate::verbose_print!(
                "[godot-neovim] Shader path search depth {}: {} ({})",
                depth,
                node_name,
                class_name
            );

            // Check for HSplitContainer - this contains both shader_list (ItemList) and shader_tabs
            // The shader path is stored in the ItemList's tooltip
            if class_name == "HSplitContainer" {
                use godot::classes::ItemList;

                let child_count = node.get_child_count();
                for i in 0..child_count {
                    if let Some(child) = node.get_child(i) {
                        let child_class = child.get_class().to_string();
                        crate::verbose_print!(
                            "[godot-neovim]   HSplitContainer child {}: {} ({})",
                            i,
                            child.get_name(),
                            child_class
                        );

                        // Look for ItemList (shader_list)
                        if child_class == "ItemList" {
                            if let Ok(mut item_list) = child.try_cast::<ItemList>() {
                                // Get the selected item index
                                let selected_items = item_list.get_selected_items();
                                crate::verbose_print!(
                                    "[godot-neovim]   ItemList selected items: {:?}",
                                    selected_items.to_vec()
                                );

                                if !selected_items.is_empty() {
                                    if let Some(selected_idx) = selected_items.get(0) {
                                        // The tooltip contains the full shader path
                                        let tooltip =
                                            item_list.get_item_tooltip(selected_idx).to_string();
                                        crate::verbose_print!(
                                            "[godot-neovim]   ItemList item {} tooltip: '{}'",
                                            selected_idx,
                                            tooltip
                                        );

                                        if !tooltip.is_empty()
                                            && (tooltip.ends_with(".gdshader")
                                                || tooltip.ends_with(".shader")
                                                || tooltip.ends_with(".gdshaderinc"))
                                        {
                                            crate::verbose_print!(
                                                "[godot-neovim] Found shader path from ItemList: {}",
                                                tooltip
                                            );
                                            return Some(tooltip);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check for shader-related classes that might have get_shader() or similar method
            // TextShaderEditor, ShaderTextEditor, or any class containing "Shader"
            if class_name.contains("Shader") {
                // Try multiple method names (Godot versions may differ)
                let method_names = ["get_shader", "get_edited_shader", "get_current_shader"];

                for method_name in method_names {
                    if node.has_method(method_name) {
                        crate::verbose_print!(
                            "[godot-neovim]   {} has method '{}'",
                            class_name,
                            method_name
                        );
                        let result = node.call(method_name, &[]);
                        crate::verbose_print!(
                            "[godot-neovim]   {}() result type: {:?}",
                            method_name,
                            result.get_type()
                        );
                        if let Ok(shader) = result.try_to::<Gd<Resource>>() {
                            let path = shader.get_path().to_string();
                            crate::verbose_print!(
                                "[godot-neovim]   shader.get_path() = '{}'",
                                path
                            );
                            if !path.is_empty() {
                                crate::verbose_print!(
                                    "[godot-neovim] Found shader path via {}.{}(): {}",
                                    class_name,
                                    method_name,
                                    path
                                );
                                return Some(path);
                            }
                        }
                    }
                }

                // Try get_shader_include() / get_edited_shader_include() for .gdshaderinc files
                let include_methods = ["get_shader_include", "get_edited_shader_include"];
                for method_name in include_methods {
                    if node.has_method(method_name) {
                        let result_inc = node.call(method_name, &[]);
                        if let Ok(shader_inc) = result_inc.try_to::<Gd<Resource>>() {
                            let path = shader_inc.get_path().to_string();
                            if !path.is_empty() {
                                crate::verbose_print!(
                                    "[godot-neovim] Found shader include path via {}.{}(): {}",
                                    class_name,
                                    method_name,
                                    path
                                );
                                return Some(path);
                            }
                        }
                    }
                }
            }

            current = node.get_parent();
            depth += 1;
        }
        crate::verbose_print!(
            "[godot-neovim] Shader path not found in hierarchy (searched {} levels)",
            depth
        );
        None
    }

    /// Check if editor has focus, updating current_editor if a different CodeEdit has focus
    pub(super) fn editor_has_focus(&mut self) -> bool {
        // First check if current_editor has focus
        if let Some(ref editor) = self.current_editor {
            if editor.is_instance_valid() && editor.has_focus() {
                // Update float window connection if needed (checks internally if already connected)
                self.update_float_window_connection();
                return true;
            }
            crate::verbose_print!(
                "[godot-neovim] current_editor exists but no focus (valid={}, has_focus={})",
                editor.is_instance_valid(),
                if editor.is_instance_valid() {
                    editor.has_focus()
                } else {
                    false
                }
            );
        }

        // Current editor doesn't have focus - check if a different CodeEdit has focus
        // This handles the case when the editor is floated to a different window
        crate::verbose_print!("[godot-neovim] Searching for focused CodeEdit in all windows...");
        if let Some(focused_code_edit) = self.get_focused_code_edit_direct() {
            // Check if this CodeEdit is in ShaderEditor
            if self.is_code_edit_in_shader_editor(&focused_code_edit) {
                // ShaderEditor has focus - enable Neovim integration for shaders
                let previous_path = self.current_script_path.clone();
                let previous_type = self.current_editor_type;
                self.current_editor = Some(focused_code_edit.clone());
                self.current_editor_type = EditorType::Shader;

                // Try to get shader path
                let mut needs_buffer_sync = previous_type != EditorType::Shader;
                if let Some(path) = self.get_shader_path_from_code_edit(&focused_code_edit) {
                    crate::verbose_print!("[godot-neovim] ShaderEditor has focus, path: {}", path);
                    if previous_path != path {
                        needs_buffer_sync = true;
                    }
                    self.current_script_path = path;
                } else {
                    crate::verbose_print!("[godot-neovim] ShaderEditor has focus, path not found");
                }

                // Trigger buffer sync if editor type changed or path changed
                if needs_buffer_sync {
                    crate::verbose_print!(
                        "[godot-neovim] ShaderEditor: triggering buffer sync (type_changed={}, path_changed={})",
                        previous_type != EditorType::Shader,
                        previous_path != self.current_script_path
                    );
                    self.handle_script_changed();
                }

                self.connect_caret_changed_signal();
                self.connect_resized_signal();
                self.update_float_window_connection();
                return true;
            }

            // Check if this is a different CodeEdit
            let is_different = match &self.current_editor {
                Some(current) => current.instance_id() != focused_code_edit.instance_id(),
                None => true,
            };

            if is_different {
                crate::verbose_print!(
                    "[godot-neovim] Switching to focused CodeEdit (float/dock change)"
                );
                self.current_editor = Some(focused_code_edit);
                self.current_editor_type = EditorType::Script;
                self.connect_caret_changed_signal();
                self.connect_resized_signal();
                self.reposition_mode_label();
            } else {
                crate::verbose_print!("[godot-neovim] Same CodeEdit found in focused window");
            }

            // Update float window connection for input handling
            self.update_float_window_connection();

            // Found a CodeEdit in a focused window - return true regardless
            return true;
        }

        false
    }

    /// Find and focus the ShaderEditor's CodeEdit after closing a shader tab
    /// Called from process() when focus_shader_after_close flag is set
    pub(super) fn focus_shader_editor_code_edit(&mut self) {
        // Find the ShaderEditor's visible CodeEdit
        let editor = EditorInterface::singleton();
        let Some(base_control) = editor.get_base_control() else {
            crate::verbose_print!("[godot-neovim] focus_shader_editor: no base_control");
            return;
        };

        // Find EditorNode to search for ShaderEditor
        let Some(editor_node) = self.find_editor_node(base_control.clone().upcast()) else {
            crate::verbose_print!("[godot-neovim] focus_shader_editor: no EditorNode found");
            return;
        };

        // Search for ShaderEditor's CodeEdit
        if let Some(mut code_edit) = self.find_shader_editor_code_edit(editor_node) {
            code_edit.grab_focus();
            crate::verbose_print!("[godot-neovim] Focused ShaderEditor CodeEdit after close");
        } else {
            crate::verbose_print!(
                "[godot-neovim] focus_shader_editor: no ShaderEditor CodeEdit found"
            );
        }
    }

    /// Find visible CodeEdit in ShaderEditor hierarchy
    fn find_shader_editor_code_edit(&self, node: Gd<Node>) -> Option<Gd<CodeEdit>> {
        let class_name = node.get_class().to_string();

        // Look for ShaderTextEditor (contains CodeEdit for shaders)
        if class_name == "ShaderTextEditor" {
            // ShaderTextEditor > CodeEdit
            let child_count = node.get_child_count();
            for i in 0..child_count {
                if let Some(child) = node.get_child(i) {
                    if let Ok(code_edit) = child.try_cast::<CodeEdit>() {
                        if code_edit.is_visible_in_tree() {
                            return Some(code_edit);
                        }
                    }
                }
            }
        }

        // Recursively search children
        let child_count = node.get_child_count();
        for i in 0..child_count {
            if let Some(child) = node.get_child(i) {
                // Skip hidden nodes for efficiency
                if let Ok(control) = child.clone().try_cast::<Control>() {
                    if !control.is_visible() {
                        continue;
                    }
                }
                if let Some(code_edit) = self.find_shader_editor_code_edit(child) {
                    return Some(code_edit);
                }
            }
        }

        None
    }
}
