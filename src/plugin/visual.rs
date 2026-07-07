//! Visual mode selection handling

use super::GodotNeovimPlugin;

impl GodotNeovimPlugin {
    /// Update visual selection in Godot editor
    pub(super) fn update_visual_selection(&mut self) {
        // Skip if user is controlling cursor/selection (e.g., mouse drag)
        if self.user_cursor_sync {
            return;
        }

        // Skip if mouse selection is being synced (to preserve Godot's selection)
        if self.mouse_selection_syncing {
            return;
        }

        let Some(neovim) = self.get_current_neovim() else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Get visual selection from Neovim
        let Some(((start_line, start_col), (end_line, end_col))) = client.get_visual_selection()
        else {
            return;
        };

        // Release lock before updating UI
        drop(client);

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Endpoints are (anchor "v", cursor "."). Godot's select() places the
        // caret at the second pair, so pass (anchor, cursor) in that order to
        // preserve the selection direction (e.g. after `o` the caret must sit
        // at the start). The +1 inclusive adjustment (visual selections
        // include the character under the later endpoint) applies to
        // whichever endpoint is later in the buffer.
        let forward = start_line < end_line || (start_line == end_line && start_col <= end_col);

        // Convert byte columns to character columns for Godot
        // Neovim returns byte positions, Godot expects character positions
        let anchor_line_text = editor.get_line(start_line as i32).to_string();
        let cursor_line_text = editor.get_line(end_line as i32).to_string();
        let mut anchor_col = Self::byte_col_to_char_col(&anchor_line_text, start_col as i32);
        let mut cursor_col = Self::byte_col_to_char_col(&cursor_line_text, end_col as i32);
        if forward {
            cursor_col += 1;
        } else {
            anchor_col += 1;
        }

        crate::verbose_print!(
            "[godot-neovim] Visual selection: anchor=({}, {}) cursor=({}, {})",
            start_line,
            anchor_col,
            end_line,
            cursor_col
        );

        // Enable selecting and update Godot selection (caret at the cursor end)
        editor.set_selecting_enabled(true);
        editor.select(start_line as i32, anchor_col, end_line as i32, cursor_col);
    }

    /// Update visual line selection in Godot editor (V mode - selects entire lines)
    pub(super) fn update_visual_line_selection(&mut self) {
        // Skip if user is controlling cursor/selection (e.g., mouse drag)
        if self.user_cursor_sync {
            return;
        }

        // Skip if mouse selection is being synced (to preserve Godot's selection)
        if self.mouse_selection_syncing {
            return;
        }

        let Some(neovim) = self.get_current_neovim() else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Get visual selection from Neovim
        let Some(((start_line, _), (end_line, _))) = client.get_visual_selection() else {
            return;
        };

        // Release lock before updating UI
        drop(client);

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Normalize line order
        let (from_line, to_line) = if start_line <= end_line {
            (start_line, end_line)
        } else {
            (end_line, start_line)
        };

        // Select entire lines
        // Use chars().count() for character count, not byte length
        let line_text = editor.get_line(to_line as i32).to_string();
        let to_line_length = line_text.chars().count() as i64;

        crate::verbose_print!(
            "[godot-neovim] Visual line selection: lines {} to {}",
            from_line + 1,
            to_line + 1
        );

        // Enable selecting and update Godot selection (from start of first line to end of last line)
        editor.set_selecting_enabled(true);
        editor.select(from_line as i32, 0, to_line as i32, to_line_length as i32);
    }

    /// Clear visual selection in Godot editor
    pub(super) fn clear_visual_selection(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        editor.deselect();
        crate::verbose_print!("[godot-neovim] Cleared visual selection");
    }
}
