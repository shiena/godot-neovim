//! Insert mode input handling

use super::super::GodotNeovimPlugin;
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    pub(in crate::plugin) fn handle_insert_mode_input(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) {
        // Intercept Escape or Ctrl+[ to exit insert mode
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

        // Ctrl+B in insert mode: exit insert and enter visual block mode
        let is_ctrl_b = key_event.is_ctrl_pressed() && key_event.get_keycode() == Key::B;
        if is_ctrl_b {
            // First sync buffer and exit insert mode
            self.send_escape();
            // Then enter visual block mode
            let completed = self.send_keys("<C-v>");
            if completed {
                self.clear_last_key();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Ctrl/Alt modified keys are sent to Neovim for Vim insert mode commands
        // (Ctrl+w, Ctrl+u, Ctrl+r, Ctrl+o, etc.)
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
        }

        // Normal character input: let Godot handle it (IME/autocomplete support)
    }
}
