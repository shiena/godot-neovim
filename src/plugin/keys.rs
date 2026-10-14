//! Key conversion utilities

use super::GodotNeovimPlugin;
use godot::classes::InputEventKey;
use godot::global::Key;
use godot::obj::EngineEnum;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// Detect whether the OS has already produced a composed character via Alt
    /// (e.g. macOS Option+Q → `@`, or AltGr+Q → `@` on Windows German layout).
    /// In that case the user wants the produced character inserted as text,
    /// not `<A-x>` / `<C-A-x>` sent to Neovim.
    ///
    /// Note: Windows reports AltGr as Ctrl+Alt simultaneously, so we don't
    /// exclude ctrl here. IME like CorvusSKK reports composed characters with
    /// only ctrl set (no alt), so requiring alt avoids interfering with them.
    pub(super) fn is_alt_composed_unicode(&self, event: &Gd<InputEventKey>) -> bool {
        if !event.is_alt_pressed() {
            return false;
        }
        let unicode = event.get_unicode();
        if unicode == 0 {
            return false;
        }
        let Some(uc) = char::from_u32(unicode) else {
            return false;
        };
        let Some(kc) = char::from_u32(event.get_keycode().ord() as u32) else {
            return false;
        };
        !kc.eq_ignore_ascii_case(&uc)
    }

    /// Convert a Ctrl/Alt/Meta-modified key event to Neovim notation for
    /// keymap lookup, e.g. Ctrl+S → `<C-s>`, Cmd+S (macOS) → `<D-s>`,
    /// Ctrl+Shift+S → `<C-S-s>`. Modifiers are emitted in C-A-S-D order to
    /// match the keymap editor's accepted notation.
    ///
    /// Letters and digits are resolved from the keycode rather than the
    /// unicode value, because the OS may report no character (or a control
    /// character) while Ctrl/Cmd is held.
    /// Returns None for unmodified keys and modifier-only presses.
    pub(super) fn modified_key_notation(&self, event: &Gd<InputEventKey>) -> Option<String> {
        let ctrl = event.is_ctrl_pressed();
        let alt = event.is_alt_pressed();
        let meta = event.is_meta_pressed();
        let shift = event.is_shift_pressed();
        if !(ctrl || alt || meta) {
            return None;
        }

        let keycode = event.get_keycode();
        let ord = keycode.ord();
        let base = match keycode {
            Key::ESCAPE => "Esc".to_string(),
            Key::ENTER => "CR".to_string(),
            Key::TAB => "Tab".to_string(),
            Key::BACKSPACE => "BS".to_string(),
            Key::DELETE => "Del".to_string(),
            Key::UP => "Up".to_string(),
            Key::DOWN => "Down".to_string(),
            Key::LEFT => "Left".to_string(),
            Key::RIGHT => "Right".to_string(),
            Key::HOME => "Home".to_string(),
            Key::END => "End".to_string(),
            Key::PAGEUP => "PageUp".to_string(),
            Key::PAGEDOWN => "PageDown".to_string(),
            Key::SPACE => "Space".to_string(),
            _ if (Key::A.ord()..=Key::Z.ord()).contains(&ord)
                || (Key::KEY_0.ord()..=Key::KEY_9.ord()).contains(&ord) =>
            {
                let c = char::from_u32(ord as u32)?.to_ascii_lowercase();
                c.to_string()
            }
            _ => {
                // Punctuation: the unicode value already reflects Shift.
                let c = char::from_u32(event.get_unicode()).filter(|c| !c.is_control())?;
                return Some(Self::format_modified_key(
                    ctrl,
                    alt,
                    false,
                    meta,
                    &c.to_string(),
                ));
            }
        };
        // Shift is significant for letters/digits and special keys.
        Some(Self::format_modified_key(ctrl, alt, shift, meta, &base))
    }

    fn format_modified_key(ctrl: bool, alt: bool, shift: bool, meta: bool, base: &str) -> String {
        let mut s = String::from("<");
        for (on, m) in [(ctrl, "C-"), (alt, "A-"), (shift, "S-"), (meta, "D-")] {
            if on {
                s.push_str(m);
            }
        }
        s.push_str(base);
        s.push('>');
        s
    }

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
            // Note: F1-F12 are intentionally NOT mapped - let Godot handle them
            // F1 opens :help in Neovim which causes freezes
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
            // Note: F1-F12 are intentionally NOT mapped - let Godot handle them
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
