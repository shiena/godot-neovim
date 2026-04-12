//! GDScript dispatch: process_key_event implementation
//!
//! Translates the normal.rs state machine into a dispatch-friendly form.
//! Instead of directly calling action_*_impl(), returns a VarDictionary
//! telling GDScript what key was resolved and whether to dispatch it.

use super::super::GodotNeovimPlugin;
use godot::classes::Input;
use godot::global::Key;
use godot::prelude::*;

/// Result keys for the Dictionary returned by process_key_event
const KEY_NEEDS_DISPATCH: &str = "needs_dispatch";
const KEY_MODE: &str = "mode";
const KEY_RESOLVED_KEY: &str = "resolved_key";
const KEY_PASS_THROUGH: &str = "pass_through";

impl GodotNeovimPlugin {
    /// Mark the current input event as handled (prevent Godot's default handling)
    fn mark_input_handled(&self) {
        if let Some(mut viewport) = self.base().get_viewport() {
            viewport.set_input_as_handled();
        }
    }

    /// Create a result dict indicating the key was handled internally (no GDScript dispatch needed)
    fn dispatch_handled(&self) -> VarDictionary {
        self.mark_input_handled();
        let mut dict = VarDictionary::new();
        dict.set(KEY_NEEDS_DISPATCH, false);
        dict
    }

    /// Create a result dict indicating the key should be dispatched to GDScript keymap
    fn dispatch_key(&self, resolved_key: &str) -> VarDictionary {
        self.mark_input_handled();
        let mut dict = VarDictionary::new();
        dict.set(KEY_NEEDS_DISPATCH, true);
        dict.set(KEY_MODE, GString::from(&self.current_mode));
        dict.set(KEY_RESOLVED_KEY, GString::from(resolved_key));
        dict
    }

    /// Create a result dict indicating the key should pass through to Godot
    fn dispatch_pass_through(&self) -> VarDictionary {
        // Do NOT mark as handled - let Godot process it
        let mut dict = VarDictionary::new();
        dict.set(KEY_NEEDS_DISPATCH, false);
        dict.set(KEY_PASS_THROUGH, true);
        dict
    }

