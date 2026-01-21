//! Editor management: finding CodeEdit, script change handling

use super::GodotNeovimPlugin;
use godot::classes::{CodeEdit, Control, EditorInterface, Window};
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
        let Some(ref neovim) = self.neovim else {
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
        // Use call_deferred to ensure Godot has fully switched to the new script
        // before we try to find the CodeEdit and sync buffer
        self.base_mut()
            .call_deferred("handle_script_changed_deferred", &[]);
    }

    /// Reposition mode label to current editor's status bar
    pub(super) fn reposition_mode_label(&mut self) {
        // Check if label is still valid (may have been freed with previous status bar)
        let label_valid = self
            .mode_label
            .as_ref()
            .is_some_and(|label| label.is_instance_valid());

        if !label_valid {
            // Label was freed, create a new one
            self.mode_label = None;
            self.create_mode_label();
            return;
        }

        let Some(ref label) = self.mode_label else {
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
                        if let Some(mut old_label) = self.mode_label.take() {
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

        let editor = EditorInterface::singleton();

        // First, try to get the focused CodeEdit directly via gui_get_focus_owner
        // This works for both docked and floating windows
        if let Some(code_edit) = self.get_focused_code_edit_direct() {
            crate::verbose_print!("[godot-neovim] Found focused CodeEdit (direct)");
            self.current_editor = Some(code_edit);
        } else if let Some(script_editor) = editor.get_script_editor() {
            // Try to find the currently focused CodeEdit by traversing ScriptEditor
            if let Some(code_edit) =
                self.find_focused_code_edit(script_editor.clone().upcast::<Control>())
            {
                crate::verbose_print!("[godot-neovim] Found focused CodeEdit");
                self.current_editor = Some(code_edit);
            } else if let Some(code_edit) =
                self.find_visible_code_edit(script_editor.upcast::<Control>())
            {
                // Fallback: find visible CodeEdit
                crate::verbose_print!("[godot-neovim] Found visible CodeEdit");
                self.current_editor = Some(code_edit);
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

    /// Recursively find visible CodeEdit
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
}
