//! UI-related operations: mode label, status bar, signal connections, key sequence display

use super::GodotNeovimPlugin;
use crate::settings;
use godot::classes::{Control, EditorInterface, Label, StyleBoxFlat};
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Create and add the mode label to the status bar
    pub(super) fn create_mode_label(&mut self) {
        // Only create label if we have a current editor with a status bar
        let Some(code_edit) = &self.current_editor else {
            return;
        };

        let Some(mut status_bar) = self.find_status_bar(code_edit.clone().upcast()) else {
            return;
        };

        let mut label = Label::new_alloc();
        label.set_text(" NORMAL ");
        label.set_name("NeovimModeLabel");

        // Style the label
        label.add_theme_color_override("font_color", Color::from_rgb(0.0, 1.0, 0.5));

        // Add to status bar
        status_bar.add_child(&label);
        status_bar.move_child(&label, 0);
        self.mode_label = Some(label);
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
        for i in 0..child_count {
            if let Some(child) = parent.get_child(i) {
                let class_name = child.get_class().to_string();
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

    /// Create and add the key sequence label overlay (verbose mode only)
    pub(super) fn create_key_sequence_label(&mut self) {
        // Only create if show_key_sequence is enabled (requires verbose mode)
        if !settings::get_show_key_sequence() {
            return;
        }

        let Some(ref code_edit) = self.current_editor else {
            return;
        };

        // Create label as child of CodeEdit for overlay positioning
        let mut label = Label::new_alloc();
        label.set_name("NeovimKeySequenceLabel");
        label.set_text("");

        // Style: white background, white text with black outline
        let mut style_box = StyleBoxFlat::new_gd();
        style_box.set_bg_color(Color::from_rgba(1.0, 1.0, 1.0, 0.9));
        style_box.set_corner_radius_all(4);
        style_box.set_content_margin_all(8.0);
        label.add_theme_stylebox_override("normal", &style_box);

        label.add_theme_font_size_override("font_size", 24);
        label.add_theme_color_override("font_color", Color::from_rgba(1.0, 1.0, 1.0, 1.0));
        label.add_theme_constant_override("outline_size", 4);
        label.add_theme_color_override("font_outline_color", Color::from_rgba(0.0, 0.0, 0.0, 1.0));

        // Set anchors to bottom-right
        label.set_anchors_preset(godot::classes::control::LayoutPreset::BOTTOM_RIGHT);
        label.set_h_grow_direction(godot::classes::control::GrowDirection::BEGIN);
        label.set_v_grow_direction(godot::classes::control::GrowDirection::BEGIN);

        // Add offset from corner (adjusted for larger font)
        label.set_position(Vector2::new(-10.0, -40.0));

        // Start hidden (alpha = 0)
        label.set_modulate(Color::from_rgba(1.0, 1.0, 1.0, 0.0));

        // Add to CodeEdit
        code_edit.clone().add_child(&label);

        self.key_sequence_label = Some(label);
        crate::verbose_print!("[godot-neovim] Key sequence label created");
    }

    /// Helper to format key sequence with spacing
    fn format_key_sequence(seq: &str) -> String {
        seq.chars()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Update key sequence display with new input
    pub(super) fn update_key_sequence_display(&mut self, key: &str) {
        // Only update if show_key_sequence is enabled
        if !settings::get_show_key_sequence() {
            return;
        }

        // Skip in Insert/Replace modes
        if self.is_insert_mode() || self.is_replace_mode() {
            return;
        }

        // Append key to display
        self.key_sequence_display.push_str(key);

        // Update label with 2-line display (previous + current)
        if let Some(ref mut label) = self.key_sequence_label {
            if label.is_instance_valid() {
                let current_spaced = Self::format_key_sequence(&self.key_sequence_display);
                let text = if self.key_sequence_previous.is_empty() {
                    format!(" {} ", current_spaced)
                } else {
                    let prev_spaced = Self::format_key_sequence(&self.key_sequence_previous);
                    format!(" {} \n {} ", prev_spaced, current_spaced)
                };
                label.set_text(&text);
                // Reset to fully visible
                label.set_modulate(Color::from_rgba(1.0, 1.0, 1.0, 1.0));
            }
        }

        // Reset fade timer
        self.key_sequence_fade_start = None;
    }

    /// Clear key sequence display and start fade out
    pub(super) fn clear_key_sequence_display(&mut self) {
        if self.key_sequence_display.is_empty() {
            return;
        }

        // Move current to previous for history display
        self.key_sequence_previous = std::mem::take(&mut self.key_sequence_display);

        // Update label to show only previous (current is now empty)
        if let Some(ref mut label) = self.key_sequence_label {
            if label.is_instance_valid() {
                let prev_spaced = Self::format_key_sequence(&self.key_sequence_previous);
                label.set_text(&format!(" {} ", prev_spaced));
            }
        }

        // Start fade out
        self.key_sequence_fade_start = Some(std::time::Instant::now());
    }

    /// Process key sequence fade animation (call from process())
    pub(super) fn process_key_sequence_fade(&mut self) {
        let Some(fade_start) = self.key_sequence_fade_start else {
            return;
        };

        let Some(ref mut label) = self.key_sequence_label else {
            return;
        };

        if !label.is_instance_valid() {
            self.key_sequence_label = None;
            return;
        }

        // Fade duration: 1000ms (doubled for better visibility)
        let elapsed = fade_start.elapsed().as_millis() as f32;
        let fade_duration = 1000.0;

        if elapsed >= fade_duration {
            // Fade complete - hide label and clear history
            label.set_modulate(Color::from_rgba(1.0, 1.0, 1.0, 0.0));
            label.set_text("");
            self.key_sequence_fade_start = None;
            self.key_sequence_previous.clear();
        } else {
            // Interpolate alpha from 1.0 to 0.0
            let alpha = 1.0 - (elapsed / fade_duration);
            label.set_modulate(Color::from_rgba(1.0, 1.0, 1.0, alpha));
        }
    }

    /// Cleanup key sequence label
    pub(super) fn cleanup_key_sequence_label(&mut self) {
        if let Some(mut label) = self.key_sequence_label.take() {
            if label.is_instance_valid() {
                label.queue_free();
            }
        }
        self.key_sequence_display.clear();
        self.key_sequence_previous.clear();
        self.key_sequence_fade_start = None;
    }
}
