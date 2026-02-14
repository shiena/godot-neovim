//! Action API: Methods callable from GDScript via #[func] wrappers in mod.rs
//!
//! These actions encapsulate the operations that were previously hardcoded in normal.rs.
//! Each action handles macro recording and delegates to internal methods.
//! GDScript keymaps can call these methods to implement custom key bindings.
//!
//! The #[func] wrappers are defined in mod.rs's #[godot_api] block because
//! godot-rs only allows one #[godot_api] impl block per struct.

use super::GodotNeovimPlugin;
use godot::prelude::*;

/// Helper macro to record a key to the macro buffer if recording
macro_rules! record_macro {
    ($self:expr, $key:expr) => {
        if $self.recording_macro.is_some() && !$self.playing_macro {
            $self.macro_buffer.push($key.to_string());
        }
    };
}

impl GodotNeovimPlugin {
    // =========================================================================
    // General key sending
    // =========================================================================

    /// Send arbitrary keys to Neovim (generic action for unmapped keys)
    pub(super) fn action_send_keys_impl(&mut self, keys: &str) {
        record_macro!(self, keys);
        self.send_keys(keys);
    }

    // =========================================================================
    // Undo / Redo
    // =========================================================================

    /// Undo (u)
    pub(super) fn action_undo_impl(&mut self) {
        record_macro!(self, "u");
        self.send_keys("u");
    }

    /// Redo (Ctrl+R)
    pub(super) fn action_redo_impl(&mut self) {
        record_macro!(self, "<C-r>");
        self.send_keys("<C-r>");
    }

    // =========================================================================
    // Page / Scroll navigation
    // =========================================================================

    /// Page up (Ctrl+B)
    pub(super) fn action_page_up_impl(&mut self) {
        self.cancel_pending_operator();
        self.pending_page_up_correction = true;
        record_macro!(self, "<C-b>");
        self.send_keys("<C-b>");
    }

    /// Page down (Ctrl+F)
    pub(super) fn action_page_down_impl(&mut self) {
        self.cancel_pending_operator();
        record_macro!(self, "<C-f>");
        self.send_keys("<C-f>");
    }

    /// Half page down (Ctrl+D)
    pub(super) fn action_half_page_down_impl(&mut self) {
        self.cancel_pending_operator();
        record_macro!(self, "<C-d>");
        self.send_keys("<C-d>");
    }

    /// Half page up (Ctrl+U)
    pub(super) fn action_half_page_up_impl(&mut self) {
        self.cancel_pending_operator();
        record_macro!(self, "<C-u>");
        self.send_keys("<C-u>");
    }

    /// Scroll viewport up by one line (Ctrl+Y)
    pub(super) fn action_scroll_viewport_up_impl(&mut self) {
        self.cancel_pending_operator();
        record_macro!(self, "<C-y>");
        self.scroll_viewport_up();
    }

    /// Scroll viewport down by one line (Ctrl+E)
    pub(super) fn action_scroll_viewport_down_impl(&mut self) {
        self.cancel_pending_operator();
        record_macro!(self, "<C-e>");
        self.scroll_viewport_down();
    }

    // =========================================================================
    // Number increment / decrement
    // =========================================================================

    /// Increment number under cursor (Ctrl+A)
    pub(super) fn action_increment_impl(&mut self) {
        record_macro!(self, "<C-a>");
        self.send_keys("<C-a>");
    }

    /// Decrement number under cursor (Ctrl+X)
    pub(super) fn action_decrement_impl(&mut self) {
        record_macro!(self, "<C-x>");
        self.send_keys("<C-x>");
    }

    // =========================================================================
    // Jump list
    // =========================================================================

    /// Jump back in jump list (Ctrl+O)
    pub(super) fn action_jump_back_impl(&mut self) {
        record_macro!(self, "<C-o>");
        self.send_keys("<C-o>");
    }