    /// Main process_key_event implementation.
    /// Called by GDScript input handler to resolve a key event.
    /// Process a key event for normal/visual mode and return dispatch info.
    /// Called from input() after mode routing (command/search/insert/replace/pending ops)
    /// has already been handled. Only processes normal/visual mode keys.
    pub(in crate::plugin) fn process_key_event_impl(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> VarDictionary {
        // =====================================================================
        // Normal/Visual mode key processing
        // (Mode routing is done by input() before calling this)
        // =====================================================================
        let keycode = key_event.get_keycode();
        let unicode_char = char::from_u32(key_event.get_unicode());

        // ----- Ctrl+/ (toggle comment) → pass through to Godot -----
        if key_event.is_command_or_control_pressed() && keycode == Key::SLASH {
            self.action_toggle_comment_impl();
            return self.dispatch_pass_through();
        }

        // ----- Ctrl+key combinations → dispatch to GDScript keymap -----
        if key_event.is_ctrl_pressed() {
            if let Some(resolved) = self.resolve_ctrl_key(key_event) {
                return self.dispatch_key(&resolved);
            }
        }

        // ----- 'o' in visual mode: toggle selection direction (internal) -----
        if Self::is_visual_mode(&self.current_mode)
            && keycode == Key::O
            && !key_event.is_ctrl_pressed()
            && !key_event.is_shift_pressed()
        {
            self.send_keys("o");
            if self.current_mode == "v" {
                self.update_visual_selection();
            } else if self.current_mode == "V" {
                self.update_visual_line_selection();
            }
            crate::verbose_print!("[godot-neovim] o: Toggle visual selection direction");
            return self.dispatch_handled();
        }

        // ----- Pending prefix resolution -----
        // Must check before single-key handling to resolve g+key, [+key, etc.
        if let Some(result) = self.resolve_pending_prefix(key_event) {
            return result;
        }

        // ----- Register-aware operations -----
        if let Some(result) = self.handle_register_operations(key_event) {
            return result;
        }

        // ----- Count prefix (digits) -----
        if let Some(c) = unicode_char {
            if c.is_ascii_digit() && (c != '0' || !self.count_buffer.is_empty()) {
                self.count_buffer.push(c);
                self.send_keys(&c.to_string());
                self.last_key_time = Some(std::time::Instant::now());
                return self.dispatch_handled();
            }
        }

        // ----- Side-effect keys (handled internally) -----
        if let Some(result) = self.handle_side_effect_keys(key_event) {
            return result;
        }

        // ----- Pending state setup keys -----
        if let Some(result) = self.handle_pending_state_setup(key_event) {
            return result;
        }

        // ----- Operator keys (>>, <<, > motion, < motion) -----
        if let Some(result) = self.handle_operator_keys(key_event) {
            return result;
        }

        // ----- Prefix key accumulation (g, [, ], Z) -----
        if let Some(result) = self.handle_prefix_key_accumulation(key_event) {
            return result;
        }

        // ----- Dispatchable single keys (mapped to GDScript actions) -----
        // Keys like /, ?, :, n, N, *, #, u, K open Godot-side UI or call LSP,
        // and must go through the keymap dispatch (not sent directly to Neovim).
        if let Some(resolved) = self.resolve_dispatchable_single_key(key_event) {
            return self.dispatch_key(&resolved);
        }

        // ----- Visual mode type tracking -----
        if keycode == Key::V && !key_event.is_ctrl_pressed() {
            if key_event.is_shift_pressed() {
                self.visual_mode_type = 'V';
            } else {
                self.visual_mode_type = 'v';
            }
        }

        // =====================================================================
        // Phase 3: Default - convert to Neovim notation and dispatch
        // =====================================================================
        if let Some(keys) = self.key_event_to_nvim_string(key_event) {
            // Record key for macro if recording
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push(keys.clone());
            }

            // Handle scroll commands (zz, zt, zb) after sending key
            let completed = self.send_keys(&keys);
            let scroll_handled = if completed {
                self.handle_scroll_command(&keys)
            } else {
                false
            };

            // Track last key for sequence detection
            if !scroll_handled && !self.is_insert_mode() && !self.is_replace_mode() {
                if self.is_in_visual_mode() {
                    if keys == "i" || keys == "a" {
                        self.set_last_key(keys);
                    }
                } else {
                    self.set_last_key(keys);
                }
            }

            return self.dispatch_handled();
        }

        // Key couldn't be converted - ignore
        self.dispatch_handled()
    }

    // =====================================================================
    // Helper: Resolve Ctrl+key to Neovim notation for GDScript dispatch
    // =====================================================================
    fn resolve_ctrl_key(&self, key_event: &Gd<godot::classes::InputEventKey>) -> Option<String> {
        let keycode = key_event.get_keycode();
        match keycode {
            Key::B
            | Key::F
            | Key::D
            | Key::U
            | Key::Y
            | Key::E
            | Key::A
            | Key::X
            | Key::O
            | Key::I
            | Key::G
            | Key::R => {
                let ch = match keycode {
                    Key::B => 'b',
                    Key::F => 'f',
                    Key::D => 'd',
                    Key::U => 'u',
                    Key::Y => 'y',
                    Key::E => 'e',
                    Key::A => 'a',
                    Key::X => 'x',
                    Key::O => 'o',
                    Key::I => 'i',
                    Key::G => 'g',
                    Key::R => 'r',
                    _ => unreachable!(),
                };
                Some(format!("<C-{}>", ch))
            }
            _ => None,
        }
    }

