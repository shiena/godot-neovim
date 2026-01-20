//! Editor management: finding CodeEdit, script change handling

use super::GodotNeovimPlugin;
use godot::classes::{CodeEdit, Control, EditorInterface};
use godot::prelude::*;

impl GodotNeovimPlugin {
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
                if let Some(mut parent) = label.get_parent() {
                    if parent.instance_id() == status_bar.instance_id() {
                        return; // Already in correct position
                    }
                    // Remove from current parent
                    parent.remove_child(label);
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
        if let Some(script_editor) = editor.get_script_editor() {
            // Try to find the currently focused CodeEdit first
            if let Some(code_edit) =
                self.find_focused_code_edit(script_editor.clone().upcast::<Control>())
            {
                crate::verbose_print!("[godot-neovim] Found focused CodeEdit");
                self.current_editor = Some(code_edit);
                self.connect_caret_changed_signal();
                self.connect_resized_signal();
                return;
            }
            // Fallback: find visible CodeEdit
            if let Some(code_edit) = self.find_visible_code_edit(script_editor.upcast::<Control>())
            {
                crate::verbose_print!("[godot-neovim] Found visible CodeEdit");
                self.current_editor = Some(code_edit);
                self.connect_caret_changed_signal();
                self.connect_resized_signal();
            }
        }
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

    /// Check if editor has focus
    pub(super) fn editor_has_focus(&self) -> bool {
        if let Some(ref editor) = self.current_editor {
            // Check if editor instance is still valid (not freed)
            if editor.is_instance_valid() {
                return editor.has_focus();
            }
        }
        false
    }
}
