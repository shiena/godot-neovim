use crate::neovim::NeovimClient;
use crate::settings;
use godot::classes::{CodeEdit, Control, EditorInterface, EditorPlugin, IEditorPlugin, Label};
use godot::global::Key;
use godot::prelude::*;
use std::sync::Mutex;

/// Main editor plugin for godot-neovim
#[derive(GodotClass)]
#[class(tool, init, base=EditorPlugin)]
pub struct GodotNeovimPlugin {
    base: Base<EditorPlugin>,
    #[init(val = None)]
    neovim: Option<Mutex<NeovimClient>>,
    #[init(val = None)]
    mode_label: Option<Gd<Label>>,
    #[init(val = None)]
    current_editor: Option<Gd<CodeEdit>>,
}

#[godot_api]
impl IEditorPlugin for GodotNeovimPlugin {
    fn enter_tree(&mut self) {
        godot_print!("[godot-neovim] Plugin entering tree");

        // Initialize settings first
        settings::initialize_settings();

        // Validate Neovim path
        let validation = settings::validate_current_path();
        if !validation.is_valid() {
            godot_warn!("[godot-neovim] Neovim validation failed, plugin may not work correctly");
        }

        // Initialize Neovim client
        match NeovimClient::new() {
            Ok(mut client) => {
                if let Err(e) = client.start() {
                    godot_error!("[godot-neovim] Failed to start Neovim: {}", e);
                    return;
                }
                self.neovim = Some(Mutex::new(client));
            }
            Err(e) => {
                godot_error!("[godot-neovim] Failed to create Neovim client: {}", e);
                return;
            }
        }

        // Create mode indicator label
        self.create_mode_label();

        // Connect to script editor signals
        self.connect_script_editor_signals();

        // Connect to settings changed signal
        self.connect_settings_signals();

        godot_print!("[godot-neovim] Plugin initialized successfully");
    }

    fn exit_tree(&mut self) {
        godot_print!("[godot-neovim] Plugin exiting tree");

        // Cleanup mode label
        if let Some(mut label) = self.mode_label.take() {
            label.queue_free();
        }

        // Neovim client will be stopped when dropped
        self.neovim = None;
    }

    fn input(&mut self, event: Gd<godot::classes::InputEvent>) {
        // Only handle key events
        let Ok(key_event) = event.try_cast::<godot::classes::InputEventKey>() else {
            return;
        };

        // Only handle key press (not release)
        if !key_event.is_pressed() {
            return;
        }

        // Check if the current editor has focus
        if !self.editor_has_focus() {
            return;
        }

        // Forward key to Neovim
        if let Some(keys) = self.key_event_to_nvim_string(&key_event) {
            self.send_keys(&keys);
        }
    }
}

#[godot_api]
impl GodotNeovimPlugin {
    fn create_mode_label(&mut self) {
        let mut label = Label::new_alloc();
        label.set_text("NORMAL");
        label.set_name("NeovimModeLabel");

        // Style the label
        label.add_theme_color_override("font_color", Color::from_rgb(0.0, 1.0, 0.5));

        // Get editor interface and add label to script editor
        let editor = EditorInterface::singleton();
        if let Some(mut script_editor) = editor.get_script_editor() {
            script_editor.add_child(&label);
        }

        self.mode_label = Some(label);
    }

    fn connect_script_editor_signals(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(mut script_editor) = editor.get_script_editor() {
            // Connect to editor script changed signal
            let callable = self.base().callable("on_script_changed");
            script_editor.connect("editor_script_changed", &callable);
        }
    }

    fn connect_settings_signals(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(mut editor_settings) = editor.get_editor_settings() {
            // Connect to settings changed signal
            let callable = self.base().callable("on_settings_changed");
            editor_settings.connect("settings_changed", &callable);
        }
    }

    #[func]
    fn on_settings_changed(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(editor_settings) = editor.get_editor_settings() {
            settings::on_settings_changed(&editor_settings);
        }
    }

    #[func]
    fn on_script_changed(&mut self, _script: Gd<godot::classes::Script>) {
        godot_print!("[godot-neovim] Script changed");
        self.find_current_code_edit();
        self.sync_buffer_to_neovim();
    }

    fn find_current_code_edit(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(script_editor) = editor.get_script_editor() {
            // Find CodeEdit in the script editor
            if let Some(code_edit) = self.find_code_edit_recursive(script_editor.upcast::<Control>()) {
                self.current_editor = Some(code_edit);
            }
        }
    }

