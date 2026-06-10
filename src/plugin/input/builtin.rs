//! Built-in keymap dispatch.
//!
//! Fallback used when no GDScript input handler is registered (e.g. the
//! addon's GDScript half failed to load). Mirrors the lookup performed by
//! godot_neovim_input.gd::_on_dispatch with the tables from
//! default_keymaps.gd: resolved keys map to the same action implementations,
//! and unmapped keys are forwarded to Neovim verbatim.
//!
//! Keep these tables in sync with addons/godot-neovim/input/default_keymaps.gd.

use super::super::GodotNeovimPlugin;

impl GodotNeovimPlugin {
    /// Resolve a dispatched key against the built-in default keymap and run
    /// the action. `mode` and `resolved_key` come from the VarDictionary
    /// returned by process_*_key_event_impl.
    pub(in crate::plugin) fn dispatch_key_builtin(&mut self, mode: &str, resolved_key: &str) {
        // Insert/replace: every default binding forwards to Neovim, as does
        // the unmapped fallback, so the whole mode collapses to send_keys.
        if matches!(mode, "i" | "insert" | "R" | "replace") {
            self.action_send_keys_impl(resolved_key);
            return;
        }

        if Self::is_visual_mode(mode) {
            match resolved_key {
                "<C-b>" | "gv" => self.action_visual_block_toggle_impl(),
                "<C-f>" => self.action_page_down_impl(),
                "<C-d>" => self.action_half_page_down_impl(),
                "<C-u>" => self.action_half_page_up_impl(),
                "<C-y>" => self.action_scroll_viewport_up_impl(),
                "<C-e>" => self.action_scroll_viewport_down_impl(),
                "/" => self.action_open_search_forward_impl(),
                "?" => self.action_open_search_backward_impl(),
                "n" => self.action_search_next_impl(),
                "N" => self.action_search_prev_impl(),
                "*" => self.action_search_word_forward_impl(),
                "#" => self.action_search_word_backward_impl(),
                ":" => self.action_open_command_line_impl(),
                "gj" => self.action_display_line_down_impl(),
                "gk" => self.action_display_line_up_impl(),
                _ => self.action_send_keys_impl(resolved_key),
            }
            return;
        }

        // Normal mode. Unknown modes (e.g. operator-pending) also land here,
        // matching godot_neovim_input.gd::_get_keymap_for_mode's fallback.
        match resolved_key {
            "<C-b>" => self.action_page_up_impl(),
            "<C-f>" => self.action_page_down_impl(),
            "<C-d>" => self.action_half_page_down_impl(),
            "<C-u>" => self.action_half_page_up_impl(),
            "<C-y>" => self.action_scroll_viewport_up_impl(),
            "<C-e>" => self.action_scroll_viewport_down_impl(),
            "<C-a>" => self.action_increment_impl(),
            "<C-x>" => self.action_decrement_impl(),
            "<C-o>" => self.action_jump_back_impl(),
            "<C-i>" => self.action_jump_forward_impl(),
            "<C-g>" => self.action_show_file_info_impl(),
            "/" => self.action_open_search_forward_impl(),
            "?" => self.action_open_search_backward_impl(),
            "n" => self.action_search_next_impl(),
            "N" => self.action_search_prev_impl(),
            "*" => self.action_search_word_forward_impl(),
            "#" => self.action_search_word_backward_impl(),
            ":" => self.action_open_command_line_impl(),
            "u" => self.action_undo_impl(),
            "<C-r>" => self.action_redo_impl(),
            "K" => self.action_open_documentation_impl(),
            "gd" => self.action_goto_definition_impl(),
            "gf" => self.action_goto_file_impl(),
            "gx" => self.action_open_url_impl(),
            "gt" => self.action_next_tab_impl(),
            "gT" => self.action_prev_tab_impl(),
            "gv" => self.action_visual_block_toggle_impl(),
            "gj" => self.action_display_line_down_impl(),
            "gk" => self.action_display_line_up_impl(),
            "gI" => self.action_insert_at_column_zero_impl(),
            "gi" => self.action_insert_at_last_position_impl(),
            "ga" => self.action_show_char_info_impl(),
            "g&" => self.action_repeat_substitution_impl(),
            "gJ" => self.action_join_no_space_impl(),
            "gp" => self.action_paste_move_cursor_impl(),
            "gP" => self.action_paste_before_move_cursor_impl(),
            "ge" => self.action_word_end_backward_impl(),
            "g0" => self.action_display_line_start_impl(),
            "g$" => self.action_display_line_end_impl(),
            "g^" => self.action_display_line_first_non_blank_impl(),
            "ZZ" => self.action_save_and_close_impl(),
            "ZQ" => self.action_close_discard_impl(),
            _ => self.action_send_keys_impl(resolved_key),
        }
    }
}
