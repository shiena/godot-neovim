//! State management: mode checking, pending input states, mode display

use super::GodotNeovimPlugin;
use godot::classes::text_edit::CaretType;
use godot::prelude::*;
use std::time::Instant;

/// Plugin version: Cargo.toml version for release, build datetime for debug
const VERSION: &str = env!("BUILD_VERSION");

impl GodotNeovimPlugin {
    /// Check if currently in insert mode
    /// Neovim mode_change events can send "i" or "insert" depending on context
    pub(super) fn is_insert_mode(&self) -> bool {
        self.current_mode == "i" || self.current_mode == "insert"
    }

    /// Check if currently in replace mode
    /// Neovim mode_change events can send "R" or "replace" depending on context
    pub(super) fn is_replace_mode(&self) -> bool {
        self.current_mode == "R" || self.current_mode == "replace"
    }

    /// Check if mode is a visual mode (v, V, or Ctrl+V)
    pub(super) fn is_visual_mode(mode: &str) -> bool {
        matches!(mode, "v" | "V" | "\x16" | "^V" | "CTRL-V" | "visual")
    }

    /// Check if currently in visual mode (instance method)
    pub(super) fn is_in_visual_mode(&self) -> bool {
        Self::is_visual_mode(&self.current_mode)
    }

    /// Check if mode is operator-pending mode (d, c, y, etc. waiting for motion)
    pub(super) fn is_operator_pending_mode(mode: &str) -> bool {
        matches!(mode, "operator" | "no")
    }

    /// Clear all pending input states to ensure mutual exclusivity
    /// Call this before setting any pending state
    pub(super) fn clear_pending_input_states(&mut self) {
        self.command_mode = false;
        self.search_mode = false;
        self.pending_char_op = None;
        self.pending_mark_op = None;
        self.pending_macro_op = None;
        // Clear register waiting state (Some('\0')) but preserve selected register
        if self.selected_register == Some('\0') {
            self.selected_register = None;
        }
    }

    /// Set last_key with timestamp for timeout tracking
    pub(super) fn set_last_key(&mut self, key: impl Into<String>) {
        self.last_key = key.into();
        self.last_key_time = Some(Instant::now());
    }

    /// Clear last_key and its timestamp
    pub(super) fn clear_last_key(&mut self) {
        self.last_key.clear();
        self.last_key_time = None;
    }

    /// Cancel any pending operator in Neovim and clear local state
    /// Call this before executing local commands that would conflict with pending operators
    pub(super) fn cancel_pending_operator(&mut self) {
        if !self.last_key.is_empty() {
            crate::verbose_print!(
                "[godot-neovim] Cancelling pending operator: '{}'",
                self.last_key
            );
            // Send Escape to cancel Neovim's pending operator via channel
            if let Some(neovim) = self.get_current_neovim() {
                if let Ok(client) = neovim.try_lock() {
                    if !client.send_key_via_channel("<Esc>") {
                        crate::verbose_print!(
                            "[godot-neovim] Failed to send <Esc> for pending operator cancellation"
                        );
                    }
                } else {
                    crate::verbose_print!(
                        "[godot-neovim] Mutex busy, could not send <Esc> for pending operator cancellation"
                    );
                }
            }
            // Always clear local state even if Neovim Escape is queued
            self.clear_last_key();
        }
    }

    pub(super) fn update_mode_display_with_cursor(
        &mut self,
        mode: &str,
        cursor: Option<(i64, i64)>,
    ) {
        // Clear version display flag (any operation returns to normal display)
        self.show_version = false;

        // Get the appropriate label based on current editor type
        let label = match self.current_editor_type {
            super::EditorType::Shader => self.shader_mode_label.as_mut(),
            _ => self.mode_label.as_mut(),
        };

        let Some(label) = label else {
            return;
        };

        // Check if label is still valid (may have been freed when script was closed)
        if !label.is_instance_valid() {
            match self.current_editor_type {
                super::EditorType::Shader => self.shader_mode_label = None,
                _ => self.mode_label = None,
            }
            return;
        }

        // Get mode display name
        // Note: Neovim returns "visual" for all visual modes (v, V, Ctrl+V)
        // We use visual_mode_type to distinguish between them
        let mode_name = match mode {
            "n" | "normal" => "NORMAL",
            "i" | "insert" => "INSERT",
            "v" | "visual" => {
                // Use tracked visual mode type since Neovim returns "visual" for all
                match self.visual_mode_type {
                    'V' => "V-LINE",
                    '\x16' => "V-BLOCK",
                    _ => "VISUAL",
                }
            }
            "V" | "visual-line" => "V-LINE",
            "\x16" | "^V" | "CTRL-V" | "visual-block" => "V-BLOCK",
            "c" | "command" => "COMMAND",
            "R" | "replace" => "REPLACE",
            _ => mode,
        };

        // Format with cursor position if available
        let display_text = if let Some((line, col)) = cursor {
            format!(" {} {}:{} ", mode_name, line, col)
        } else {
            format!(" {} ", mode_name)
        };

        label.set_text(&display_text);

        // Set color based on mode
        let color = match mode {
            "n" | "normal" => Color::from_rgb(0.0, 1.0, 0.5), // Green for normal
            "i" | "insert" => Color::from_rgb(0.4, 0.6, 1.0), // Blue for insert
            "R" | "replace" => Color::from_rgb(1.0, 0.3, 0.3), // Red for replace
            "v" | "V" | "\x16" | "^V" | "CTRL-V" | "visual" | "visual-line" | "visual-block" => {
                Color::from_rgb(1.0, 0.6, 0.2) // Orange for visual
            }
            "c" | "command" => Color::from_rgb(1.0, 1.0, 0.4), // Yellow for command
            _ => Color::from_rgb(1.0, 1.0, 1.0),               // White for unknown
        };

        label.add_theme_color_override("font_color", color);

        // Update caret type based on mode
        // Normal mode: block cursor, Insert mode: line cursor
        if let Some(ref mut editor) = self.current_editor {
            let caret_type = match mode {
                "i" | "insert" | "R" | "replace" => CaretType::LINE,
                _ => CaretType::BLOCK,
            };
            editor.set_caret_type(caret_type);
        }
    }

    /// Update status label to show version
    pub(crate) fn update_version_display(&mut self) {
        // Get the appropriate label based on current editor type
        let label = match self.current_editor_type {
            super::EditorType::Shader => self.shader_mode_label.as_mut(),
            _ => self.mode_label.as_mut(),
        };

        let Some(label) = label else {
            return;
        };

        // Check if label is still valid (may have been freed when script was closed)
        if !label.is_instance_valid() {
            match self.current_editor_type {
                super::EditorType::Shader => self.shader_mode_label = None,
                _ => self.mode_label = None,
            }
            return;
        }

        let display_text = format!(" godot-neovim v{} ", VERSION);
        label.set_text(&display_text);
        // White color for version display
        label.add_theme_color_override("font_color", Color::from_rgb(1.0, 1.0, 1.0));
    }

    /// Update status label to show "SHADER" mode (when ShaderEditor is focused)
    /// This indicates that the plugin is not intercepting input and Godot is handling editing
    pub(super) fn update_shader_mode_display(&mut self) {
        let Some(ref mut label) = self.mode_label else {
            return;
        };

        // Check if label is still valid
        if !label.is_instance_valid() {
            self.mode_label = None;
            return;
        }

        let display_text = " SHADER (native) ";
        label.set_text(display_text);
        // Gray color to indicate passive mode
        label.add_theme_color_override("font_color", Color::from_rgb(0.6, 0.6, 0.6));
    }
}