    /// Jump forward in jump list (Ctrl+I)
    pub(super) fn action_jump_forward_impl(&mut self) {
        record_macro!(self, "<C-i>");
        self.send_keys("<C-i>");
    }

    // =========================================================================
    // File info
    // =========================================================================

    /// Show file info (Ctrl+G)
    pub(super) fn action_show_file_info_impl(&mut self) {
        self.cancel_pending_operator();
        self.show_file_info();
    }

    // =========================================================================
    // Search
    // =========================================================================

    /// Open forward search mode (/)
    pub(super) fn action_open_search_forward_impl(&mut self) {
        self.open_search_mode(true);
    }

    /// Open backward search mode (?)
    pub(super) fn action_open_search_backward_impl(&mut self) {
        self.open_search_mode(false);
    }

    /// Open command line (:)
    pub(super) fn action_open_command_line_impl(&mut self) {
        self.open_command_line();
    }

    /// Search word under cursor forward (*)
    pub(super) fn action_search_word_forward_impl(&mut self) {
        self.search_word("*");
    }

    /// Search word under cursor backward (#)
    pub(super) fn action_search_word_backward_impl(&mut self) {
        self.search_word("#");
    }

    /// Search next forward (n)
    pub(super) fn action_search_next_impl(&mut self) {
        self.search_next(true);
    }

    /// Search next backward (N)
    pub(super) fn action_search_prev_impl(&mut self) {
        self.search_next(false);
    }

    // =========================================================================
    // Go to definition / file / URL
    // =========================================================================

    /// Go to definition (gd) - uses Godot LSP
    pub(super) fn action_goto_definition_impl(&mut self) {
        self.add_to_jump_list();
        self.go_to_definition_lsp();
    }

    /// Go to file under cursor (gf)
    pub(super) fn action_goto_file_impl(&mut self) {
        self.go_to_file_under_cursor();
    }

    /// Open URL under cursor (gx)
    pub(super) fn action_open_url_impl(&mut self) {
        self.open_url_under_cursor();
    }

    // =========================================================================
    // Tab navigation
    // =========================================================================

    /// Go to next tab (gt)
    pub(super) fn action_next_tab_impl(&mut self) {
        self.next_script_tab();
    }

    /// Go to previous tab (gT)
    pub(super) fn action_prev_tab_impl(&mut self) {
        self.prev_script_tab();
    }

    // =========================================================================
    // Visual mode
    // =========================================================================

    /// Toggle visual block mode (gv / Ctrl+V alternative)
    pub(super) fn action_visual_block_toggle_impl(&mut self) {
        self.visual_mode_type = '\x16'; // Ctrl+V = visual block
        let completed = self.send_keys("<C-v>");
        if completed {
            self.clear_last_key();
        }
    }

    // =========================================================================
    // g-prefix commands
    // =========================================================================

    /// Join lines without space (gJ)
    pub(super) fn action_join_no_space_impl(&mut self) {
        record_macro!(self, "gJ");
        self.send_keys("<Cmd>lua require('godot_neovim').join_no_space()<CR>");
    }

    /// Move down by display line (gj)
    pub(super) fn action_display_line_down_impl(&mut self) {
        record_macro!(self, "gj");
        self.move_display_line_down();
    }

    /// Move up by display line (gk)
    pub(super) fn action_display_line_up_impl(&mut self) {
        record_macro!(self, "gk");
        self.move_display_line_up();
    }

    /// Insert at column 0 (gI)
    pub(super) fn action_insert_at_column_zero_impl(&mut self) {
        record_macro!(self, "gI");
        self.insert_at_column_zero();
    }

    /// Insert at last insert position (gi)
    pub(super) fn action_insert_at_last_position_impl(&mut self) {
        record_macro!(self, "gi");
        self.insert_at_last_position();
    }

    /// Show character info under cursor (ga)
    pub(super) fn action_show_char_info_impl(&mut self) {
        self.show_char_info();
    }

