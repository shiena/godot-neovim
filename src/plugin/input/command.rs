//! Command mode input handling (:)

use super::super::GodotNeovimPlugin;
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    pub(in crate::plugin) fn handle_command_mode_input(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) {
        let keycode = key_event.get_keycode();

        if keycode == Key::ESCAPE {
            self.close_command_line();
        } else if keycode == Key::ENTER {
            self.execute_command();
        } else if keycode == Key::BACKSPACE {
            // Remove last character (but keep the ':')
            if self.command_buffer.len() > 1 {
                self.command_buffer.pop();
                self.update_command_display();
            }
            // Reset history browsing when editing
            self.command_history_index = None;
        } else if keycode == Key::UP {
            // Browse command history (older)
            self.command_history_up();
        } else if keycode == Key::DOWN {
            // Browse command history (newer)
            self.command_history_down();
        } else {
            // Append character to command buffer
            let unicode = key_event.get_unicode();
            if unicode > 0 {
                if let Some(c) = char::from_u32(unicode) {
                    self.command_buffer.push(c);
                    self.update_command_display();
                    // Reset history browsing when typing
                    self.command_history_index = None;
                }
            }
        }

        if let Some(mut viewport) = self.base().get_viewport() {
            viewport.set_input_as_handled();
        }
    }
}
