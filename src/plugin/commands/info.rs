//! Information display: :marks, :registers, :jumps, :changes, :ls

use super::super::GodotNeovimPlugin;
use godot::classes::EditorInterface;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// :marks - Show all marks
    pub(in crate::plugin) fn cmd_show_marks(&self) {
        if self.marks.is_empty() {
            godot_print!("[godot-neovim] :marks - No marks set");
            return;
        }

        godot_print!("[godot-neovim] :marks");
        godot_print!("mark  line  col");

        // Sort marks by character
        let mut marks: Vec<_> = self.marks.iter().collect();
        marks.sort_by_key(|(k, _)| *k);

        for (mark, (line, col)) in marks {
            godot_print!(" {}    {:>4}  {:>3}", mark, line + 1, col);
        }
    }

    /// :registers or :reg - Show all registers
    pub(in crate::plugin) fn cmd_show_registers(&self) {
        if self.registers.is_empty() {
            godot_print!("[godot-neovim] :registers - No registers set");
            return;
        }

        godot_print!("[godot-neovim] :registers");

        // Sort registers by character
        let mut regs: Vec<_> = self.registers.iter().collect();
        regs.sort_by_key(|(k, _)| *k);

        for (reg, content) in regs {
            // Truncate long content and show preview
            let preview = if content.len() > 50 {
                format!("{}...", &content[..47])
            } else {
                content.replace('\n', "^J")
            };
            godot_print!("\"{}   {}", reg, preview);
        }
    }

    /// :jumps - Show the jump list
    pub(in crate::plugin) fn cmd_show_jumps(&self) {
        godot_print!("[godot-neovim] :jumps");
        godot_print!(" jump line  col");

        if self.jump_list.is_empty() {
            godot_print!("   (empty)");
            return;
        }

        for (i, (line, col)) in self.jump_list.iter().enumerate() {
            let marker = if i == self.jump_list_pos { ">" } else { " " };
            godot_print!("{}{:>4}  {:>4}  {:>3}", marker, i + 1, line + 1, col);
        }

        if self.jump_list_pos >= self.jump_list.len() {
            godot_print!(">          (current)");
        }
    }

    /// :changes - Show the change list (simplified - we don't track changes)
    pub(in crate::plugin) fn cmd_show_changes(&self) {
        godot_print!("[godot-neovim] :changes");
        godot_print!("   (change list not tracked)");
        godot_print!("   Use undo/redo (u/Ctrl+R) for changes");
    }

    /// :ls / :buffers - List open buffers
    pub(in crate::plugin) fn cmd_list_buffers(&self) {
        let editor = EditorInterface::singleton();
        if let Some(script_editor) = editor.get_script_editor() {
            let open_scripts = script_editor.get_open_scripts();

            godot_print!("[godot-neovim] :ls - Open buffers:");
            for i in 0..open_scripts.len() {
                if let Some(script_var) = open_scripts.get(i) {
                    if let Ok(script) = script_var.try_cast::<godot::classes::Script>() {
                        let path = script.get_path().to_string();
                        let name = path.split('/').next_back().unwrap_or(&path);
                        godot_print!("  {}: {}", i + 1, name);
                    }
                }
            }
        }
    }
}
