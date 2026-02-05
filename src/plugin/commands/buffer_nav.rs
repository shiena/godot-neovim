//! Buffer/tab navigation: :bn, :bp, gt, gT, :{n}

use super::super::{EditorType, GodotNeovimPlugin};
use super::simulate_ctrl_f;
use godot::classes::{CodeEdit, EditorInterface, TabBar};
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// :{number} - Jump to specific line number (Neovim Master design)
    pub(in crate::plugin) fn cmd_goto_line(&mut self, line_num: i32) {
        // Use {number}G motion instead of :{number} ex command
        // G motion properly adds to Neovim's jump list (Ctrl+O/Ctrl+I support)
        self.send_keys(&format!("{}G", line_num));

        crate::verbose_print!("[godot-neovim] :{}: Sent {}G to Neovim", line_num, line_num);
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
        // Check if we're in ShaderEditor - if so, switch shader tabs instead
        if self.current_editor_type == EditorType::Shader {
            self.switch_shader_tab(offset);
            return;
        }

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
            if let Some(script) = open_scripts.get(i as usize) {
                if script.get_path().to_string() == current_path {
                    current_idx = i;
                    break;
                }
            }
        }

        // Calculate new index with wrapping
        let new_idx = ((current_idx + offset) % count + count) % count;

        // Get the target script and switch using call_deferred
        if let Some(script) = open_scripts.get(new_idx as usize) {
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

    /// Switch shader tab by offset (1 = next, -1 = previous)
    /// Finds TabBar in ShaderEditor hierarchy and switches tabs
    fn switch_shader_tab(&mut self, offset: i32) {
        let Some(ref code_edit) = self.current_editor else {
            crate::verbose_print!("[godot-neovim] switch_shader_tab: no current editor");
            return;
        };

        // Find TabBar by traversing up from CodeEdit
        let mut current: Option<Gd<godot::classes::Node>> = code_edit.get_parent();
        let mut tab_bar: Option<Gd<TabBar>> = None;

        while let Some(mut node) = current {
            // Look for TabBar as a sibling or child in the hierarchy
            // ShaderEditor structure: ShaderEditorPlugin > HSplitContainer > TabContainer > TextShaderEditor
            // TabBar is usually a child of TabContainer
            let class_name = node.get_class().to_string();

            if class_name == "TabContainer" {
                // TabContainer has a TabBar as its first child typically
                // We can use TabContainer's methods directly
                let tab_count = node.call("get_tab_count", &[]);
                let current_tab = node.call("get_current_tab", &[]);

                if let (Ok(count), Ok(current_idx)) =
                    (tab_count.try_to::<i32>(), current_tab.try_to::<i32>())
                {
                    if count <= 1 {
                        crate::verbose_print!(
                            "[godot-neovim] switch_shader_tab: only {} tab(s) open",
                            count
                        );
                        return;
                    }

                    // Calculate new index with wrapping
                    let new_idx = ((current_idx + offset) % count + count) % count;

                    crate::verbose_print!(
                        "[godot-neovim] Shader tab switch: {} -> {} (of {})",
                        current_idx,
                        new_idx,
                        count
                    );

                    // Switch tabs
                    node.call("set_current_tab", &[new_idx.to_variant()]);

                    // Also update the ItemList selection (shader_list)
                    // ItemList is a sibling of TabContainer in HSplitContainer
                    if let Some(parent) = node.get_parent() {
                        if parent.get_class().to_string() == "HSplitContainer" {
                            let child_count = parent.get_child_count();
                            for i in 0..child_count {
                                if let Some(mut child) = parent.get_child(i) {
                                    if child.get_class().to_string() == "ItemList" {
                                        // Update ItemList selection to match the new tab
                                        child.call("select", &[new_idx.to_variant()]);
                                        crate::verbose_print!(
                                            "[godot-neovim] Updated ItemList selection to {}",
                                            new_idx
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // Get the new tab's content and find CodeEdit to grab focus
                    let tab_control = node.call("get_tab_control", &[new_idx.to_variant()]);
                    if let Ok(control) = tab_control.try_to::<Gd<godot::classes::Control>>() {
                        // Find CodeEdit in the new tab and grab focus
                        if let Some(code_edit) =
                            self.find_code_edit_in_control(control.clone().upcast())
                        {
                            crate::verbose_print!(
                                "[godot-neovim] Grabbing focus on new shader tab's CodeEdit"
                            );
                            let mut ce = code_edit;
                            ce.call_deferred("grab_focus", &[]);
                        }
                    }
                    return;
                }
            }

            // Also check for TabBar directly
            if let Ok(tb) = node.clone().try_cast::<TabBar>() {
                tab_bar = Some(tb);
                break;
            }

            current = node.get_parent();
        }

        // If we found a TabBar directly, use it
        if let Some(mut tb) = tab_bar {
            let count = tb.get_tab_count();
            if count <= 1 {
                crate::verbose_print!(
                    "[godot-neovim] switch_shader_tab: only {} tab(s) open",
                    count
                );
                return;
            }

            let current_idx = tb.get_current_tab();
            let new_idx = ((current_idx + offset) % count + count) % count;

            crate::verbose_print!(
                "[godot-neovim] Shader tab switch (TabBar): {} -> {} (of {})",
                current_idx,
                new_idx,
                count
            );

            tb.call_deferred("set_current_tab", &[new_idx.to_variant()]);
            return;
        }

        crate::verbose_print!("[godot-neovim] switch_shader_tab: TabBar/TabContainer not found");
    }

    /// Start backward search (? command) - opens Godot's search dialog
    pub(in crate::plugin) fn start_search_backward(&self) {
        simulate_ctrl_f();
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

    /// Sync Godot script tab to match Neovim's current buffer
    /// Called when Neovim switches buffer (e.g., via Ctrl+O/Ctrl+I jump)
    pub(crate) fn sync_godot_script_tab(&mut self, neovim_path: &str) {
        let mut editor = EditorInterface::singleton();
        let Some(mut script_editor) = editor.get_script_editor() else {
            return;
        };

        // Check if current Godot script already matches
        if let Some(current_script) = script_editor.get_current_script() {
            let current_res_path = current_script.get_path().to_string();
            // Convert res:// path to absolute path for comparison
            let current_abs_path =
                godot::classes::ProjectSettings::singleton().globalize_path(&current_res_path);
            let current_abs_str = current_abs_path.to_string().replace('\\', "/");

            // Normalize neovim path (ensure forward slashes)
            let neovim_normalized = neovim_path.replace('\\', "/");

            if current_abs_str == neovim_normalized {
                // Already on the correct script
                return;
            }
        }

        // Find and switch to the script matching neovim_path
        let open_scripts = script_editor.get_open_scripts();
        let neovim_normalized = neovim_path.replace('\\', "/");

        for i in 0..open_scripts.len() {
            if let Some(script) = open_scripts.get(i) {
                let res_path = script.get_path().to_string();
                let abs_path =
                    godot::classes::ProjectSettings::singleton().globalize_path(&res_path);
                let abs_str = abs_path.to_string().replace('\\', "/");

                if abs_str == neovim_normalized {
                    crate::verbose_print!(
                        "[godot-neovim] BufEnter: Switching Godot tab to {}",
                        res_path
                    );
                    // Use call_deferred to avoid issues during event processing
                    editor.call_deferred("edit_script", &[script.to_variant()]);
                    return;
                }
            }
        }

        crate::verbose_print!(
            "[godot-neovim] BufEnter: No matching Godot script for {}",
            neovim_path
        );
    }

    /// Find CodeEdit recursively within a control hierarchy
    fn find_code_edit_in_control(&self, control: Gd<godot::classes::Node>) -> Option<Gd<CodeEdit>> {
        // Check if this node is a CodeEdit
        if let Ok(code_edit) = control.clone().try_cast::<CodeEdit>() {
            return Some(code_edit);
        }

        // Recursively search children
        for i in 0..control.get_child_count() {
            if let Some(child) = control.get_child(i) {
                if let Some(code_edit) = self.find_code_edit_in_control(child) {
                    return Some(code_edit);
                }
            }
        }

        None
    }
}
