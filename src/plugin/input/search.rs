//! Search mode input handling (/, ?)

use super::super::GodotNeovimPlugin;
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    pub(in crate::plugin) fn handle_search_mode_input(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) {
        let keycode = key_event.get_keycode();

        if keycode == Key::ESCAPE {
            self.close_search_mode();
        } else if keycode == Key::ENTER {
            self.execute_search();
        } else if keycode == Key::BACKSPACE {
            // Remove last character (but keep the '/' or '?')
            if self.search_buffer.len() > 1 {
                self.search_buffer.pop();
                self.update_search_display();
            }
        } else {
            // Append character to search buffer
            let unicode = key_event.get_unicode();
            if unicode > 0 {
                if let Some(c) = char::from_u32(unicode) {
                    self.search_buffer.push(c);
                    self.update_search_display();
                }
            }
        }

        if let Some(mut viewport) = self.base().get_viewport() {
            viewport.set_input_as_handled();
        }
    }
}
