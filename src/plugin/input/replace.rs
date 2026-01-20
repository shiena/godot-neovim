//! Replace mode input handling

use super::super::GodotNeovimPlugin;
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    pub(in crate::plugin) fn handle_replace_mode_input(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) {
        // Intercept Escape or Ctrl+[ to exit replace mode
        let is_escape = key_event.get_keycode() == Key::ESCAPE;
        let is_ctrl_bracket =
            key_event.is_ctrl_pressed() && key_event.get_keycode() == Key::BRACKETLEFT;

        if is_escape || is_ctrl_bracket {
            self.send_escape();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Ctrl/Alt modified keys are sent to Neovim
        let ctrl = key_event.is_ctrl_pressed();
        let alt = key_event.is_alt_pressed();
        if ctrl || alt {
            let nvim_key = self.key_event_to_nvim_notation(key_event);
            if !nvim_key.is_empty() {
                self.send_keys(&nvim_key);
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
            }
            return;
        }

        // In replace mode, we need to delete the character under cursor
        // before letting Godot insert the new character
        // This simulates overwrite behavior
        let unicode = key_event.get_unicode();
        if unicode > 0 {
            if let Some(ref mut editor) = self.current_editor {
                let line = editor.get_caret_line();
                let col = editor.get_caret_column();
                let line_text: String = editor.get_line(line).to_string();

                // Only delete if we're not at end of line
                if (col as usize) < line_text.chars().count() {
                    // Delete character at cursor
                    editor.select(line, col, line, col + 1);
                    editor.delete_selection();
                }
            }
        }
        // Let Godot insert the character
    }
}
