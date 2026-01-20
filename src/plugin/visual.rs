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

        let Some(ref neovim) = self.neovim else {
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

        // Normalize selection direction (start should be before end)
        let (from_line, from_col, to_line, to_col) =
            if start_line < end_line || (start_line == end_line && start_col <= end_col) {
                (start_line, start_col, end_line, end_col + 1) // +1 to include cursor char
            } else {
                (end_line, end_col, start_line, start_col + 1)
            };

        crate::verbose_print!(
            "[godot-neovim] Visual selection: ({}, {}) -> ({}, {})",
            from_line,
            from_col,
            to_line,
            to_col
        );

        // Update Godot selection
        editor.select(
            from_line as i32,
            from_col as i32,
            to_line as i32,
            to_col as i32,
        );
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

        let Some(ref neovim) = self.neovim else {
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
        let to_line_length = editor.get_line(to_line as i32).len() as i64;

        crate::verbose_print!(
            "[godot-neovim] Visual line selection: lines {} to {}",
            from_line + 1,
            to_line + 1
        );

        // Update Godot selection (from start of first line to end of last line)
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
