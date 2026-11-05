use super::GodotNeovimPlugin;
use godot::prelude::*;

pub(super) enum PendingInput {
    Keys(String),
    Event(Gd<godot::classes::InputEvent>),
}

impl GodotNeovimPlugin {
    pub(super) fn current_commands_pending(&self) -> bool {
        self.get_current_neovim()
            .and_then(|client| client.try_lock().ok())
            .is_some_and(|client| client.commands_pending())
    }

    pub(super) fn clear_pending_neovim_switch(&mut self) {
        self.pending_neovim_switch = false;
        self.pending_cursor_sync = false;
        self.pending_switch_path.clear();
        self.pending_switch_input.clear();
    }

    pub(super) fn process_command_results(&mut self) {
        for client in [&self.script_neovim, &self.shader_neovim]
            .into_iter()
            .flatten()
        {
            let Ok(client) = client.try_lock() else {
                continue;
            };
            for completed in client.take_command_results() {
                match completed.result {
                    Ok(Some(value)) => {
                        godot_print!("[godot-neovim] {}: {}", completed.command, value)
                    }
                    Ok(None) => {}
                    Err(error) => godot_warn!("[godot-neovim] {}: {}", completed.command, error),
                }
            }
        }
        if self.pending_cursor_sync
            && !self.pending_neovim_switch
            && !self.current_commands_pending()
        {
            self.sync_cursor_to_neovim();
        }
    }

    pub(super) fn defer_input_during_switch(
        &mut self,
        event: &Gd<godot::classes::InputEvent>,
    ) -> bool {
        if !self.pending_neovim_switch
            || self.get_focused_code_edit_direct().is_none()
            || !event.is_class("InputEventKey")
        {
            return false;
        }
        self.pending_switch_input
            .push(PendingInput::Event(event.clone()));
        if let Some(mut viewport) = self
            .get_focused_code_edit_direct()
            .and_then(|editor| editor.get_viewport())
        {
            viewport.set_input_as_handled();
        }
        true
    }

    pub(super) fn retry_pending_neovim_switch(&mut self) {
        if !self.pending_neovim_switch {
            return;
        }
        let ready = self
            .get_current_neovim()
            .and_then(|client| client.try_lock().ok())
            .is_some_and(|client| !client.commands_pending());
        if !ready {
            return;
        }
        // Drain the previous buffer before replacing its editor and sync state.
        self.process_neovim_updates();
        self.pending_neovim_switch = false;
        self.script_changed_pending.set(false);
        self.handle_script_changed_deferred();
        if self.pending_neovim_switch {
            return;
        }
        if !self.pending_switch_path.is_empty()
            && self.pending_switch_path != self.current_script_path
        {
            self.clear_pending_neovim_switch();
            return;
        }
        for input in std::mem::take(&mut self.pending_switch_input) {
            match input {
                PendingInput::Keys(keys) => {
                    self.base_mut()
                        .call_deferred("send_deferred_keys", &[keys.to_variant()]);
                }
                PendingInput::Event(event) => {
                    if let Some(mut viewport) = self
                        .current_editor
                        .as_ref()
                        .and_then(|editor| editor.get_viewport())
                    {
                        viewport.call_deferred("push_input", &[event.to_variant()]);
                    }
                }
            }
        }
    }
}