    /// Repeat last substitution on all lines (g&)
    pub(super) fn action_repeat_substitution_impl(&mut self) {
        record_macro!(self, "g&");
        self.send_keys("g&");
    }

    /// Paste and move cursor after (gp)
    pub(super) fn action_paste_move_cursor_impl(&mut self) {
        record_macro!(self, "gp");
        self.send_keys("gp");
    }

    /// Paste before and move cursor after (gP)
    pub(super) fn action_paste_before_move_cursor_impl(&mut self) {
        record_macro!(self, "gP");
        self.send_keys("gP");
    }

    /// Move to end of previous word (ge)
    pub(super) fn action_word_end_backward_impl(&mut self) {
        record_macro!(self, "ge");
        self.move_to_word_end_backward();
        self.send_keys("ge");
    }

    /// Move to start of display line (g0)
    pub(super) fn action_display_line_start_impl(&mut self) {
        record_macro!(self, "g0");
        self.move_to_display_line_start();
    }

    /// Move to end of display line (g$)
    pub(super) fn action_display_line_end_impl(&mut self) {
        record_macro!(self, "g$");
        self.move_to_display_line_end();
    }

    /// Move to first non-blank of display line (g^)
    pub(super) fn action_display_line_first_non_blank_impl(&mut self) {
        record_macro!(self, "g^");
        self.move_to_display_line_first_non_blank();
    }

    // =========================================================================
    // Fold commands
    // =========================================================================

    /// Open fold at current line (zo)
    pub(super) fn action_fold_open_impl(&mut self) {
        record_macro!(self, "zo");
        self.unfold_current_line();
    }

    /// Close fold at current line (zc)
    pub(super) fn action_fold_close_impl(&mut self) {
        record_macro!(self, "zc");
        self.fold_current_line();
    }

    /// Toggle fold at current line (za)
    pub(super) fn action_fold_toggle_impl(&mut self) {
        record_macro!(self, "za");
        self.toggle_fold();
    }

    /// Open all folds (zR)
    pub(super) fn action_fold_open_all_impl(&mut self) {
        record_macro!(self, "zR");
        self.unfold_all();
    }

    /// Close all folds (zM)
    pub(super) fn action_fold_close_all_impl(&mut self) {
        record_macro!(self, "zM");
        self.fold_all();
    }

    // =========================================================================
    // Documentation
    // =========================================================================

    /// Open documentation for word under cursor (K)
    pub(super) fn action_open_documentation_impl(&mut self) {
        self.open_documentation();
    }

    // =========================================================================
    // Save / Close (ZZ, ZQ)
    // =========================================================================

    /// Save and close (ZZ / :wq)
    pub(super) fn action_save_and_close_impl(&mut self) {
        self.cmd_save_and_close();
    }

    /// Close without saving (ZQ / :q!)
    pub(super) fn action_close_discard_impl(&mut self) {
        self.cmd_close_discard();
    }

    // =========================================================================
    // State query methods
    // =========================================================================

    /// Get current Vim mode (n, i, v, V, R, etc.)
    pub(super) fn get_current_mode_impl(&self) -> GString {
        GString::from(&self.current_mode)
    }

    /// Get the last key pressed (for sequence detection)
    pub(super) fn get_last_key_impl(&self) -> GString {
        GString::from(&self.last_key)
    }

    /// Check if there is a pending operation (f/t/r/m/q/@/")
    pub(super) fn is_pending_operation_impl(&self) -> bool {
        self.pending_char_op.is_some()
            || self.pending_mark_op.is_some()
            || self.pending_macro_op.is_some()
            || self.selected_register == Some('\0')
    }

    /// Get the count buffer (for 3dd, 5j, etc.)
    pub(super) fn get_count_buffer_impl(&self) -> GString {
        GString::from(&self.count_buffer)
    }

    /// Check if a macro is currently being recorded
    pub(super) fn is_recording_macro_impl(&self) -> bool {
        self.recording_macro.is_some()
    }
}