    fn find_code_edit_recursive(&self, node: Gd<Control>) -> Option<Gd<CodeEdit>> {
        // Check if this node is a CodeEdit
        if let Ok(code_edit) = node.clone().try_cast::<CodeEdit>() {
            return Some(code_edit);
        }

        // Search children
        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Ok(control) = child.try_cast::<Control>() {
                    if let Some(code_edit) = self.find_code_edit_recursive(control) {
                        return Some(code_edit);
                    }
                }
            }
        }

        None
    }

    fn sync_buffer_to_neovim(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.lock() else {
            return;
        };

        // Get text from Godot editor
        let text = editor.get_text().to_string();
        let lines: Vec<String> = text.lines().map(String::from).collect();

        // Set buffer content in Neovim
        if let Err(e) = client.set_buffer_lines(0, -1, lines) {
            godot_error!("[godot-neovim] Failed to sync buffer: {}", e);
        }
    }

    fn editor_has_focus(&self) -> bool {
        if let Some(ref editor) = self.current_editor {
            return editor.has_focus();
        }
        false
    }

    fn key_event_to_nvim_string(
        &self,
        event: &Gd<godot::classes::InputEventKey>,
    ) -> Option<String> {
        let keycode = event.get_keycode();
        let ctrl = event.is_ctrl_pressed();
        let alt = event.is_alt_pressed();
        let shift = event.is_shift_pressed();

        // Handle special keys
        let key_str = match keycode {
            Key::ESCAPE => "<Esc>".to_string(),
            Key::ENTER => "<CR>".to_string(),
            Key::TAB => "<Tab>".to_string(),
            Key::BACKSPACE => "<BS>".to_string(),
            Key::DELETE => "<Del>".to_string(),
            Key::UP => "<Up>".to_string(),
            Key::DOWN => "<Down>".to_string(),
            Key::LEFT => "<Left>".to_string(),
            Key::RIGHT => "<Right>".to_string(),
            Key::HOME => "<Home>".to_string(),
            Key::END => "<End>".to_string(),
            Key::PAGEUP => "<PageUp>".to_string(),
            Key::PAGEDOWN => "<PageDown>".to_string(),
            Key::F1 => "<F1>".to_string(),
            Key::F2 => "<F2>".to_string(),
            Key::F3 => "<F3>".to_string(),
            Key::F4 => "<F4>".to_string(),
            Key::F5 => "<F5>".to_string(),
            Key::F6 => "<F6>".to_string(),
            Key::F7 => "<F7>".to_string(),
            Key::F8 => "<F8>".to_string(),
            Key::F9 => "<F9>".to_string(),
            Key::F10 => "<F10>".to_string(),
            Key::F11 => "<F11>".to_string(),
            Key::F12 => "<F12>".to_string(),
            Key::SPACE => " ".to_string(),
            _ => {
                // Get unicode character
                let unicode = event.get_unicode();
                if unicode > 0 {
                    char::from_u32(unicode as u32)?.to_string()
                } else {
                    return None;
                }
            }
        };

        // Apply modifiers
        let result = if ctrl || alt {
            let mut mods = String::new();
            if ctrl {
                mods.push('C');
            }
            if alt {
                mods.push('A');
            }
            if shift && key_str.len() == 1 {
                mods.push('S');
            }

            if key_str.starts_with('<') {
                // Already a special key
                format!("<{}-{}>", mods, &key_str[1..key_str.len() - 1])
            } else {
                format!("<{}-{}>", mods, key_str)
            }
        } else {
            key_str
        };

        Some(result)
    }

    fn send_keys(&mut self, keys: &str) {
        // Get mode and buffer data first
        let (mode, lines, cursor) = {
            let Some(ref neovim) = self.neovim else {
                return;
            };

            let Ok(client) = neovim.lock() else {
                return;
            };

            if let Err(e) = client.input(keys) {
                godot_error!("[godot-neovim] Failed to send keys: {}", e);
                return;
            }

            let mode = client.get_mode();
            let lines = client.get_buffer_lines(0, -1).ok();
            let cursor = client.get_cursor().ok();

            (mode, lines, cursor)
        };

        // Update mode display
        self.update_mode_display(&mode);

        // Sync buffer back to Godot
        if let Some(lines) = lines {
            self.sync_buffer_from_neovim(lines, cursor);
        }
    }

    fn update_mode_display(&mut self, mode: &str) {
        let mode_text = match mode {
            "n" | "normal" => "NORMAL",
            "i" | "insert" => "INSERT",
            "v" | "visual" => "VISUAL",
            "V" | "visual_line" => "V-LINE",
            "\x16" | "visual_block" => "V-BLOCK",
            "c" | "command" => "COMMAND",
            "R" | "replace" => "REPLACE",
            "t" | "terminal" => "TERMINAL",
            _ => mode,
        };

        if let Some(ref mut label) = self.mode_label {
            label.set_text(mode_text);

            // Change color based on mode
            let color = match mode {
                "n" | "normal" => Color::from_rgb(0.0, 1.0, 0.5),   // Green
                "i" | "insert" => Color::from_rgb(0.3, 0.6, 1.0),  // Blue
                "v" | "visual" | "V" | "\x16" => Color::from_rgb(1.0, 0.5, 0.0), // Orange
                "c" | "command" => Color::from_rgb(1.0, 1.0, 0.0), // Yellow
                "R" | "replace" => Color::from_rgb(1.0, 0.3, 0.3), // Red
                _ => Color::from_rgb(0.8, 0.8, 0.8),               // Gray
            };
            label.add_theme_color_override("font_color", color);
        }
    }

    fn sync_buffer_from_neovim(&mut self, lines: Vec<String>, cursor: Option<(i64, i64)>) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Update Godot editor
        let text = lines.join("\n");
        editor.set_text(&text);

        // Update cursor position
        if let Some((line, col)) = cursor {
            editor.set_caret_line((line - 1) as i32); // Neovim is 1-indexed
            editor.set_caret_column(col as i32);
        }
    }
}
