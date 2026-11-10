use super::GodotNeovimPlugin;
use crate::lsp::LspStartup;
use godot::classes::{EditorInterface, ProjectSettings};
use godot::prelude::*;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(super) enum LspAction {
    Definition,
    Documentation,
}

pub(super) struct PendingLsp {
    startup: LspStartup,
    action: LspAction,
    path: String,
    switch_id: u64,
    caret: (i32, i32),
    text: GString,
}

impl PendingLsp {
    // Startup may finish after a tab switch or edit. Replay only at the original location.
    fn matches_current_editor(&self, plugin: &GodotNeovimPlugin) -> bool {
        self.path == plugin.current_script_path
            && self.switch_id == plugin.script_switch_id
            && plugin.current_editor.as_ref().is_some_and(|editor| {
                editor.is_instance_valid()
                    && editor.has_focus()
                    && self.caret == (editor.get_caret_line(), editor.get_caret_column())
                    && self.text == editor.get_text()
            })
    }
}

impl GodotNeovimPlugin {
    pub(super) fn ensure_lsp_ready(&mut self, action: LspAction) -> bool {
        let Some(lsp) = &self.godot_lsp else {
            self.show_status_message("LSP: Enable 'Use Thread' in Editor Settings");
            return false;
        };
        if lsp.is_connected() && lsp.is_initialized() {
            return true;
        }
        let Some(editor) = &self.current_editor else {
            return false;
        };
        if let Some(pending) = &mut self.pending_lsp {
            pending.action = action;
            pending.path = self.current_script_path.clone();
            pending.switch_id = self.script_switch_id;
            pending.caret = (editor.get_caret_line(), editor.get_caret_column());
            pending.text = editor.get_text();
            return false;
        }
        let settings = EditorInterface::singleton().get_editor_settings();
        let host = settings
            .as_ref()
            .and_then(|s| {
                s.get_setting("network/language_server/remote_host")
                    .try_to::<GString>()
                    .ok()
            })
            .map(|s| s.to_string())
            .unwrap_or_else(|| "127.0.0.1".into());
        let port = settings
            .as_ref()
            .and_then(|s| {
                s.get_setting("network/language_server/remote_port")
                    .try_to::<i64>()
                    .ok()
            })
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(6005);
        let root = ProjectSettings::singleton()
            .globalize_path("res://")
            .to_string()
            .replace('\\', "/");
        let root = if root.starts_with('/') {
            format!("file://{root}")
        } else {
            format!("file:///{root}")
        };
        match LspStartup::start(host, port, root) {
            Ok(startup) => {
                self.pending_lsp = Some(PendingLsp {
                    startup,
                    action,
                    path: self.current_script_path.clone(),
                    switch_id: self.script_switch_id,
                    caret: (editor.get_caret_line(), editor.get_caret_column()),
                    text: editor.get_text(),
                });
                self.show_status_message("Waiting for Godot language server...");
            }
            Err(error) => self.show_status_message(&error),
        }
        false
    }

    pub(super) fn process_lsp_startup(&mut self) {
        let Some(pending) = &self.pending_lsp else {
            return;
        };
        if !pending.matches_current_editor(self) {
            self.pending_lsp = None;
            return;
        }
        let Some(result) = pending.startup.poll() else {
            return;
        };
        let action = pending.action;
        self.pending_lsp = None;
        match result {
            Ok(client) => {
                self.godot_lsp = Some(Arc::new(client));
                match action {
                    LspAction::Definition => self.go_to_definition_lsp(),
                    LspAction::Documentation => self.open_documentation(),
                }
            }
            Err(error) => self.show_status_message(&error),
        }
    }
}