    // =====================================================================
    // Helper: Resolve dispatchable single keys (/, ?, :, n, N, *, #, u, K)
    // These open Godot-side UI (search, command line) or call LSP (goto def,
    // documentation), so they must go through GDScript keymap dispatch rather
    // than being sent directly to Neovim.
    // =====================================================================
    fn resolve_dispatchable_single_key(
        &self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> Option<String> {
        let keycode = key_event.get_keycode();
        let unicode_char = char::from_u32(key_event.get_unicode());
        let ctrl = key_event.is_ctrl_pressed();
        let shift = key_event.is_shift_pressed();

        if ctrl {
            return None;
        }

        // '/' - forward search
        if unicode_char == Some('/') {
            return Some("/".to_string());
        }
        // '?' - backward search
        if unicode_char == Some('?') {
            return Some("?".to_string());
        }
        // ':' - command line
        if unicode_char == Some(':') {
            return Some(":".to_string());
        }
        // '*' - search word forward
        if unicode_char == Some('*') {
            return Some("*".to_string());
        }
        // '#' - search word backward
        if unicode_char == Some('#') {
            return Some("#".to_string());
        }
        // 'n' - search next (not Shift)
        if keycode == Key::N && !shift {
            return Some("n".to_string());
        }
        // 'N' - search previous (Shift+N)
        if keycode == Key::N && shift {
            return Some("N".to_string());
        }
        // 'u' - undo (not Shift, not after 'g' prefix which is gu = lowercase operator)
        if keycode == Key::U && !shift && self.last_key != "g" {
            return Some("u".to_string());
        }
        // 'K' - documentation (Shift+K, not after 'g' prefix which is gK)
        if keycode == Key::K && shift && self.last_key != "g" {
            return Some("K".to_string());
        }

        None
    }

    // =====================================================================
    // Helper: Resolve pending prefix keys (g, [, ], Z, >, <)
    // =====================================================================
    fn resolve_pending_prefix(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> Option<VarDictionary> {
        let keycode = key_event.get_keycode();
        let unicode_char = char::from_u32(key_event.get_unicode());

        // --- g-prefix resolution ---
        if self.last_key == "g" {
            if let Some(keys) = self.key_event_to_nvim_string(key_event) {
                let resolved = format!("g{}", keys);
                self.clear_last_key();
                return Some(self.dispatch_key(&resolved));
            }
            // Modifier-only key - don't clear prefix
            return Some(self.dispatch_handled());
        }

        // --- [-prefix resolution ---
        if self.last_key == "[" {
            // [[ - use keycode for keyboard layout independence
            if keycode == Key::BRACKETLEFT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.clear_last_key();
                self.send_keys("[[");
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer.push("[[".to_string());
                }
                return Some(self.dispatch_handled());
            }
            // [] - use keycode
            if keycode == Key::BRACKETRIGHT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.clear_last_key();
                self.send_keys("[]");
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer.push("[]".to_string());
                }
                return Some(self.dispatch_handled());
            }
            // [p
            if keycode == Key::P && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
                self.clear_last_key();
                self.send_keys("[p");
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer.push("[p".to_string());
                }
                return Some(self.dispatch_handled());
            }
            match unicode_char {
                Some('{') | Some('(') | Some('m') => {
                    let ch = unicode_char.unwrap();
                    let cmd = format!("[{}", ch);
                    self.clear_last_key();
                    self.send_keys(&cmd);
                    if self.recording_macro.is_some() && !self.playing_macro {
                        self.macro_buffer.push(cmd);
                    }
                    return Some(self.dispatch_handled());
                }
                Some('\0') | None => {
                    // Modifier-only key - don't clear prefix
                    return Some(self.dispatch_handled());
                }
                _ => {
                    self.clear_last_key();
                    // Not a recognized [ command, fall through
                }
            }
        }

        // --- ]-prefix resolution ---
        if self.last_key == "]" {
            // ]] - use keycode
            if keycode == Key::BRACKETRIGHT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.clear_last_key();
                self.send_keys("]]");
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer.push("]]".to_string());
                }
                return Some(self.dispatch_handled());
            }
            // ][ - use keycode
            if keycode == Key::BRACKETLEFT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.clear_last_key();
                self.send_keys("][");
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer.push("][".to_string());
                }
                return Some(self.dispatch_handled());
            }
            // ]p
            if keycode == Key::P && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
                self.clear_last_key();
                self.send_keys("]p");
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer.push("]p".to_string());
                }
                return Some(self.dispatch_handled());
            }
            match unicode_char {
                Some('}') | Some(')') | Some('m') => {
                    let ch = unicode_char.unwrap();
                    let cmd = format!("]{}", ch);
                    self.clear_last_key();
                    self.send_keys(&cmd);
                    if self.recording_macro.is_some() && !self.playing_macro {
                        self.macro_buffer.push(cmd);
                    }
                    return Some(self.dispatch_handled());
                }
                Some('\0') | None => {
                    return Some(self.dispatch_handled());
                }
                _ => {
                    self.clear_last_key();
                }
            }
        }

        // --- Z-prefix resolution ---
        if self.last_key == "Z" {
            if keycode == Key::Z && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
                self.clear_last_key();
                return Some(self.dispatch_key("ZZ"));
            }
            if keycode == Key::Q && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
                self.clear_last_key();
                return Some(self.dispatch_key("ZQ"));
            }
            // Clear Z prefix if another key
            self.clear_last_key();
        }

        // --- >-prefix resolution ---
        if self.last_key == ">" {
            if let Some(ch) = unicode_char {
                if ch == '>' {
                    // >> indent
                    self.send_keys(">>");
                    self.clear_last_key();
                    if self.recording_macro.is_some() && !self.playing_macro {
                        self.macro_buffer.push(">>".to_string());
                    }
                    return Some(self.dispatch_handled());
                } else {
                    // > + motion
                    self.send_keys(&format!(">{}", ch));
                    self.clear_last_key();
                    return Some(self.dispatch_handled());
                }
            }
        }

        // --- <-prefix resolution ---
        if self.last_key == "<" {
            if let Some(ch) = unicode_char {
                if ch == '<' {
                    // << unindent
                    self.send_keys("<LT><LT>");
                    self.clear_last_key();
                    if self.recording_macro.is_some() && !self.playing_macro {
                        self.macro_buffer.push("<<".to_string());
                    }
                    return Some(self.dispatch_handled());
                } else {
                    // < + motion
                    self.send_keys(&format!("<LT>{}", ch));
                    self.clear_last_key();
                    return Some(self.dispatch_handled());
                }
            }
        }

        // --- gq-prefix resolution ---
        if self.last_key == "gq" {
            if keycode == Key::Q && !key_event.is_shift_pressed() {
                self.send_keys("gqq");
                self.clear_last_key();
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer.push("gqq".to_string());
                }
                return Some(self.dispatch_handled());
            }
            // Other key after gq: gq + motion
            if let Some(keys) = self.key_event_to_nvim_string(key_event) {
                self.send_keys(&format!("gq{}", keys));
                self.clear_last_key();
                return Some(self.dispatch_handled());
            }
        }

        None
    }

    // =====================================================================
    // Helper: Handle register-aware operations ("ayy, "ap, etc.)
    // =====================================================================
    fn handle_register_operations(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> Option<VarDictionary> {
        let reg = self.selected_register?;
        if reg == '\0' {
            return None; // Waiting for register char - handled by mode_handler
        }

        let keycode = key_event.get_keycode();
        let unicode_char = char::from_u32(key_event.get_unicode());

        // Count prefix within register context
        if let Some(c) = unicode_char {
            if c.is_ascii_digit() && (c != '0' || !self.count_buffer.is_empty()) {
                self.count_buffer.push(c);
                return Some(self.dispatch_handled());
            }
        }

        // yy - yank line to register
        if keycode == Key::Y && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.last_key == "y" {
                let count = self.get_and_clear_count();
                let count_str = if count > 1 {
                    count.to_string()
                } else {
                    String::new()
                };
                self.send_keys(&format!("\"{}{}yy", reg, count_str));
                self.selected_register = None;
                self.clear_last_key();
                return Some(self.dispatch_handled());
            } else {
                self.set_last_key("y");
                return Some(self.dispatch_handled());
            }
        }

        // p - paste from register
        if keycode == Key::P && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys(&format!("\"{}p", reg));
            self.selected_register = None;
            self.count_buffer.clear();
            return Some(self.dispatch_handled());
        }

        // P - paste before from register
        if keycode == Key::P && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys(&format!("\"{}P", reg));
            self.selected_register = None;
            self.count_buffer.clear();
            return Some(self.dispatch_handled());
        }

        // dd - delete line to register
        if keycode == Key::D && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.last_key == "d" {
                let count = self.get_and_clear_count();
                let count_str = if count > 1 {
                    count.to_string()
                } else {
                    String::new()
                };
                self.send_keys(&format!("\"{}{}dd", reg, count_str));
                self.selected_register = None;
                self.clear_last_key();
                return Some(self.dispatch_handled());
            } else {
                self.set_last_key("d");
                return Some(self.dispatch_handled());
            }
        }

        // cc - change line to register
        if keycode == Key::C && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.last_key == "c" {
                let count = self.get_and_clear_count();
                let count_str = if count > 1 {
                    count.to_string()
                } else {
                    String::new()
                };
                self.send_keys(&format!("\"{}{}cc", reg, count_str));
                self.selected_register = None;
                self.clear_last_key();
                return Some(self.dispatch_handled());
            } else {
                self.set_last_key("c");
                return Some(self.dispatch_handled());
            }
        }

        // Operator + motion with register (y/d/c + motion)
        if let Some(keys) = self.key_event_to_nvim_string(key_event) {
            if self.last_key == "y" && keycode != Key::Y {
                let count = self.get_and_clear_count();
                let count_str = if count > 1 {
                    count.to_string()
                } else {
                    String::new()
                };
                self.send_keys(&format!("\"{}{}y{}", reg, count_str, keys));
                self.selected_register = None;
                self.clear_last_key();
                return Some(self.dispatch_handled());
            }
            if self.last_key == "d" && keycode != Key::D {
                let count = self.get_and_clear_count();
                let count_str = if count > 1 {
                    count.to_string()
                } else {
                    String::new()
                };
                self.send_keys(&format!("\"{}{}d{}", reg, count_str, keys));
                self.selected_register = None;
                self.clear_last_key();
                return Some(self.dispatch_handled());
            }
            if self.last_key == "c" && keycode != Key::C {
                let count = self.get_and_clear_count();
                let count_str = if count > 1 {
                    count.to_string()
                } else {
                    String::new()
                };
                self.send_keys(&format!("\"{}{}c{}", reg, count_str, keys));
                self.selected_register = None;
                self.clear_last_key();
                return Some(self.dispatch_handled());
            }
        }

        // Other keys cancel register selection
        if keycode != Key::Y && keycode != Key::D && keycode != Key::C {
            self.selected_register = None;
            self.count_buffer.clear();
        }

        None
    }

    // =====================================================================
    // Helper: Handle keys with Godot side effects (internal handling)
    // =====================================================================
    fn handle_side_effect_keys(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> Option<VarDictionary> {
        let keycode = key_event.get_keycode();
        let unicode_char = char::from_u32(key_event.get_unicode());

        // ';' - repeat find char same direction
        if keycode == Key::SEMICOLON && !key_event.is_shift_pressed() {
            self.repeat_find_char(true);
            self.send_keys(";");
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push(";".to_string());
            }
            return Some(self.dispatch_handled());
        }

        // ',' - repeat find char opposite direction
        if keycode == Key::COMMA && !key_event.is_shift_pressed() {
            self.repeat_find_char(false);
            self.send_keys(",");
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push(",".to_string());
            }
            return Some(self.dispatch_handled());
        }

        // '%' - matching bracket
        if unicode_char == Some('%') {
            self.jump_to_matching_bracket();
            self.send_keys("%");
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push("%".to_string());
            }
            return Some(self.dispatch_handled());
        }

        // '0' - go to start of line (only when not part of a count, not after g)
        if unicode_char == Some('0') && !key_event.is_ctrl_pressed() && self.last_key != "g" {
            self.move_to_line_start();
            self.send_keys("0");
            return Some(self.dispatch_handled());
        }

        // '^' - go to first non-blank (not after g)
        if unicode_char == Some('^') && self.last_key != "g" {
            self.move_to_first_non_blank();
            self.send_keys("^");
            return Some(self.dispatch_handled());
        }

        // '$' - go to end of line (not after g)
        if unicode_char == Some('$') && self.last_key != "g" {
            self.move_to_line_end();
            self.send_keys("$");
            return Some(self.dispatch_handled());
        }

        // 'J' - join lines (not after g - that's gJ)
        if keycode == Key::J
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            self.send_keys("J");
            return Some(self.dispatch_handled());
        }

        // H/M/L - viewport-relative movement
        if !key_event.is_ctrl_pressed()
            && !key_event.is_alt_pressed()
            && (keycode == Key::H || keycode == Key::M || keycode == Key::L)
            && key_event.is_shift_pressed()
        {
            let key_str = match keycode {
                Key::H => "H",
                Key::M => "M",
                Key::L => "L",
                _ => unreachable!(),
            };
            self.send_keys(key_str);
            return Some(self.dispatch_handled());
        }

        // 'R' - enter replace mode
        if keycode == Key::R && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push("R".to_string());
            }
            self.enter_replace_mode();
            return Some(self.dispatch_handled());
        }

        None
    }

    // =====================================================================
    // Helper: Handle keys that set up pending states
    // =====================================================================
    fn handle_pending_state_setup(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> Option<VarDictionary> {
        let keycode = key_event.get_keycode();
        let unicode_char = char::from_u32(key_event.get_unicode());

        // 'f' - find char forward (not after g/i/a prefix)
        if keycode == Key::F
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
            && self.last_key != "i"
            && self.last_key != "a"
        {
            self.clear_pending_input_states();
            self.pending_char_op = Some('f');
            return Some(self.dispatch_handled());
        }

        // 'F' - find char backward (not after i/a prefix)
        if keycode == Key::F
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "i"
            && self.last_key != "a"
        {
            self.clear_pending_input_states();
            self.pending_char_op = Some('F');
            return Some(self.dispatch_handled());
        }

        // 't' - till char forward (not after g/z/i/a prefix)
        if keycode == Key::T
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
            && self.last_key != "z"
            && self.last_key != "i"
            && self.last_key != "a"
        {
            self.clear_pending_input_states();
            self.pending_char_op = Some('t');
            return Some(self.dispatch_handled());
        }

        // 'T' - till char backward (not after g/i/a prefix)
        if keycode == Key::T
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
            && self.last_key != "i"
            && self.last_key != "a"
        {
            self.clear_pending_input_states();
            self.pending_char_op = Some('T');
            return Some(self.dispatch_handled());
        }

        // 'r' - replace char
        if keycode == Key::R && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
            self.pending_char_op = Some('r');
            return Some(self.dispatch_handled());
        }

        // 'm' - set mark
        if keycode == Key::M && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
            self.pending_mark_op = Some('m');
            return Some(self.dispatch_handled());
        }

        // '\'' - jump to mark line (not in operator-pending or visual mode)
        if unicode_char == Some('\'')
            && !key_event.is_ctrl_pressed()
            && self.current_mode != "operator"
            && !Self::is_visual_mode(&self.current_mode)
        {
            self.clear_pending_input_states();
            self.pending_mark_op = Some('\'');
            return Some(self.dispatch_handled());
        }

        // '`' - jump to mark position (not in operator-pending or visual mode)
        if unicode_char == Some('`')
            && !key_event.is_ctrl_pressed()
            && self.current_mode != "operator"
            && !Self::is_visual_mode(&self.current_mode)
        {
            self.clear_pending_input_states();
            self.pending_mark_op = Some('`');
            return Some(self.dispatch_handled());
        }

        // 'q' - macro recording start/stop (not after g, not with AltGr)
        let input = Input::singleton();
        let is_altgr_held = input.is_key_pressed(Key::CTRL) && input.is_key_pressed(Key::ALT);
        if keycode == Key::Q
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && !is_altgr_held
            && self.last_key != "g"
        {
            if self.recording_macro.is_some() {
                self.stop_macro_recording();
            } else {
                self.clear_pending_input_states();
                self.pending_macro_op = Some('q');
            }
            return Some(self.dispatch_handled());
        }

        // '@' - macro playback
        if unicode_char == Some('@') && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
            self.pending_macro_op = Some('@');
            return Some(self.dispatch_handled());
        }

        // '"' - register selection (not in operator-pending or visual mode)
        if unicode_char == Some('"')
            && !key_event.is_ctrl_pressed()
            && self.current_mode != "operator"
            && !Self::is_visual_mode(&self.current_mode)
        {
            self.clear_pending_input_states();
            self.clear_last_key();
            self.selected_register = Some('\0');
            return Some(self.dispatch_handled());
        }

        None
    }

    // =====================================================================
    // Helper: Handle operator keys (>, <)
    // =====================================================================
    fn handle_operator_keys(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> Option<VarDictionary> {
        let unicode_char = char::from_u32(key_event.get_unicode());

        // '>' - indent operator
        if unicode_char == Some('>') {
            if self.last_key == ">" {
                self.send_keys(">>");
                self.clear_last_key();
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer.push(">>".to_string());
                }
            } else {
                self.set_last_key(">");
            }
            return Some(self.dispatch_handled());
        }

        // '<' - unindent operator
        if unicode_char == Some('<') {
            if self.last_key == "<" {
                self.send_keys("<LT><LT>");
                self.clear_last_key();
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer.push("<<".to_string());
                }
            } else {
                self.set_last_key("<");
            }
            return Some(self.dispatch_handled());
        }

        None
    }

    // =====================================================================
    // Helper: Prefix key accumulation (g, [, ], Z)
    // =====================================================================
    fn handle_prefix_key_accumulation(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) -> Option<VarDictionary> {
        let keycode = key_event.get_keycode();
        let unicode_char = char::from_u32(key_event.get_unicode());

        // 'g' prefix (not after another g - allow gg)
        if unicode_char == Some('g')
            && !key_event.is_ctrl_pressed()
            && !key_event.is_shift_pressed()
            && self.last_key != "g"
        {
            self.set_last_key("g");
            return Some(self.dispatch_handled());
        }

        // '[' prefix (not after [ or ])
        if keycode == Key::BRACKETLEFT
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "["
            && self.last_key != "]"
        {
            self.set_last_key("[");
            return Some(self.dispatch_handled());
        }

        // ']' prefix (not after [ or ])
        if keycode == Key::BRACKETRIGHT
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "["
            && self.last_key != "]"
        {
            self.set_last_key("]");
            return Some(self.dispatch_handled());
        }

        // 'Z' prefix (Shift+Z)
        if keycode == Key::Z
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "Z"
        {
            self.set_last_key("Z");
            return Some(self.dispatch_handled());
        }

        None
    }

    // =====================================================================
    // Get Neovim keymaps via RPC
    // =====================================================================
    pub(in crate::plugin) fn get_neovim_keymaps_impl(&self, mode: &str) -> VarDictionary {
        let mut dict = VarDictionary::new();

        let Some(neovim_mutex) = self.get_current_neovim() else {
            return dict;
        };

        let Ok(neovim) = neovim_mutex.lock() else {
            return dict;
        };

        // Use exec_lua to get keymaps and return a simplified table
        let lua_code = r#"
            local mode = ...
            local maps = vim.api.nvim_get_keymap(mode)
            local result = {}
            for _, map in ipairs(maps) do
                if map.lhs and map.rhs then
                    result[map.lhs] = map.rhs
                end
            end
            return result
        "#;

        let result = neovim.execute_lua_with_args(lua_code, vec![rmpv::Value::from(mode)]);

        if let Ok(rmpv::Value::Map(entries)) = result {
            // Convert rmpv::Value (Map) to VarDictionary
            for (k, v) in entries {
                if let (rmpv::Value::String(lhs), rmpv::Value::String(rhs)) = (k, v) {
                    if let (Some(lhs_str), Some(rhs_str)) = (lhs.as_str(), rhs.as_str()) {
                        dict.set(GString::from(lhs_str), GString::from(rhs_str));
                    }
                }
            }
        }

        dict
    }
}
