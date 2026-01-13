//! Key conversion utilities

use super::GodotNeovimPlugin;
use godot::classes::InputEventKey;
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Convert Godot key event to Neovim key string
    pub(super) fn key_event_to_nvim_string(&self, event: &Gd<InputEventKey>) -> Option<String> {
        let keycode = event.get_keycode();
        let ctrl = event.is_ctrl_pressed();
        let alt = event.is_alt_pressed();
        let shift = event.is_shift_pressed();

        // Ctrl+[ is equivalent to Escape (terminal standard)
        if ctrl && keycode == Key::BRACKETLEFT {
            return Some("<Esc>".to_string());
        }

        // Handle special keys
        let key_str = match keycode {
            Key::ESCAPE => "<Esc>".to_string(),
            Key::ENTER => "<CR>".to_string(),
            Key::TAB => "<Tab>".to_string(),
            Key::BACKSPACE => "<BS>".to_string(),
            Key::DELETE => "<Del>".to_string(),
            Key::UP => "<Up>".to_string(),
            Key::DOWN => "<Down>".to_string(),
            Key::LEFT => "<Left>".to_string(),
            Key::RIGHT => "<Right>".to_string(),
            Key::HOME => "<Home>".to_string(),
            Key::END => "<End>".to_string(),
            Key::PAGEUP => "<PageUp>".to_string(),
            Key::PAGEDOWN => "<PageDown>".to_string(),
            Key::F1 => "<F1>".to_string(),
            Key::F2 => "<F2>".to_string(),
            Key::F3 => "<F3>".to_string(),
            Key::F4 => "<F4>".to_string(),
            Key::F5 => "<F5>".to_string(),
            Key::F6 => "<F6>".to_string(),
            Key::F7 => "<F7>".to_string(),
            Key::F8 => "<F8>".to_string(),
            Key::F9 => "<F9>".to_string(),
            Key::F10 => "<F10>".to_string(),
            Key::F11 => "<F11>".to_string(),
            Key::F12 => "<F12>".to_string(),
            Key::SPACE => " ".to_string(),
            _ => {
                // Get unicode character
                let unicode = event.get_unicode();
                if unicode > 0 {
                    let c = char::from_u32(unicode)?;
                    // Apply shift modifier for letters (get_unicode may not include shift)
                    if shift && c.is_ascii_lowercase() {
                        c.to_ascii_uppercase().to_string()
                    } else {
                        c.to_string()
                    }
                } else {
                    return None;
                }
            }
        };

        // Apply modifiers
        let result = if ctrl || alt {
            let mut mods = String::new();
            if ctrl {
                mods.push('C');
            }
            if alt {
                mods.push('A');
            }
            if shift && key_str.len() == 1 {
                mods.push('S');
            }

            if key_str.starts_with('<') {
                // Already a special key
                format!("<{}-{}>", mods, &key_str[1..key_str.len() - 1])
            } else {
                format!("<{}-{}>", mods, key_str)
            }
        } else {
            key_str
        };

        Some(result)
    }

    /// Convert Godot key event to Neovim notation for insert mode (strict mode)
    pub(super) fn key_event_to_nvim_notation(&self, key_event: &Gd<InputEventKey>) -> String {
        let keycode = key_event.get_keycode();
        let unicode = key_event.get_unicode();
        let ctrl = key_event.is_ctrl_pressed();
        let alt = key_event.is_alt_pressed();
        let shift = key_event.is_shift_pressed();

        // Handle special keys
        let special = match keycode {
            Key::BACKSPACE => Some("<BS>"),
            Key::TAB => Some("<Tab>"),
            Key::ENTER => Some("<CR>"),
            Key::DELETE => Some("<Del>"),
            Key::HOME => Some("<Home>"),
            Key::END => Some("<End>"),
            Key::PAGEUP => Some("<PageUp>"),
            Key::PAGEDOWN => Some("<PageDown>"),
            Key::UP => Some("<Up>"),
            Key::DOWN => Some("<Down>"),
            Key::LEFT => Some("<Left>"),
            Key::RIGHT => Some("<Right>"),
            Key::F1 => Some("<F1>"),
            Key::F2 => Some("<F2>"),
            Key::F3 => Some("<F3>"),
            Key::F4 => Some("<F4>"),
            Key::F5 => Some("<F5>"),
            Key::F6 => Some("<F6>"),
            Key::F7 => Some("<F7>"),
            Key::F8 => Some("<F8>"),
            Key::F9 => Some("<F9>"),
            Key::F10 => Some("<F10>"),
            Key::F11 => Some("<F11>"),
            Key::F12 => Some("<F12>"),
            _ => None,
        };

        if let Some(key_str) = special {
            // Add modifiers to special keys
            if ctrl || alt || shift {
                let mut modifiers = String::new();
                if ctrl {
                    modifiers.push_str("C-");
                }
                if alt {
                    modifiers.push_str("A-");
                }
                if shift {
                    modifiers.push_str("S-");
                }
                // Convert <Key> to <C-A-S-Key>
                let inner = &key_str[1..key_str.len() - 1];
                return format!("<{}{}>", modifiers, inner);
            }
            return key_str.to_string();
        }

        // Handle printable characters
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                // Ctrl+letter combinations
                if ctrl && !alt {
                    let base_char = c.to_ascii_lowercase();
                    if base_char.is_ascii_alphabetic() {
                        return format!("<C-{}>", base_char);
                    }
                }
                // Alt combinations
                if alt && !ctrl {
                    return format!("<A-{}>", c);
                }
                // Ctrl+Alt combinations
                if ctrl && alt {
                    return format!("<C-A-{}>", c);
                }
                // Regular character (shift is already applied in unicode)
                return c.to_string();
            }
        }

        String::new()
    }

    /// Get and clear the count buffer, returning 1 if empty
    pub(super) fn get_and_clear_count(&mut self) -> i32 {
        if self.count_buffer.is_empty() {
            return 1;
        }
        let count = self.count_buffer.parse::<i32>().unwrap_or(1).max(1);
        self.count_buffer.clear();
        count
    }
}
