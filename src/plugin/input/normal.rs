//! Normal mode input handling
//!
//! This is the largest input handler, handling all normal mode key sequences
//! including g-prefix commands, [/] bracket commands, z-commands, etc.

use super::super::GodotNeovimPlugin;
use godot::classes::Input;
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    pub(in crate::plugin) fn handle_normal_mode_input(
        &mut self,
        key_event: &Gd<godot::classes::InputEventKey>,
    ) {
        let keycode = key_event.get_keycode();
        let unicode_char = char::from_u32(key_event.get_unicode());

        // Handle Ctrl+B: visual block in visual mode, page up in normal mode
        if key_event.is_ctrl_pressed() && keycode == Key::B {
            if Self::is_visual_mode(&self.current_mode) {
                self.action_visual_block_toggle_impl();
            } else {
                self.action_page_up_impl();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'o' in visual mode: toggle selection direction
        if Self::is_visual_mode(&self.current_mode)
            && keycode == Key::O
            && !key_event.is_ctrl_pressed()
            && !key_event.is_shift_pressed()
        {
            // Send 'o' to Neovim to toggle selection direction
            self.send_keys("o");
            // Update selection display (Neovim will swap anchor and cursor)
            if self.current_mode == "v" {
                self.update_visual_selection();
            } else if self.current_mode == "V" {
                self.update_visual_line_selection();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            crate::verbose_print!("[godot-neovim] o: Toggle visual selection direction");
            return;
        }

        // Handle Ctrl+F for page down
        if key_event.is_ctrl_pressed() && keycode == Key::F {
            self.action_page_down_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+Y/Ctrl+E for viewport scrolling (cursor stays on same line)
        if key_event.is_ctrl_pressed() && (keycode == Key::Y || keycode == Key::E) {
            if keycode == Key::Y {
                self.action_scroll_viewport_up_impl();
            } else {
                self.action_scroll_viewport_down_impl();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+A for increment number under cursor
        if key_event.is_ctrl_pressed() && keycode == Key::A {
            self.action_increment_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+X for decrement number under cursor
        if key_event.is_ctrl_pressed() && keycode == Key::X {
            self.action_decrement_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+O for jump back in jump list
        if key_event.is_ctrl_pressed() && keycode == Key::O {
            self.action_jump_back_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+I (Tab) for jump forward in jump list
        if key_event.is_ctrl_pressed() && keycode == Key::I {
            self.action_jump_forward_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+G for file info
        if key_event.is_ctrl_pressed() && keycode == Key::G {
            self.action_show_file_info_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '/' for forward search mode
        if unicode_char == Some('/') && !key_event.is_ctrl_pressed() {
            self.action_open_search_forward_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '?' for backward search mode
        if unicode_char == Some('?') && !key_event.is_ctrl_pressed() {
            self.action_open_search_backward_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ':' for command-line mode
        if unicode_char == Some(':') {
            self.action_open_command_line_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '*' for search forward word under cursor
        if unicode_char == Some('*') {
            self.action_search_word_forward_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '#' for search backward word under cursor
        if unicode_char == Some('#') {
            self.action_search_word_backward_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'n' for repeat search forward
        if keycode == Key::N && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.action_search_next_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'N' for repeat search backward
        if keycode == Key::N && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.action_search_prev_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'u' for undo (but not after 'g' - that's 'gu' for lowercase)
        if keycode == Key::U
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            self.action_undo_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'Ctrl+R' for redo
        if keycode == Key::R && key_event.is_ctrl_pressed() {
            self.action_redo_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'f' for find char forward (but not after 'g' - that's 'gf' for go to file,
        // and not after 'i'/'a' - that's text object selection like 'vif')
        if keycode == Key::F
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
            && self.last_key != "i"
            && self.last_key != "a"
        {
            self.clear_pending_input_states();
            self.pending_char_op = Some('f');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'F' for find char backward (not after 'i'/'a' - text object selection)
        if keycode == Key::F
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "i"
            && self.last_key != "a"
        {
            self.clear_pending_input_states();
            self.pending_char_op = Some('F');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 't' for till char forward (but not after 'g' - that's gt for tab navigation,
        // not after 'z' - that's zt for scroll cursor to top,
        // and not after 'i'/'a' - that's text object selection like 'vit')
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
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'T' for till char backward (but not after 'g' - that's gT for tab navigation,
        // and not after 'i'/'a' - text object selection)
        if keycode == Key::T
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
            && self.last_key != "i"
            && self.last_key != "a"
        {
            self.clear_pending_input_states();
            self.pending_char_op = Some('T');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ';' for repeat find char same direction
        if keycode == Key::SEMICOLON && !key_event.is_shift_pressed() {
            self.repeat_find_char(true);
            self.send_keys(";");
            // Record to local macro buffer (early return skips normal recording)
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push(";".to_string());
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ',' for repeat find char opposite direction
        if keycode == Key::COMMA && !key_event.is_shift_pressed() {
            self.repeat_find_char(false);
            self.send_keys(",");
            // Record to local macro buffer (early return skips normal recording)
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push(",".to_string());
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '%' for matching bracket
        if unicode_char == Some('%') {
            self.jump_to_matching_bracket();
            self.send_keys("%");
            // Record to local macro buffer (early return skips normal recording)
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push("%".to_string());
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle count prefix (1-9, or 0 if count_buffer not empty)
        // This tracks the count locally while also sending to Neovim
        if let Some(c) = unicode_char {
            if c.is_ascii_digit() && (c != '0' || !self.count_buffer.is_empty()) {
                self.count_buffer.push(c);
                self.send_keys(&c.to_string());
                // Reset timeout to prevent <Esc> being sent during count input
                self.last_key_time = Some(std::time::Instant::now());
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
        }

        // Handle '0' for go to start of line (only when not part of a count)
        // Skip if last_key is "g" (g0 is handled separately for display line)
        if unicode_char == Some('0') && !key_event.is_ctrl_pressed() && self.last_key != "g" {
            self.move_to_line_start();
            self.send_keys("0"); // Also send to Neovim
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '^' for go to first non-blank
        // Skip if last_key is "g" (g^ is handled separately for display line)
        if unicode_char == Some('^') && self.last_key != "g" {
            self.move_to_first_non_blank();
            self.send_keys("^"); // Also send to Neovim
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '$' for go to end of line
        // Skip if last_key is "g" (g$ is handled separately for display line)
        if unicode_char == Some('$') && self.last_key != "g" {
            self.move_to_line_end();
            self.send_keys("$"); // Also send to Neovim
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '{' for previous paragraph (send to Neovim for proper cursor positioning)
        // Skip if last_key is '[' or ']' - these are [{ / ]{ commands handled later
        if unicode_char == Some('{') && self.last_key != "[" && self.last_key != "]" {
            self.send_keys("{");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '}' for next paragraph (send to Neovim for proper cursor positioning)
        // Skip if last_key is '[' or ']' - these are [} / ]} commands handled later
        if unicode_char == Some('}') && self.last_key != "[" && self.last_key != "]" {
            self.send_keys("}");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'x' for delete char under cursor (but not after 'g' - that's 'gx' for open URL)
        // Neovim Master: send to Neovim only, reflect via nvim_buf_lines_event
        if keycode == Key::X
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            self.send_keys("x");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'X' for delete char before cursor
        // Neovim Master: send to Neovim only
        if keycode == Key::X && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys("X");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'Y' for yank to end of line
        // Neovim Master: send to Neovim only
        if keycode == Key::Y && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys("Y");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'D' for delete to end of line
        // Neovim Master: send to Neovim only, reflect via nvim_buf_lines_event
        if keycode == Key::D && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys("D");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'C' for change to end of line
        // Neovim Master: send to Neovim only
        if keycode == Key::C && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys("C");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 's' for substitute char (delete char and enter insert mode)
        if keycode == Key::S && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.action_send_keys_impl("s");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'S' for substitute line (delete line content and enter insert mode)
        if keycode == Key::S && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.action_send_keys_impl("S");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // 'c' operator: send directly to Neovim for proper operator-pending mode
        // This allows 'ci(', 'cw', 'cc', etc. to work correctly
        // Neovim handles operator-pending mode and text objects natively

        // Handle 'r' for replace char
        if keycode == Key::R && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
            self.pending_char_op = Some('r');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'R' for replace mode (continuous overwrite)
        if keycode == Key::R && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push("R".to_string());
            }
            self.enter_replace_mode();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '~' for toggle case
        if unicode_char == Some('~') {
            self.action_send_keys_impl("~");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'm' for set mark
        if keycode == Key::M && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
            self.pending_mark_op = Some('m');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '\'' (single quote) for jump to mark line
        // Skip if in operator-pending mode (e.g., ci' should send ' to Neovim as text object)
        // Skip if in visual mode (e.g., vi' should select inside quotes)
        if unicode_char == Some('\'')
            && !key_event.is_ctrl_pressed()
            && self.current_mode != "operator"
            && !Self::is_visual_mode(&self.current_mode)
        {
            self.clear_pending_input_states();
            self.pending_mark_op = Some('\'');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '`' (backtick) for jump to mark position
        // Skip if in operator-pending mode (e.g., ci` should send ` to Neovim as text object)
        // Skip if in visual mode (e.g., vi` should select inside backticks)
        if unicode_char == Some('`')
            && !key_event.is_ctrl_pressed()
            && self.current_mode != "operator"
            && !Self::is_visual_mode(&self.current_mode)
        {
            self.clear_pending_input_states();
            self.pending_mark_op = Some('`');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'q' for macro recording (start/stop) - but not after 'g' (that's gq for format)
        // Also skip if AltGr is pressed (Ctrl+Alt on German keyboards for '@')
        // Note: Use Input::singleton() to check actual key state because AltGr sends
        // separate Ctrl and Alt events, and key_event modifiers may not be set correctly
        let input = Input::singleton();
        let is_altgr_held = input.is_key_pressed(Key::CTRL) && input.is_key_pressed(Key::ALT);
        if keycode == Key::Q
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && !is_altgr_held
            && self.last_key != "g"
        {
            if self.recording_macro.is_some() {
                // Stop recording
                self.stop_macro_recording();
            } else {
                // Wait for register character
                self.clear_pending_input_states();
                self.pending_macro_op = Some('q');
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '@' for macro playback
        if unicode_char == Some('@') && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
            self.pending_macro_op = Some('@');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '"' for register selection
        // Skip if in operator-pending mode (e.g., ci" should send " to Neovim as text object)
        // Skip if in visual mode (e.g., vi" should select inside quotes)
        if unicode_char == Some('"')
            && !key_event.is_ctrl_pressed()
            && self.current_mode != "operator"
            && !Self::is_visual_mode(&self.current_mode)
        {
            // Use '\0' as marker for "waiting for register char"
            self.clear_pending_input_states();
            // Clear last_key to prevent timeout from clearing selected_register
            self.clear_last_key();
            self.selected_register = Some('\0');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '>' operator with motion/text object (>iw, >i{, >aw, etc.)
        // When '>' is pending and next key is not '>', send '>' + key to Neovim
        if self.last_key == ">" {
            if let Some(ch) = unicode_char {
                if ch != '>' {
                    self.send_keys(&format!(">{}", ch));
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
            }
        }

        // Handle '<' operator with motion/text object (<iw, <i{, <aw, etc.)
        // When '<' is pending and next key is not '<', send '<LT>' + key to Neovim
        // Use <LT> because nvim_input interprets < as special key sequence start
        if self.last_key == "<" {
            if let Some(ch) = unicode_char {
                if ch != '<' {
                    self.send_keys(&format!("<LT>{}", ch));
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
            }
        }

        // Handle '>>' for indent (first '>' sets pending, second '>' executes)
        // Handle '<<' for unindent (first '<' sets pending, second '<' executes)
        // Neovim Master: send to Neovim for proper undo/register integration
        if unicode_char == Some('>') {
            if self.last_key == ">" {
                self.send_keys(">>");
                self.clear_last_key();
            } else {
                self.set_last_key(">");
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        if unicode_char == Some('<') {
            if self.last_key == "<" {
                // Use <LT><LT> because nvim_input interprets < as special key sequence start
                self.send_keys("<LT><LT>");
                self.clear_last_key();
            } else {
                self.set_last_key("<");
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'g' prefix - don't send to Neovim yet, wait for next key
        // (like '[' and ']' prefixes)
        // Note: Skip if last_key is already "g" to allow 'gg' to be processed
        if unicode_char == Some('g')
            && !key_event.is_ctrl_pressed()
            && !key_event.is_shift_pressed()
            && self.last_key != "g"
        {
            self.set_last_key("g");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '[' prefix - don't send to Neovim yet, wait for next key
        // Use keycode for keyboard layout independence (JP keyboard may have different unicode)
        // Skip if last_key is already '[' or ']' (to allow [[, ]], [], ][ sequences)
        if keycode == Key::BRACKETLEFT
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "["
            && self.last_key != "]"
        {
            self.set_last_key("[");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ']' prefix - don't send to Neovim yet, wait for next key
        // Use keycode for keyboard layout independence (JP keyboard may have different unicode)
        // Skip if last_key is already '[' or ']' (to allow [[, ]], [], ][ sequences)
        if keycode == Key::BRACKETRIGHT
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "["
            && self.last_key != "]"
        {
            self.set_last_key("]");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle p after [ or ]
        // Neovim Master: send to Neovim for proper undo/register integration
        if keycode == Key::P && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.last_key == "[" {
                self.send_keys("[p");
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            } else if self.last_key == "]" {
                self.send_keys("]p");
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
        }

        // Handle '?' for backward search
        if unicode_char == Some('?') && !key_event.is_ctrl_pressed() {
            self.start_search_backward();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'K' for documentation lookup
        if keycode == Key::K && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.action_open_documentation_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '[' commands
        // Use keycode for keyboard layout independence (JP keyboard support)
        if self.last_key == "[" {
            // [[ - jump to previous '{' at start of line (send to Neovim)
            if keycode == Key::BRACKETLEFT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.send_keys("[[");
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
            // [] - jump to previous '}' at start of line (send to Neovim)
            if keycode == Key::BRACKETRIGHT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.send_keys("[]");
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
            match unicode_char {
                Some('{') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("[{");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some('(') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("[(");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some('m') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("[m");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some('\0') | None => {
                    // Modifier-only key (SHIFT, etc.) or NUL char - don't clear last_key
                }
                _ => {
                    // Not a recognized [ command, clear and continue
                    self.clear_last_key();
                }
            }
        }

        // Handle ']' commands
        // Use keycode for keyboard layout independence (JP keyboard support)
        if self.last_key == "]" {
            // ]] - jump to next '{' at start of line (send to Neovim)
            if keycode == Key::BRACKETRIGHT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.send_keys("]]");
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
            // ][ - jump to next '}' at start of line (send to Neovim)
            if keycode == Key::BRACKETLEFT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.send_keys("][");
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
            match unicode_char {
                Some('}') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("]}");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some(')') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("])");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some('m') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("]m");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some('\0') | None => {
                    // Modifier-only key (SHIFT, etc.) or NUL char - don't clear last_key
                }
                _ => {
                    // Not a recognized ] command, clear and continue
                    self.clear_last_key();
                }
            }
        }

        // Handle gqq (format current line)
        // Neovim Master: send to Neovim for proper undo/register integration
        if self.last_key == "gq" && keycode == Key::Q && !key_event.is_shift_pressed() {
            self.send_keys("gqq");
            self.clear_last_key();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'J' for join lines - send to Neovim (Neovim Master design)
        // Neovim will process the join and send buffer changes via nvim_buf_lines_event
        // Note: Skip if last_key is "g" to allow 'gJ' to be processed in g-prefix block
        if keycode == Key::J
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            self.send_keys("J");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+D for half page down
        if key_event.is_ctrl_pressed() && keycode == Key::D {
            self.action_half_page_down_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+U for half page up
        if key_event.is_ctrl_pressed() && keycode == Key::U {
            self.action_half_page_up_impl();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle H/M/L based on Godot's visible area (not Neovim's)
        if !key_event.is_ctrl_pressed()
            && !key_event.is_alt_pressed()
            && (keycode == Key::H || keycode == Key::M || keycode == Key::L)
            && key_event.is_shift_pressed()
        {
            // H/M/L are valid motions in all contexts:
            // - Normal mode: move cursor
            // - Visual mode: extend selection
            // - Operator-pending mode (d, c, y + H/M/L): complete the operation
            // Do NOT cancel pending operator - let Neovim handle it
            // Shift+h/m/l = H/M/L (uppercase) - send to Neovim for viewport-aware handling
            match keycode {
                Key::H => {
                    self.send_keys("H");
                }
                Key::M => {
                    self.send_keys("M");
                }
                Key::L => {
                    self.send_keys("L");
                }
                _ => {}
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Z-prefixed commands (ZZ, ZQ)
        if keycode == Key::Z && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.last_key == "Z" {
                // Second Z - this is ZZ (save and close)
                self.action_save_and_close_impl();
                self.clear_last_key();
            } else {
                // First Z - wait for next key
                self.set_last_key("Z");
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ZQ (Z then Q) - close without saving
        if keycode == Key::Q
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key == "Z"
        {
            self.action_close_discard_impl();
            self.clear_last_key();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Clear Z prefix if another key is pressed (not Z or Q)
        if self.last_key == "Z" && keycode != Key::Z && keycode != Key::Q {
            self.clear_last_key();
        }

        // Handle register-aware yy (yank line)
        if let Some(reg) = self.selected_register {
            if reg != '\0' {
                // Handle count prefix (digits 1-9, or 0 if count_buffer not empty)
                if let Some(c) = unicode_char {
                    if c.is_ascii_digit() && (c != '0' || !self.count_buffer.is_empty()) {
                        self.count_buffer.push(c);
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                }

                // Register is selected, check for yy
                // Neovim Master: send to Neovim for proper undo/register integration
                if keycode == Key::Y
                    && !key_event.is_shift_pressed()
                    && !key_event.is_ctrl_pressed()
                {
                    if self.last_key == "y" {
                        // yy - yank current line(s) to register
                        let count = self.get_and_clear_count();
                        let count_str = if count > 1 {
                            count.to_string()
                        } else {
                            String::new()
                        };
                        self.send_keys(&format!("\"{}{}yy", reg, count_str));
                        self.selected_register = None;
                        self.clear_last_key();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    } else {
                        // First y - wait for second
                        self.set_last_key("y");
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                }

                // Handle register-aware p (paste)
                // Neovim Master: send to Neovim for proper undo/register integration
                if keycode == Key::P
                    && !key_event.is_shift_pressed()
                    && !key_event.is_ctrl_pressed()
                {
                    self.send_keys(&format!("\"{}p", reg));
                    self.selected_register = None;
                    self.count_buffer.clear();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }

                // Handle register-aware P (paste before)
                // Neovim Master: send to Neovim for proper undo/register integration
                if keycode == Key::P && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed()
                {
                    self.send_keys(&format!("\"{}P", reg));
                    self.selected_register = None;
                    self.count_buffer.clear();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }

                // Handle register-aware dd (delete line and yank)
                // Neovim Master: send to Neovim for proper undo/register integration
                if keycode == Key::D
                    && !key_event.is_shift_pressed()
                    && !key_event.is_ctrl_pressed()
                {
                    if self.last_key == "d" {
                        // dd - delete line(s) and store in register
                        let count = self.get_and_clear_count();
                        let count_str = if count > 1 {
                            count.to_string()
                        } else {
                            String::new()
                        };
                        self.send_keys(&format!("\"{}{}dd", reg, count_str));
                        self.selected_register = None;
                        self.clear_last_key();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    } else {
                        // First d - wait for second or motion
                        self.set_last_key("d");
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                }

                // Handle register-aware cc (change line)
                // Neovim Master: send to Neovim for proper undo/register integration
                if keycode == Key::C
                    && !key_event.is_shift_pressed()
                    && !key_event.is_ctrl_pressed()
                {
                    if self.last_key == "c" {
                        // cc - change line(s) and store in register
                        let count = self.get_and_clear_count();
                        let count_str = if count > 1 {
                            count.to_string()
                        } else {
                            String::new()
                        };
                        self.send_keys(&format!("\"{}{}cc", reg, count_str));
                        self.selected_register = None;
                        self.clear_last_key();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    } else {
                        // First c - wait for second or motion
                        self.set_last_key("c");
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                }

                // Handle operator + motion/text object with register (e.g., "adi(, "ayi(, "aci()
                // When last_key is an operator (y/d) and current key is a motion/text object,
                // send the full command to Neovim
                if let Some(keys) = self.key_event_to_nvim_string(key_event) {
                    if self.last_key == "y" && keycode != Key::Y {
                        // y + motion (e.g., yi(, yw, y$)
                        let count = self.get_and_clear_count();
                        let count_str = if count > 1 {
                            count.to_string()
                        } else {
                            String::new()
                        };
                        self.send_keys(&format!("\"{}{}y{}", reg, count_str, keys));
                        self.selected_register = None;
                        self.clear_last_key();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                    if self.last_key == "d" && keycode != Key::D {
                        // d + motion (e.g., di(, dw, d$)
                        let count = self.get_and_clear_count();
                        let count_str = if count > 1 {
                            count.to_string()
                        } else {
                            String::new()
                        };
                        self.send_keys(&format!("\"{}{}d{}", reg, count_str, keys));
                        self.selected_register = None;
                        self.clear_last_key();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                    if self.last_key == "c" && keycode != Key::C {
                        // c + motion (e.g., ci(, cw, c$)
                        let count = self.get_and_clear_count();
                        let count_str = if count > 1 {
                            count.to_string()
                        } else {
                            String::new()
                        };
                        self.send_keys(&format!("\"{}{}c{}", reg, count_str, keys));
                        self.selected_register = None;
                        self.clear_last_key();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                }

                // Other keys cancel register selection
                if keycode != Key::Y && keycode != Key::D && keycode != Key::C {
                    self.selected_register = None;
                    self.count_buffer.clear();
                }
            }
        }

        // Track visual mode type when entering visual mode
        // Neovim returns "visual" for all visual modes, so we track the key pressed
        if keycode == Key::V && !key_event.is_ctrl_pressed() {
            if key_event.is_shift_pressed() {
                // V (shift+V) - visual line mode
                self.visual_mode_type = 'V';
            } else {
                // v - visual char mode
                self.visual_mode_type = 'v';
            }
        }

        // Forward key to Neovim (normal/visual/etc modes)
        if let Some(keys) = self.key_event_to_nvim_string(key_event) {
            // Intercept g-prefix commands
            // Note: 'g' is NOT sent to Neovim when typed - we wait for the second key
            // and send the full command (like 'ge', 'gj', etc.) or 'g' + second key for unhandled commands
            if self.last_key == "g" {
                let handled = match keys.as_str() {
                    "x" => {
                        self.action_open_url_impl();
                        true
                    }
                    "f" => {
                        self.action_goto_file_impl();
                        true
                    }
                    "d" => {
                        self.action_goto_definition_impl();
                        true
                    }
                    "I" => {
                        self.action_insert_at_column_zero_impl();
                        true
                    }
                    "i" => {
                        self.action_insert_at_last_position_impl();
                        true
                    }
                    "a" => {
                        self.action_show_char_info_impl();
                        true
                    }
                    "&" => {
                        self.action_repeat_substitution_impl();
                        true
                    }
                    "J" => {
                        self.action_join_no_space_impl();
                        true
                    }
                    "p" => {
                        self.action_paste_move_cursor_impl();
                        true
                    }
                    "P" => {
                        self.action_paste_before_move_cursor_impl();
                        true
                    }
                    "e" => {
                        self.action_word_end_backward_impl();
                        true
                    }
                    "j" => {
                        self.action_display_line_down_impl();
                        true
                    }
                    "k" => {
                        self.action_display_line_up_impl();
                        true
                    }
                    "t" => {
                        self.action_next_tab_impl();
                        true
                    }
                    "T" => {
                        self.action_prev_tab_impl();
                        true
                    }
                    "v" => {
                        self.action_visual_block_toggle_impl();
                        true
                    }
                    "0" => {
                        self.action_display_line_start_impl();
                        true
                    }
                    "$" => {
                        self.action_display_line_end_impl();
                        true
                    }
                    "^" => {
                        self.action_display_line_first_non_blank_impl();
                        true
                    }
                    _ => {
                        // Unhandled g-command: send 'g' + second key to Neovim
                        // (e.g., gg, g_, etc.)
                        self.action_send_keys_impl(&format!("g{}", keys));
                        true
                    }
                };

                if handled {
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
            }

            // Record key for macro if recording (and not playing back)
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push(keys.clone());
            }

            let completed = self.send_keys(&keys);

            // Handle scroll commands (zz, zt, zb) only if command completed
            let scroll_handled = if completed {
                self.handle_scroll_command(&keys)
            } else {
                false
            };

            // Handle gq (format operator) - needs to wait for motion
            if completed && self.last_key == "g" && keys == "q" {
                self.set_last_key("gq");
                // Don't return - let normal key handling continue for motion
            }

            // Track last key for sequence detection, unless:
            // - scroll command was handled, or
            // - we entered insert/replace mode (no sequence expected in those modes)
            // Note: In visual mode, we still track 'i' and 'a' for text object selection (vit, vat, etc.)
            if !scroll_handled && !self.is_insert_mode() && !self.is_replace_mode() {
                // In visual mode, only track 'i' and 'a' for text object prefix
                if self.is_in_visual_mode() {
                    if keys == "i" || keys == "a" {
                        self.set_last_key(keys);
                    }
                } else {
                    self.set_last_key(keys);
                }
            }

            // Consume the event to prevent Godot's default handling
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
        }
    }
}
