//! Neovim timeout recovery: automatic recovery when Neovim becomes unresponsive

use super::GodotNeovimPlugin;
use crate::neovim::{TIMEOUT_RECOVERY_THRESHOLD, TIMEOUT_RECOVERY_WINDOW_SECS};
use crate::neovim::NeovimClient;
use godot::classes::{ConfirmationDialog, EditorInterface, ProjectSettings, ResourceSaver};
use godot::prelude::*;
use std::sync::Mutex;
use std::time::{Duration, Instant};

impl GodotNeovimPlugin {
    /// Record a timeout error and check if recovery should be triggered.
    /// Returns true if the recovery dialog should be shown.
    pub(super) fn record_timeout_error(&mut self) -> bool {
        if self.recovery_dialog_open {
            return false;
        }

        let now = Instant::now();
        let window = Duration::from_secs(TIMEOUT_RECOVERY_WINDOW_SECS);

        self.timeout_timestamps.push(now);
        self.timeout_timestamps
            .retain(|t| now.duration_since(*t) < window);

        if self.timeout_timestamps.len() >= TIMEOUT_RECOVERY_THRESHOLD as usize {
            self.timeout_timestamps.clear();
            true
        } else {
            false
        }
    }

    /// Reset the timeout counter (e.g., after successful recovery)
    pub(super) fn reset_timeout_counter(&mut self) {
        self.timeout_timestamps.clear();
    }

    /// Show the recovery dialog when Neovim becomes unresponsive
    pub(super) fn show_recovery_dialog(&mut self) {
        self.recovery_dialog_open = true;

        let mut dialog = ConfirmationDialog::new_alloc();
        dialog.set_title("Neovim Recovery");
        dialog.set_text("Neovim is not responding.\n\nSave all files and restart?");
        dialog.set_ok_button_text("Save & Restart");
        dialog.set_cancel_button_text("Cancel");

        // Add custom button for restart without saving
        dialog.add_button_ex("Restart without Saving")
            .right(false)
            .action("restart_no_save")
            .done();

        // Connect signals
        let callable_confirmed = self.base().callable("on_recovery_save_restart");
        let callable_canceled = self.base().callable("on_recovery_cancel");
        let callable_custom = self.base().callable("on_recovery_custom_action");

        dialog.connect("confirmed", &callable_confirmed);
        dialog.connect("canceled", &callable_canceled);
        dialog.connect("custom_action", &callable_custom);

        // Add to editor and show
        if let Some(base_control) = EditorInterface::singleton().get_base_control() {
            let mut base_control = base_control;
            base_control.add_child(&dialog);
            dialog.popup_centered();
        }

        self.recovery_dialog = Some(dialog);
    }

    /// Save all open scripts via Godot's ResourceSaver
    pub(super) fn save_all_open_scripts(&mut self) {
        let editor = EditorInterface::singleton();
        let Some(mut script_editor) = editor.get_script_editor() else {
            crate::verbose_print!("[godot-neovim] Recovery: No script editor found");
            return;
        };

        let open_scripts = script_editor.get_open_scripts();
        let current_script = script_editor.get_current_script();

        for i in 0..open_scripts.len() {
            let Some(script) = open_scripts.get(i) else {
                continue;
            };
            let Some(mut script) = script.try_cast::<godot::classes::Script>().ok() else {
                continue;
            };

            let path = script.get_path();
            if path.is_empty() {
                continue;
            }

            // If this is the current script and we have a valid editor,
            // sync the CodeEdit content to the Script first
            let is_current = current_script
                .as_ref()
                .map(|cs| cs.get_path() == path)
                .unwrap_or(false);

            if is_current {
                if let Some(ref editor) = self.current_editor {
                    if editor.is_instance_valid() {
                        let text = editor.get_text();
                        script.set_source_code(&text);
                        crate::verbose_print!(
                            "[godot-neovim] Recovery: Synced CodeEdit to Script: {}",
                            path
                        );
                    }
                }
            }

            // Save using ResourceSaver
            let result = ResourceSaver::singleton()
                .save_ex(&script)
                .path(&path)
                .done();

            if result == godot::global::Error::OK {
                crate::verbose_print!("[godot-neovim] Recovery: Saved {}", path);
            } else {
                godot_warn!("[godot-neovim] Recovery: Failed to save {}", path);
            }
        }
    }

    /// Restart the Neovim client
    pub(super) fn restart_neovim(&mut self) {
        crate::verbose_print!("[godot-neovim] Recovery: Restarting Neovim...");

        // Stop existing Neovim client
        if let Some(ref neovim) = self.neovim {
            if let Ok(mut client) = neovim.lock() {
                client.stop();
            }
        }
        self.neovim = None;

        // Reset sync state
        self.sync_manager.reset();
        self.reset_timeout_counter();

        // Get addons path for Lua plugin
        let addons_path = ProjectSettings::singleton()
            .globalize_path("res://addons/godot-neovim")
            .to_string();

        // Create new Neovim client
        match NeovimClient::new() {
            Ok(mut client) => {
                if let Err(e) = client.start(Some(&addons_path)) {
                    godot_error!("[godot-neovim] Recovery: Failed to start Neovim: {}", e);
                    return;
                }

                self.neovim = Some(Mutex::new(client));
                crate::verbose_print!("[godot-neovim] Recovery: Neovim restarted successfully");

                // Reinitialize current buffer
                self.script_changed_pending.set(true);
            }
            Err(e) => {
                godot_error!("[godot-neovim] Recovery: Failed to create Neovim client: {}", e);
            }
        }
    }

    /// Clean up the recovery dialog
    pub(super) fn cleanup_recovery_dialog(&mut self) {
        if let Some(mut dialog) = self.recovery_dialog.take() {
            if dialog.is_instance_valid() {
                dialog.queue_free();
            }
        }
        self.recovery_dialog_open = false;
    }
}
