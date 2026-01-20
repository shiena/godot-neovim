//! Buffer/tab navigation: :bn, :bp, gt, gT, :{n}

use super::super::GodotNeovimPlugin;
use godot::classes::{EditorInterface, Input, InputEventKey};
use godot::global::Key;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// :{number} - Jump to specific line number (Neovim Master design)
    pub(in crate::plugin) fn cmd_goto_line(&mut self, line_num: i32) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        // Send command to Neovim - it will process and send win_viewport event
        // This ensures cursor sync is consistent with Neovim's state
        self.send_keys(&format!(":{}<CR>", line_num));

        crate::verbose_print!("[godot-neovim] :{}: Sent to Neovim", line_num);
    }

    /// gt - Go to next script tab
    pub(in crate::plugin) fn next_script_tab(&mut self) {
        self.switch_script_tab(1);
    }

    /// gT - Go to previous script tab
    pub(in crate::plugin) fn prev_script_tab(&mut self) {
        self.switch_script_tab(-1);
    }

    /// Switch script tab by offset (1 = next, -1 = previous)
    fn switch_script_tab(&mut self, offset: i32) {
        let mut editor = EditorInterface::singleton();
        let Some(mut script_editor) = editor.get_script_editor() else {
            return;
        };

        let open_scripts = script_editor.get_open_scripts();
        let count = open_scripts.len() as i32;
        if count <= 1 {
            crate::verbose_print!(
                "[godot-neovim] switch_script_tab: only {} script(s) open",
                count
            );
            return;
        }

        // Find current script index
        let current_script = script_editor.get_current_script();
        let current_path = current_script
            .as_ref()
            .map(|s| s.get_path().to_string())
            .unwrap_or_default();

        let mut current_idx: i32 = 0;
        for i in 0..count {
            if let Some(script_var) = open_scripts.get(i as usize) {
                if let Ok(script) = script_var.try_cast::<godot::classes::Script>() {
                    if script.get_path().to_string() == current_path {
                        current_idx = i;
                        break;
                    }
                }
            }
        }

        // Calculate new index with wrapping
        let new_idx = ((current_idx + offset) % count + count) % count;

        // Get the target script and switch using call_deferred
        if let Some(script_var) = open_scripts.get(new_idx as usize) {
            if let Ok(script) = script_var.try_cast::<godot::classes::Script>() {
                let new_path = script.get_path().to_string();
                crate::verbose_print!(
                    "[godot-neovim] Tab switch: {} -> {} ({})",
                    current_idx,
                    new_idx,
                    new_path
                );
                // Use call_deferred to avoid blocking during input handling
                editor.call_deferred("edit_script", &[script.to_variant()]);
            }
        }
    }

    /// Start backward search (? command) - opens Godot's search dialog
    pub(in crate::plugin) fn start_search_backward(&self) {
        // Simulate Ctrl+F to open the search dialog
        let mut key_event = InputEventKey::new_gd();
        key_event.set_keycode(Key::F);
        key_event.set_ctrl_pressed(true);
        key_event.set_pressed(true);
        Input::singleton().parse_input_event(&key_event);

        crate::verbose_print!(
            "[godot-neovim] ?: Opening search dialog (use Find Previous for backward search)"
        );
    }

    /// :bn / :bnext - Go to next buffer (script tab)
    pub(in crate::plugin) fn cmd_buffer_next(&mut self) {
        self.next_script_tab();
    }

    /// :bp / :bprev - Go to previous buffer (script tab)
    pub(in crate::plugin) fn cmd_buffer_prev(&mut self) {
        self.prev_script_tab();
    }
}
