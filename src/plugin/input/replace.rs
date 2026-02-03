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
            // Record <Esc> to macro buffer before send_escape
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push("<Esc>".to_string());
            }
            self.send_escape();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Ctrl/Alt modified keys are sent to Neovim
        // IMPORTANT: Only send actual Vim commands (<C-...>, <A-...>), not plain characters
        // IME like CorvusSKK may report composed characters with ctrl modifier still set
        let ctrl = key_event.is_ctrl_pressed();
        let alt = key_event.is_alt_pressed();
        if ctrl || alt {
            let nvim_key = self.key_event_to_nvim_notation(key_event);
            // Only send if it's an actual Vim command notation (starts with <)
            if !nvim_key.is_empty() && nvim_key.starts_with('<') {
                // Record to macro buffer
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer.push(nvim_key.clone());
                }
                self.send_keys(&nvim_key);
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
            }
            return;
        }

        // Record keys to macro buffer if recording
        if self.recording_macro.is_some() && !self.playing_macro {
            let keycode = key_event.get_keycode();
            // Special keys
            match keycode {
                Key::BACKSPACE => {
                    self.macro_buffer.push("<BS>".to_string());
                }
                Key::ENTER => {
                    self.macro_buffer.push("<CR>".to_string());
                }
                Key::DELETE => {
                    self.macro_buffer.push("<Del>".to_string());
                }
                Key::TAB => {
                    self.macro_buffer.push("<Tab>".to_string());
                }
                _ => {
                    // Normal characters
                    let unicode = key_event.get_unicode();
                    if unicode > 0 {
                        if let Some(c) = char::from_u32(unicode) {
                            self.macro_buffer.push(c.to_string());
                        }
                    }
                }
            }
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
