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
    /// Current mode cached from last update
    #[init(val = String::from("n"))]
    current_mode: String,
    /// Current cursor position (line, col) - 0-indexed from grid
    #[init(val = (0, 0))]
    current_cursor: (i64, i64),
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

        // Try to find and sync current editor (in case a script is already open)
        self.find_current_code_edit();
        if self.current_editor.is_some() {
            godot_print!("[godot-neovim] Found existing CodeEdit, syncing initial buffer");
            self.reposition_mode_label();
            self.sync_buffer_to_neovim();
        }

        // Enable process() to be called every frame for checking redraw events
        self.base_mut().set_process(true);

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

    fn process(&mut self, _delta: f64) {
        // Check for pending updates from Neovim redraw events
        self.process_neovim_updates();
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

        // Check if Neovim is connected
        if self.neovim.is_none() {
            godot_print!("[godot-neovim] input: No neovim");
            return;
        }

        godot_print!("[godot-neovim] input: mode={}, key={:?}", self.current_mode, key_event.get_keycode());

        // In insert mode, let Godot handle most keys natively
        if self.is_insert_mode() {
            // Intercept Escape or Ctrl+[ to exit insert mode
            let is_escape = key_event.get_keycode() == Key::ESCAPE;
            let is_ctrl_bracket = key_event.is_ctrl_pressed() && key_event.get_keycode() == Key::BRACKETLEFT;

            if is_escape || is_ctrl_bracket {
                self.send_escape();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
            }
            // Let Godot handle other keys in insert mode
            return;
        }

        // Forward key to Neovim (normal/visual/etc modes)
        if let Some(keys) = self.key_event_to_nvim_string(&key_event) {
            self.send_keys(&keys);

            // Consume the event to prevent Godot's default handling
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
        }
    }
}

#[godot_api]
impl GodotNeovimPlugin {
    fn create_mode_label(&mut self) {
        // Only create label if we have a current editor with a status bar
        let Some(code_edit) = &self.current_editor else {
            return;
        };

        let Some(mut status_bar) = self.find_status_bar(code_edit.clone().upcast()) else {
            return;
        };

        let mut label = Label::new_alloc();
        label.set_text(" NORMAL ");
        label.set_name("NeovimModeLabel");

        // Style the label
        label.add_theme_color_override("font_color", Color::from_rgb(0.0, 1.0, 0.5));

        // Add to status bar
        status_bar.add_child(&label);
        status_bar.move_child(&label, 0);
        self.mode_label = Some(label);
    }

    fn find_status_bar(&self, node: Gd<Control>) -> Option<Gd<Control>> {
        // The status bar is an HBoxContainer inside CodeTextEditor (sibling of CodeEdit)
        // CodeTextEditor > CodeEdit (node)
        //                > HBoxContainer (status bar with line/column info)

        // Get parent (should be CodeTextEditor)
        let Some(parent) = node.get_parent() else {
            return None;
        };

        let parent_class = parent.get_class().to_string();
        godot_print!("[godot-neovim] CodeEdit parent: {} ({})", parent.get_name(), parent_class);

        // Search siblings for HBoxContainer (status bar)
        let child_count = parent.get_child_count();
        for i in 0..child_count {
            if let Some(child) = parent.get_child(i) {
                let class_name = child.get_class().to_string();
                if class_name == "HBoxContainer" {
                    if let Ok(control) = child.try_cast::<Control>() {
                        godot_print!("[godot-neovim] Found HBoxContainer status bar");
                        return Some(control);
                    }
                }
            }
        }

        godot_print!("[godot-neovim] Status bar not found");
        None
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
        self.reposition_mode_label();
        self.sync_buffer_to_neovim();
    }

    fn reposition_mode_label(&mut self) {
        // Check if label is still valid (may have been freed with previous status bar)
        let label_valid = self.mode_label.as_ref().map_or(false, |label| {
            label.is_instance_valid()
        });

        if !label_valid {
            // Label was freed, create a new one
            self.mode_label = None;
            self.create_mode_label();
            return;
        }

        let Some(ref label) = self.mode_label else {
            return;
        };

        // Check if label needs to be moved to status bar
        if let Some(code_edit) = &self.current_editor {
            if let Some(mut status_bar) = self.find_status_bar(code_edit.clone().upcast()) {
                // Check if already in this status bar
                if let Some(mut parent) = label.get_parent() {
                    if parent.instance_id() == status_bar.instance_id() {
                        return; // Already in correct position
                    }
                    // Remove from current parent
                    parent.remove_child(label);
                }

                // Add to status bar
                status_bar.add_child(label);
                status_bar.move_child(label, 0);
                godot_print!("[godot-neovim] Mode label moved to status bar");
            }
        }
    }

