//! UI-related operations: mode label, status bar, signal connections

use super::{EditorType, GodotNeovimPlugin};
use godot::classes::{Control, EditorInterface, Label};
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Create and add the mode label to the status bar
    /// Creates separate labels for ScriptEditor and ShaderEditor
    pub(super) fn create_mode_label(&mut self) {
        crate::verbose_print!(
            "[godot-neovim] create_mode_label called for {:?}",
            self.current_editor_type
        );

        // Only create label if we have a current editor with a status bar
        let Some(code_edit) = &self.current_editor else {
            crate::verbose_print!("[godot-neovim] create_mode_label: no current_editor");
            return;
        };

        let Some(mut status_bar) = self.find_status_bar(code_edit.clone().upcast()) else {
            crate::verbose_print!("[godot-neovim] create_mode_label: no status_bar found");
            return;
        };

        let mut label = Label::new_alloc();
        label.set_text(" NORMAL ");

        // Style the label
        label.add_theme_color_override("font_color", Color::from_rgb(0.0, 1.0, 0.5));

        // Add to status bar
        status_bar.add_child(&label);
        status_bar.move_child(&label, 0);

        // Store in appropriate field based on editor type
        match self.current_editor_type {
            EditorType::Shader => {
                label.set_name("NeovimShaderModeLabel");
                self.shader_mode_label = Some(label);
                crate::verbose_print!("[godot-neovim] Created mode label for ShaderEditor");
            }
            _ => {
                label.set_name("NeovimModeLabel");
                self.mode_label = Some(label);
                crate::verbose_print!("[godot-neovim] Created mode label for ScriptEditor");
            }
        }
    }

    /// Get the current mode label based on editor type
    #[allow(dead_code)]
    pub(super) fn get_current_mode_label(&mut self) -> Option<&mut Gd<Label>> {
        match self.current_editor_type {
            EditorType::Shader => self.shader_mode_label.as_mut(),
            _ => self.mode_label.as_mut(),
        }
    }

    /// Create and add the recording indicator label to the status bar
    /// Creates separate labels for ScriptEditor and ShaderEditor
    pub(super) fn create_recording_label(&mut self) {
        let Some(code_edit) = &self.current_editor else {
            return;
        };

        let Some(mut status_bar) = self.find_status_bar(code_edit.clone().upcast()) else {
            return;
        };

        // Check if label already exists
        let label_name = match self.current_editor_type {
            EditorType::Shader => "NeovimShaderRecordingLabel",
            _ => "NeovimRecordingLabel",
        };

        // Don't create if already exists
        if status_bar.has_node(label_name) {
            return;
        }

        let mut label = Label::new_alloc();
        label.set_name(label_name);
        label.set_text("");
        label.set_visible(false);

        // Style: red color for recording indicator
        label.add_theme_color_override("font_color", Color::from_rgb(1.0, 0.3, 0.3));

        // Add to status bar, after mode label (index 1)
        status_bar.add_child(&label);
        status_bar.move_child(&label, 1);

        match self.current_editor_type {
            EditorType::Shader => {
                self.shader_recording_label = Some(label);
            }
            _ => {
                self.recording_label = Some(label);
            }
        }
    }

    /// Update recording indicator visibility and text
    pub(super) fn update_recording_label(&mut self, register: Option<char>) {
        let label = match self.current_editor_type {
            EditorType::Shader => self.shader_recording_label.as_mut(),
            _ => self.recording_label.as_mut(),
        };

        let Some(label) = label else {
            return;
        };

        if !label.is_instance_valid() {
            return;
        }

        match register {
            Some(reg) => {
                label.set_text(&format!(" recording @{} ", reg));
                label.set_visible(true);
            }
            None => {
                label.set_text("");
                label.set_visible(false);
            }
        }
    }

    /// Find the status bar HBoxContainer in the editor hierarchy
    pub(super) fn find_status_bar(&self, node: Gd<Control>) -> Option<Gd<Control>> {
        // The status bar is an HBoxContainer inside CodeTextEditor (sibling of CodeEdit)
        // CodeTextEditor > CodeEdit (node)
        //                > HBoxContainer (status bar with line/column info)

        // Get parent (should be CodeTextEditor)
        let parent = node.get_parent()?;

        let parent_class = parent.get_class().to_string();
        crate::verbose_print!(
            "[godot-neovim] CodeEdit parent: {} ({})",
            parent.get_name(),
            parent_class
        );

        // Search siblings for HBoxContainer (status bar)
        let child_count = parent.get_child_count();
        crate::verbose_print!(
            "[godot-neovim] Searching {} siblings for status bar",
            child_count
        );
        for i in 0..child_count {
            if let Some(child) = parent.get_child(i) {
                let class_name = child.get_class().to_string();
                crate::verbose_print!(
                    "[godot-neovim]   Sibling {}: {} ({})",
                    i,
                    child.get_name(),
                    class_name
                );
                if class_name == "HBoxContainer" {
                    if let Ok(control) = child.try_cast::<Control>() {
                        crate::verbose_print!("[godot-neovim] Found HBoxContainer status bar");
                        return Some(control);
                    }
                }
            }
        }

        crate::verbose_print!("[godot-neovim] Status bar not found");
        None
    }

    /// Connect to ScriptEditor signals (script changed, script close)
    pub(super) fn connect_script_editor_signals(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(mut script_editor) = editor.get_script_editor() {
            // Connect to editor script changed signal
            let callable = self.base().callable("on_script_changed");
            script_editor.connect("editor_script_changed", &callable);

            // Connect to script close signal (for Neovim buffer cleanup)
            let close_callable = self.base().callable("on_script_close");
            script_editor.connect("script_close", &close_callable);
        }
    }

    /// Connect to EditorSettings changed signal
    pub(super) fn connect_settings_signals(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(mut editor_settings) = editor.get_editor_settings() {
            // Connect to settings changed signal
            let callable = self.base().callable("on_settings_changed");
            editor_settings.connect("settings_changed", &callable);
        }
    }

    /// Connect to CodeEdit caret_changed signal
    pub(super) fn connect_caret_changed_signal(&mut self) {
        // Create callable first to avoid borrow conflicts
        let callable = self.base().callable("on_caret_changed");

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Connect to caret_changed signal to detect mouse clicks and other cursor changes
        // Check if already connected to avoid duplicate connections
        if !editor.is_connected("caret_changed", &callable) {
            editor.connect("caret_changed", &callable);
            crate::verbose_print!("[godot-neovim] Connected to caret_changed signal");
        }
    }

    /// Disconnect from CodeEdit caret_changed signal
    pub(super) fn disconnect_caret_changed_signal(&mut self) {
        // Create callable first to avoid borrow conflicts
        let callable = self.base().callable("on_caret_changed");

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Disconnect from caret_changed signal before closing
        if editor.is_connected("caret_changed", &callable) {
            editor.disconnect("caret_changed", &callable);
            crate::verbose_print!("[godot-neovim] Disconnected from caret_changed signal");
        }
    }

    /// Connect to CodeEdit resized signal
    pub(super) fn connect_resized_signal(&mut self) {
        // Create callable first to avoid borrow conflicts
        let callable = self.base().callable("on_editor_resized");

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Connect to resized signal to detect editor size changes
        // (window resize, panel toggle, dock width changes, etc.)
        if !editor.is_connected("resized", &callable) {
            editor.connect("resized", &callable);
            crate::verbose_print!("[godot-neovim] Connected to resized signal");
        }
    }

    /// Disconnect from CodeEdit resized signal
    pub(super) fn disconnect_resized_signal(&mut self) {
        // Create callable first to avoid borrow conflicts
        let callable = self.base().callable("on_editor_resized");

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Disconnect from resized signal before closing
        if editor.is_connected("resized", &callable) {
            editor.disconnect("resized", &callable);
            crate::verbose_print!("[godot-neovim] Disconnected from resized signal");
        }
    }

    /// Connect to CodeEdit gui_input signal for float window input handling
    /// Float windows don't receive input through EditorPlugin.input()
    pub(super) fn connect_gui_input_signal(&mut self) {
        let callable = self.base().callable("on_codeedit_gui_input");

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        if !editor.is_connected("gui_input", &callable) {
            editor.connect("gui_input", &callable);
            crate::verbose_print!("[godot-neovim] Connected to gui_input signal");
        }
    }

    /// Disconnect from CodeEdit gui_input signal
    pub(super) fn disconnect_gui_input_signal(&mut self) {
        let callable = self.base().callable("on_codeedit_gui_input");

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        if editor.is_connected("gui_input", &callable) {
            editor.disconnect("gui_input", &callable);
            crate::verbose_print!("[godot-neovim] Disconnected from gui_input signal");
        }
    }
}
