//! Macro recording and playback

use super::GodotNeovimPlugin;

impl GodotNeovimPlugin {
    /// Start recording a macro to the specified register
    pub(super) fn start_macro_recording(&mut self, register: char) {
        self.recording_macro = Some(register);
        self.macro_buffer.clear();
        self.update_recording_label(Some(register));
        crate::verbose_print!("[godot-neovim] q{}: Started recording macro", register);
    }

    /// Stop recording the current macro and save it
    pub(super) fn stop_macro_recording(&mut self) {
        if let Some(register) = self.recording_macro.take() {
            let keys = std::mem::take(&mut self.macro_buffer);
            self.update_recording_label(None);
            if !keys.is_empty() {
                self.macros.insert(register, keys.clone());
                crate::verbose_print!(
                    "[godot-neovim] q: Stopped recording macro '{}' ({} keys)",
                    register,
                    keys.len()
                );
            } else {
                crate::verbose_print!(
                    "[godot-neovim] q: Stopped recording macro '{}' (empty)",
                    register
                );
            }
        }
    }

    /// Play a macro from the specified register
    pub(super) fn play_macro(&mut self, register: char) {
        let Some(keys) = self.macros.get(&register).cloned() else {
            crate::verbose_print!("[godot-neovim] @{}: Macro not recorded", register);
            return;
        };

        if keys.is_empty() {
            crate::verbose_print!("[godot-neovim] @{}: Macro is empty", register);
            return;
        }

        self.last_macro = Some(register);
        self.playing_macro = true;

        crate::verbose_print!(
            "[godot-neovim] @{}: Playing macro ({} keys)",
            register,
            keys.len()
        );

        // Play back each key
        for key in &keys {
            self.send_keys(key);
        }

        self.playing_macro = false;
    }

    /// Replay the last played macro (@@)
    pub(super) fn replay_last_macro(&mut self) {
        if let Some(register) = self.last_macro {
            crate::verbose_print!("[godot-neovim] @@: Replaying macro '{}'", register);
            self.play_macro(register);
        } else {
            crate::verbose_print!("[godot-neovim] @@: No macro played yet");
        }
    }
}
