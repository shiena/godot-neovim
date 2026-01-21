//! Pending operation handlers (f/t/r, marks, macros, registers)

use super::super::GodotNeovimPlugin;
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    pub(in crate::plugin) fn handle_pending_char_op(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> bool {
        let Some(op) = self.pending_char_op else {
            return false;
        };

        let keycode = key_event.get_keycode();

        // Ignore modifier-only key presses (SHIFT, CTRL, ALT, META keys themselves)
        // These are pressed before the actual character key and should not cancel the operation
        if matches!(
            keycode,
            Key::SHIFT | Key::CTRL | Key::ALT | Key::META | Key::CAPSLOCK | Key::NUMLOCK
        ) {
            // Don't consume the event, but don't cancel either - wait for actual character
            return false;
        }

        // Cancel on Escape or any modifier key combination (Ctrl+X, Alt+X, etc.)
        if keycode == Key::ESCAPE
            || key_event.is_ctrl_pressed()
            || key_event.is_alt_pressed()
            || key_event.is_meta_pressed()
        {
            self.pending_char_op = None;
            crate::verbose_print!(
                "[godot-neovim] Cancelled pending char op '{}' due to modifier/escape",
                op
            );
            // Don't consume the event - let it be processed normally
            return false;
        }

        // Get the character
        let unicode = key_event.get_unicode();
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                self.pending_char_op = None;
                // Build the key sequence for f/F/t/T
                let keys = match op {
                    'f' | 'F' | 't' | 'T' | 'r' => Some(format!("{}{}", op, c)),
                    _ => None,
                };

                match op {
                    'f' => self.find_char_forward(c, false),
                    'F' => self.find_char_backward(c, false),
                    't' => self.find_char_forward(c, true),
                    'T' => self.find_char_backward(c, true),
                    // 'r' is sent to Neovim via keys above (Neovim Master design)
                    _ => {}
                }

                // Send to Neovim and record to local macro buffer
                if let Some(keys) = keys {
                    self.send_keys(&keys);
                    // Record to local macro buffer (early return skips normal recording)
                    if self.recording_macro.is_some() && !self.playing_macro {
                        self.macro_buffer.push(keys);
                    }
                }
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return true;
            }
        }

        // Non-printable key pressed - cancel the pending operation
        self.pending_char_op = None;
        crate::verbose_print!(
            "[godot-neovim] Cancelled pending char op '{}' due to non-printable key",
            op
        );
        false
    }

    pub(in crate::plugin) fn handle_pending_mark_op(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> bool {
        let Some(op) = self.pending_mark_op else {
            return false;
        };

        let keycode = key_event.get_keycode();

        // Ignore modifier-only key presses (SHIFT, CTRL, ALT, META keys themselves)
        if matches!(
            keycode,
            Key::SHIFT | Key::CTRL | Key::ALT | Key::META | Key::CAPSLOCK | Key::NUMLOCK
        ) {
            return false;
        }

        // Cancel on Escape or any modifier key combination
        if keycode == Key::ESCAPE
            || key_event.is_ctrl_pressed()
            || key_event.is_alt_pressed()
            || key_event.is_meta_pressed()
        {
            self.pending_mark_op = None;
            crate::verbose_print!(
                "[godot-neovim] Cancelled pending mark op '{}' due to modifier/escape",
                op
            );
            // Don't consume the event - let it be processed normally
            return false;
        }

        // Get the character (must be a-z for marks)
        let unicode = key_event.get_unicode();
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                if c.is_ascii_lowercase() {
                    self.pending_mark_op = None;
                    match op {
                        'm' => self.set_mark(c),
                        '\'' => self.jump_to_mark_line(c),
                        '`' => self.jump_to_mark_position(c),
                        _ => {}
                    }
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return true;
                }
                // Non a-z character - cancel and let it be processed normally
                self.pending_mark_op = None;
                crate::verbose_print!(
                    "[godot-neovim] Cancelled pending mark op '{}' - invalid mark char '{}'",
                    op,
                    c
                );
                return false;
            }
        }

        // Non-printable key pressed - cancel the pending operation
        self.pending_mark_op = None;
        crate::verbose_print!(
            "[godot-neovim] Cancelled pending mark op '{}' due to non-printable key",
            op
        );
        false
    }

    pub(in crate::plugin) fn handle_pending_macro_op(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> bool {
        let Some(op) = self.pending_macro_op else {
            return false;
        };

        let keycode = key_event.get_keycode();

        // Ignore modifier-only key presses (SHIFT, CTRL, ALT, META keys themselves)
        if matches!(
            keycode,
            Key::SHIFT | Key::CTRL | Key::ALT | Key::META | Key::CAPSLOCK | Key::NUMLOCK
        ) {
            return false;
        }

        // Cancel on Escape or any modifier key combination
        if keycode == Key::ESCAPE
            || key_event.is_ctrl_pressed()
            || key_event.is_alt_pressed()
            || key_event.is_meta_pressed()
        {
            self.pending_macro_op = None;
            crate::verbose_print!(
                "[godot-neovim] Cancelled pending macro op '{}' due to modifier/escape",
                op
            );
            // Don't consume the event - let it be processed normally
            return false;
        }

        // Get the character
        let unicode = key_event.get_unicode();
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                self.pending_macro_op = None;
                match op {
                    'q' => {
                        // Start recording if a-z
                        if c.is_ascii_lowercase() {
                            self.start_macro_recording(c);
                        } else {
                            crate::verbose_print!(
                                "[godot-neovim] Macro recording cancelled - invalid register '{}'",
                                c
                            );
                        }
                    }
                    '@' => {
                        if c == '@' {
                            // @@ - replay last macro
                            self.replay_last_macro();
                        } else if c == ':' {
                            // @: - repeat last Ex command
                            self.repeat_last_ex_command();
                        } else if c.is_ascii_lowercase() {
                            // @{a-z} - play specific macro
                            self.play_macro(c);
                        } else {
                            crate::verbose_print!(
                                "[godot-neovim] Macro playback cancelled - invalid register '{}'",
                                c
                            );
                        }
                    }
                    _ => {}
                }
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return true;
            }
        }

        // Non-printable key pressed - cancel the pending operation
        self.pending_macro_op = None;
        crate::verbose_print!(
            "[godot-neovim] Cancelled pending macro op '{}' due to non-printable key",
            op
        );
        false
    }

    pub(in crate::plugin) fn handle_pending_register(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> bool {
        if self.selected_register != Some('\0') {
            return false;
        }

        let keycode = key_event.get_keycode();

        // Cancel on Escape
        if keycode == Key::ESCAPE {
            self.selected_register = None;
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return true;
        }

        // Get the character
        // Valid registers: a-z (named), + and * (clipboard), _ (black hole), 0 (yank)
        let unicode = key_event.get_unicode();
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                let is_valid_register =
                    c.is_ascii_lowercase() || c == '+' || c == '*' || c == '_' || c == '0';
                if is_valid_register {
                    self.selected_register = Some(c);
                    crate::verbose_print!("[godot-neovim] \"{}: Register selected", c);
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return true;
                }
            }
        }
        false
    }
}