    fn find_current_code_edit(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(script_editor) = editor.get_script_editor() {
            // Try to find the currently focused CodeEdit first
            if let Some(code_edit) = self.find_focused_code_edit(script_editor.clone().upcast::<Control>()) {
                godot_print!("[godot-neovim] Found focused CodeEdit");
                self.current_editor = Some(code_edit);
                return;
            }
            // Fallback: find visible CodeEdit
            if let Some(code_edit) = self.find_visible_code_edit(script_editor.upcast::<Control>()) {
                godot_print!("[godot-neovim] Found visible CodeEdit");
                self.current_editor = Some(code_edit);
            }
        }
    }

    fn find_focused_code_edit(&self, node: Gd<Control>) -> Option<Gd<CodeEdit>> {
        // Check if this node is a focused CodeEdit
        if let Ok(code_edit) = node.clone().try_cast::<CodeEdit>() {
            if code_edit.has_focus() {
                return Some(code_edit);
            }
        }

        // Search children
        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Ok(control) = child.try_cast::<Control>() {
                    if let Some(code_edit) = self.find_focused_code_edit(control) {
                        return Some(code_edit);
                    }
                }
            }
        }

        None
    }

    fn find_visible_code_edit(&self, node: Gd<Control>) -> Option<Gd<CodeEdit>> {
        // Check if this node is a visible CodeEdit
        if let Ok(code_edit) = node.clone().try_cast::<CodeEdit>() {
            if code_edit.is_visible_in_tree() {
                return Some(code_edit);
            }
        }

        // Search children
        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Ok(control) = child.try_cast::<Control>() {
                    if let Some(code_edit) = self.find_visible_code_edit(control) {
                        return Some(code_edit);
                    }
                }
            }
        }

        None
    }

    fn sync_buffer_to_neovim(&mut self) {
        let Some(ref editor) = self.current_editor else {
            godot_print!("[godot-neovim] sync_buffer_to_neovim: No current editor");
            return;
        };

        let Some(ref neovim) = self.neovim else {
            godot_print!("[godot-neovim] sync_buffer_to_neovim: No neovim");
            return;
        };

        let Ok(client) = neovim.lock() else {
            godot_print!("[godot-neovim] sync_buffer_to_neovim: Failed to lock");
            return;
        };

        // Get text from Godot editor
        let text = editor.get_text().to_string();
        let lines: Vec<String> = text.lines().map(String::from).collect();

        godot_print!("[godot-neovim] Syncing {} lines to Neovim", lines.len());
        if !lines.is_empty() {
            godot_print!("[godot-neovim] First line: '{}'", lines[0].chars().take(50).collect::<String>());
        }

        // Set buffer content in Neovim
        if let Err(e) = client.set_buffer_lines(0, -1, lines) {
            godot_error!("[godot-neovim] Failed to sync buffer: {}", e);
        } else {
            godot_print!("[godot-neovim] Buffer synced to Neovim successfully");
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

        // Ctrl+[ is equivalent to Escape (terminal standard)
        if ctrl && keycode == Key::BRACKETLEFT {
            return Some("<Esc>".to_string());
        }

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

    /// Check if currently in insert mode
    fn is_insert_mode(&self) -> bool {
        self.current_mode == "i"
    }

    /// Process pending updates from Neovim redraw events
    fn process_neovim_updates(&mut self) {
        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Poll the runtime to process async events
        client.poll();

        // Check if there are pending updates
        if let Some((mode, cursor)) = client.take_state() {
            godot_print!("[godot-neovim] Got update: mode={}, cursor=({}, {})", mode, cursor.0, cursor.1);

            // Release lock before updating UI
            drop(client);

            let old_mode = self.current_mode.clone();
            self.current_mode = mode.clone();
            self.current_cursor = cursor;

            // Update mode display
            // Convert grid cursor (0-indexed) to Neovim cursor (1-indexed for display)
            let display_cursor = (cursor.0 + 1, cursor.1);
            self.update_mode_display_with_cursor(&mode, Some(display_cursor));

            // Sync cursor to Godot editor
            self.sync_cursor_from_grid(cursor);

            // If exiting insert mode, sync buffer from Godot to Neovim
            if old_mode == "i" && mode != "i" {
                self.sync_buffer_to_neovim();
            }
        }
    }

    /// Send Escape to Neovim and force mode to normal
    fn send_escape(&mut self) {
        godot_print!("[godot-neovim] send_escape");

        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Send Escape to Neovim
        if let Err(e) = client.input("<Esc>") {
            godot_error!("[godot-neovim] Failed to send Escape: {}", e);
            return;
        }

        // Release lock
        drop(client);

        // Force mode to normal (ESC always returns to normal mode)
        self.current_mode = "n".to_string();
        self.update_mode_display_with_cursor("n", None);

        godot_print!("[godot-neovim] Escaped to normal mode");
    }

    /// Send keys to Neovim and update state
    fn send_keys(&mut self, keys: &str) {
        godot_print!("[godot-neovim] send_keys: {}", keys);

        let Some(ref neovim) = self.neovim else {
            godot_print!("[godot-neovim] No neovim");
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            godot_warn!("[godot-neovim] Mutex busy, dropping key: {}", keys);
            return;
        };

        // Send input to Neovim
        if let Err(e) = client.input(keys) {
            godot_error!("[godot-neovim] Failed to send keys: {}", e);
            return;
        }
        godot_print!("[godot-neovim] Key sent successfully");

        // Query mode - if blocking (operator-pending), skip cursor sync
        let (mode, blocking) = client.get_mode();

        if blocking {
            godot_print!("[godot-neovim] Operator pending, skipping sync");
            return;
        }

        // Query cursor
        let cursor = client.get_cursor().unwrap_or((1, 0));

        godot_print!("[godot-neovim] After key: mode={}, cursor=({}, {})", mode, cursor.0, cursor.1);

        // Release lock before updating UI
        drop(client);

        // Update state
        self.current_mode = mode.clone();
        self.current_cursor = (cursor.0 - 1, cursor.1); // Convert to 0-indexed

        // Update mode display
        self.update_mode_display_with_cursor(&mode, Some(cursor));

        // Sync cursor to Godot editor
        self.sync_cursor_from_grid((cursor.0 - 1, cursor.1));
    }

    /// Sync cursor from grid position (0-indexed)
    fn sync_cursor_from_grid(&mut self, cursor: (i64, i64)) {
        let Some(ref mut editor) = self.current_editor else {
            godot_print!("[godot-neovim] sync_cursor_from_grid: No current editor");
            return;
        };

        let (row, col) = cursor;
        godot_print!("[godot-neovim] sync_cursor_from_grid: Setting cursor to row={}, col={}", row, col);
        editor.set_caret_line(row as i32);
        editor.set_caret_column(col as i32);
    }

    fn sync_cursor_from_neovim(&mut self, cursor: Option<(i64, i64)>) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        if let Some((line, col)) = cursor {
            editor.set_caret_line((line - 1) as i32);
            editor.set_caret_column(col as i32);
        }
    }

    fn update_mode_display(&mut self, mode: &str) {
        self.update_mode_display_with_cursor(mode, None);
    }

    fn update_mode_display_with_cursor(&mut self, mode: &str, cursor: Option<(i64, i64)>) {
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

        // Format: "NORMAL 123:45" (line:col)
        let display_text = if let Some((line, col)) = cursor {
            format!("{} {}:{}", mode_text, line, col)
        } else {
            mode_text.to_string()
        };

        if let Some(ref mut label) = self.mode_label {
            label.set_text(&display_text);

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
            godot_print!("[godot-neovim] No current editor for buffer sync");
            return;
        };

        godot_print!("[godot-neovim] Syncing buffer from Neovim: {} lines", lines.len());
        if !lines.is_empty() {
            godot_print!("[godot-neovim] First line from Neovim: '{}'", lines[0].chars().take(50).collect::<String>());
            if lines.len() > 1 {
                godot_print!("[godot-neovim] Last line from Neovim: '{}'", lines[lines.len()-1].chars().take(50).collect::<String>());
            }
        }

        // Update Godot editor
        let text = lines.join("\n");
        godot_print!("[godot-neovim] Setting text ({} chars)", text.len());
        editor.set_text(&text);

        // Update cursor position
        if let Some((line, col)) = cursor {
            godot_print!("[godot-neovim] Setting cursor to line {}, col {}", line, col);
            editor.set_caret_line((line - 1) as i32); // Neovim is 1-indexed
            editor.set_caret_column(col as i32);
        }
    }
}
