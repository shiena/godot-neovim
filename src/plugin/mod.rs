//! Godot Neovim Plugin - Main module

mod commands;
mod editing;
mod keys;
mod macros;
mod marks;
mod motions;
mod neovim;
mod registers;
mod search;
mod visual;

use crate::neovim::NeovimClient;
use crate::settings;
use godot::classes::text_edit::CaretType;
use godot::classes::{
    CodeEdit, Control, EditorInterface, EditorPlugin, IEditorPlugin, Label,
};
use godot::global::Key;
use godot::prelude::*;
use std::collections::HashMap;
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
    /// Last key sent to Neovim (for detecting sequences like zz, zt, zb)
    #[init(val = String::new())]
    last_key: String,
    /// Command line input buffer for ':' commands
    #[init(val = String::new())]
    command_buffer: String,
    /// Flag indicating command-line mode is active
    #[init(val = false)]
    command_mode: bool,
    /// Last searched word (for n/N repeat)
    #[init(val = String::new())]
    last_search_word: String,
    /// Last search direction (true = forward, false = backward)
    #[init(val = true)]
    last_search_forward: bool,
    /// Last find character (for ;/, repeat)
    #[init(val = None)]
    last_find_char: Option<char>,
    /// Last find direction (true = forward f/t, false = backward F/T)
    #[init(val = true)]
    last_find_forward: bool,
    /// Last find was till (t/T) vs on (f/F)
    #[init(val = false)]
    last_find_till: bool,
    /// Pending operator waiting for character input (f, F, t, T, r)
    #[init(val = None)]
    pending_char_op: Option<char>,
    /// Command history for ':' commands
    #[init(val = Vec::new())]
    command_history: Vec<String>,
    /// Current position in command history (None = not browsing history)
    #[init(val = None)]
    command_history_index: Option<usize>,
    /// Temporary buffer for current input when browsing history
    #[init(val = String::new())]
    command_history_temp: String,
    /// Marks storage: char -> (line, col) - 0-indexed
    #[init(val = HashMap::new())]
    marks: HashMap<char, (i32, i32)>,
    /// Pending mark operation: Some('m') for set mark, Some('\'') for jump to line, Some('`') for jump to position
    #[init(val = None)]
    pending_mark_op: Option<char>,
    /// Macro storage: char -> Vec of key sequences
    #[init(val = HashMap::new())]
    macros: HashMap<char, Vec<String>>,
    /// Currently recording macro (None if not recording)
    #[init(val = None)]
    recording_macro: Option<char>,
    /// Buffer for keys being recorded
    #[init(val = Vec::new())]
    macro_buffer: Vec<String>,
    /// Last played macro register (for @@)
    #[init(val = None)]
    last_macro: Option<char>,
    /// Flag to prevent recursive macro recording
    #[init(val = false)]
    playing_macro: bool,
    /// Pending macro operation: Some('q') for record, Some('@') for play
    #[init(val = None)]
    pending_macro_op: Option<char>,
    /// Named registers storage: char -> content
    #[init(val = HashMap::new())]
    registers: HashMap<char, String>,
    /// Currently selected register for next yank/paste (None = default/system clipboard)
    #[init(val = None)]
    selected_register: Option<char>,
    /// Jump list: stores (line, col) positions for Ctrl+O/Ctrl+I navigation
    #[init(val = Vec::new())]
    jump_list: Vec<(i32, i32)>,
    /// Current position in jump list (index into jump_list, or len() if at end)
    #[init(val = 0)]
    jump_list_pos: usize,
    /// Count prefix buffer for commands like 3dd, 5yy
    #[init(val = String::new())]
    count_buffer: String,
    /// Last substitution: (pattern, replacement) for g& command
    #[init(val = None)]
    last_substitute: Option<(String, String)>,
    /// Last insert position: (line, col) for gi command
    #[init(val = None)]
    last_insert_position: Option<(i32, i32)>,
}

#[godot_api]
impl IEditorPlugin for GodotNeovimPlugin {
    fn enter_tree(&mut self) {
        crate::verbose_print!("[godot-neovim] Plugin entering tree");

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
            crate::verbose_print!("[godot-neovim] Found existing CodeEdit, syncing initial buffer");
            self.reposition_mode_label();
            self.sync_buffer_to_neovim();
            self.update_cursor_from_editor();
            self.sync_cursor_to_neovim();
        }

        // Enable process() to be called every frame for checking redraw events
        self.base_mut().set_process(true);

        crate::verbose_print!("[godot-neovim] Plugin initialized successfully");
    }

    fn exit_tree(&mut self) {
        crate::verbose_print!("[godot-neovim] Plugin exiting tree");

        // Cleanup mode label (check if still valid before freeing)
        if let Some(mut label) = self.mode_label.take() {
            if label.is_instance_valid() {
                label.queue_free();
            }
        }

        // Clear current editor reference
        self.current_editor = None;

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
            crate::verbose_print!("[godot-neovim] input: No neovim");
            return;
        }

        crate::verbose_print!(
            "[godot-neovim] input: mode={}, key={:?}",
            self.current_mode,
            key_event.get_keycode()
        );

        // Handle command-line mode input
        if self.command_mode {
            self.handle_command_mode_input(&key_event);
            return;
        }

        // Handle pending character operator (f, F, t, T, r)
        if self.handle_pending_char_op(&key_event) {
            return;
        }

        // Handle pending mark operation (m, ', `)
        if self.handle_pending_mark_op(&key_event) {
            return;
        }

        // Handle pending macro operation (q for record, @ for play)
        if self.handle_pending_macro_op(&key_event) {
            return;
        }

        // Handle pending register selection (waiting for register char after ")
        if self.handle_pending_register(&key_event) {
            return;
        }

        // Handle insert mode
        if self.is_insert_mode() {
            self.handle_insert_mode_input(&key_event);
            return;
        }

        // Handle replace mode
        if self.is_replace_mode() {
            self.handle_replace_mode_input(&key_event);
            return;
        }

        // Handle normal/visual mode input
        self.handle_normal_mode_input(&key_event);
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
        let parent = node.get_parent()?;

        let parent_class = parent.get_class().to_string();
        crate::verbose_print!(
            "[godot-neovim] CodeEdit parent: {} ({})",
            parent.get_name(),
            parent_class
        );

        // Search siblings for HBoxContainer (status bar)
        let child_count = parent.get_child_count();
        for i in 0..child_count {
            if let Some(child) = parent.get_child(i) {
                let class_name = child.get_class().to_string();
                if class_name == "HBoxContainer" {
                    if let Ok(control) = child.try_cast::<Control>() {
                        crate::verbose_print!("[godot-neovim] Found HBoxContainer status bar");
                        return Some(control);
                    }
                }
            }
        }

        crate::verbose_print!("[godot-neovim] Status bar not found");
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
        crate::verbose_print!("[godot-neovim] Script changed");
        self.find_current_code_edit();
        self.reposition_mode_label();
        self.sync_buffer_to_neovim();
        self.update_cursor_from_editor();
        self.sync_cursor_to_neovim();
    }

    fn reposition_mode_label(&mut self) {
        // Check if label is still valid (may have been freed with previous status bar)
        let label_valid = self
            .mode_label
            .as_ref()
            .is_some_and(|label| label.is_instance_valid());

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
                crate::verbose_print!("[godot-neovim] Mode label moved to status bar");
            }
        }
    }

    fn find_current_code_edit(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(script_editor) = editor.get_script_editor() {
            // Try to find the currently focused CodeEdit first
            if let Some(code_edit) =
                self.find_focused_code_edit(script_editor.clone().upcast::<Control>())
            {
                crate::verbose_print!("[godot-neovim] Found focused CodeEdit");
                self.current_editor = Some(code_edit);
                return;
            }
            // Fallback: find visible CodeEdit
            if let Some(code_edit) = self.find_visible_code_edit(script_editor.upcast::<Control>())
            {
                crate::verbose_print!("[godot-neovim] Found visible CodeEdit");
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

    fn editor_has_focus(&self) -> bool {
        if let Some(ref editor) = self.current_editor {
            // Check if editor instance is still valid (not freed)
            if editor.is_instance_valid() {
                return editor.has_focus();
            }
        }
        false
    }

    /// Check if currently in insert mode
    fn is_insert_mode(&self) -> bool {
        self.current_mode == "i"
    }

    /// Check if currently in replace mode
    fn is_replace_mode(&self) -> bool {
        self.current_mode == "R"
    }

    /// Check if mode is a visual mode (v, V, or Ctrl+V)
    fn is_visual_mode(mode: &str) -> bool {
        matches!(mode, "v" | "V" | "\x16" | "^V" | "CTRL-V")
    }

    /// Mark input as handled on the CodeEdit's viewport
    /// This prevents CodeEdit from processing the input event
    #[allow(dead_code)]
    fn consume_input_on_editor(&self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        // Use CodeEdit's viewport, not the plugin's viewport
        if let Some(mut viewport) = editor.get_viewport() {
            viewport.set_input_as_handled();
        }
    }

    fn update_mode_display(&mut self, mode: &str) {
        self.update_mode_display_with_cursor(mode, None);
    }

    fn update_mode_display_with_cursor(&mut self, mode: &str, cursor: Option<(i64, i64)>) {
        let Some(ref mut label) = self.mode_label else {
            return;
        };

        // Get mode display name
        let mode_name = match mode {
            "n" => "NORMAL",
            "i" | "insert" => "INSERT",
            "v" | "visual" => "VISUAL",
            "V" | "visual-line" => "V-LINE",
            "\x16" | "^V" | "CTRL-V" | "visual-block" => "V-BLOCK",
            "c" | "command" => "COMMAND",
            "R" | "replace" => "REPLACE",
            _ => mode,
        };

        // Format with cursor position if available
        let display_text = if let Some((line, col)) = cursor {
            format!(" {} {}:{} ", mode_name, line, col)
        } else {
            format!(" {} ", mode_name)
        };

        label.set_text(&display_text);

        // Set color based on mode
        let color = match mode {
            "n" => Color::from_rgb(0.0, 1.0, 0.5),   // Green for normal
            "i" | "insert" => Color::from_rgb(0.4, 0.6, 1.0), // Blue for insert
            "R" | "replace" => Color::from_rgb(1.0, 0.3, 0.3), // Red for replace
            "v" | "V" | "\x16" | "^V" | "CTRL-V" | "visual" | "visual-line" | "visual-block" => {
                Color::from_rgb(1.0, 0.6, 0.2) // Orange for visual
            }
            "c" | "command" => Color::from_rgb(1.0, 1.0, 0.4), // Yellow for command
            _ => Color::from_rgb(1.0, 1.0, 1.0),  // White for unknown
        };

        label.add_theme_color_override("font_color", color);

        // Update caret type based on mode
        // Normal mode: block cursor, Insert mode: line cursor
        if let Some(ref mut editor) = self.current_editor {
            let caret_type = match mode {
                "i" | "insert" | "R" | "replace" => CaretType::LINE,
                _ => CaretType::BLOCK,
            };
            editor.set_caret_type(caret_type);
        }
    }
}

// Input handling (separate impl block for organization)
impl GodotNeovimPlugin {
    fn handle_command_mode_input(&mut self, key_event: &Gd<godot::classes::InputEventKey>) {
        let keycode = key_event.get_keycode();

        if keycode == Key::ESCAPE {
            self.close_command_line();
        } else if keycode == Key::ENTER {
            self.execute_command();
        } else if keycode == Key::BACKSPACE {
            // Remove last character (but keep the ':')
            if self.command_buffer.len() > 1 {
                self.command_buffer.pop();
                self.update_command_display();
            }
            // Reset history browsing when editing
            self.command_history_index = None;
        } else if keycode == Key::UP {
            // Browse command history (older)
            self.command_history_up();
        } else if keycode == Key::DOWN {
            // Browse command history (newer)
            self.command_history_down();
        } else {
            // Append character to command buffer
            let unicode = key_event.get_unicode();
            if unicode > 0 {
                if let Some(c) = char::from_u32(unicode) {
                    self.command_buffer.push(c);
                    self.update_command_display();
                    // Reset history browsing when typing
                    self.command_history_index = None;
                }
            }
        }

        if let Some(mut viewport) = self.base().get_viewport() {
            viewport.set_input_as_handled();
        }
    }

    fn handle_pending_char_op(&mut self, key_event: &Gd<godot::classes::InputEventKey>) -> bool {
        let Some(op) = self.pending_char_op else {
            return false;
        };

        let keycode = key_event.get_keycode();

        // Cancel on Escape
        if keycode == Key::ESCAPE {
            self.pending_char_op = None;
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return true;
        }

        // Get the character
        let unicode = key_event.get_unicode();
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                self.pending_char_op = None;
                match op {
                    'f' => self.find_char_forward(c, false),
                    'F' => self.find_char_backward(c, false),
                    't' => self.find_char_forward(c, true),
                    'T' => self.find_char_backward(c, true),
                    'r' => self.replace_char(c),
                    _ => {}
                }
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return true;
            }
        }
        false
    }

    fn handle_pending_mark_op(&mut self, key_event: &Gd<godot::classes::InputEventKey>) -> bool {
        let Some(op) = self.pending_mark_op else {
            return false;
        };

        let keycode = key_event.get_keycode();

        // Cancel on Escape
        if keycode == Key::ESCAPE {
            self.pending_mark_op = None;
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return true;
        }

        // Get the character (must be a-z for marks)
        let unicode = key_event.get_unicode();
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                if c.is_ascii_lowercase() {
                    self.pending_mark_op = None;
                    match op {
                        'm' => self.set_mark(c),
                        '\'' => self.jump_to_mark_line(c),
                        '`' => self.jump_to_mark_position(c),
                        _ => {}
                    }
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return true;
                }
            }
        }
        false
    }

    fn handle_pending_macro_op(&mut self, key_event: &Gd<godot::classes::InputEventKey>) -> bool {
        let Some(op) = self.pending_macro_op else {
            return false;
        };

        let keycode = key_event.get_keycode();

        // Cancel on Escape
        if keycode == Key::ESCAPE {
            self.pending_macro_op = None;
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return true;
        }

        // Get the character
        let unicode = key_event.get_unicode();
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                self.pending_macro_op = None;
                match op {
                    'q' => {
                        // Start recording if a-z
                        if c.is_ascii_lowercase() {
                            self.start_macro_recording(c);
                        }
                    }
                    '@' => {
                        if c == '@' {
                            // @@ - replay last macro
                            self.replay_last_macro();
                        } else if c == ':' {
                            // @: - repeat last Ex command
                            self.repeat_last_ex_command();
                        } else if c.is_ascii_lowercase() {
                            // @{a-z} - play specific macro
                            self.play_macro(c);
                        }
                    }
                    _ => {}
                }
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return true;
            }
        }
        false
    }

    fn handle_pending_register(&mut self, key_event: &Gd<godot::classes::InputEventKey>) -> bool {
        if self.selected_register != Some('\0') {
            return false;
        }

        let keycode = key_event.get_keycode();

        // Cancel on Escape
        if keycode == Key::ESCAPE {
            self.selected_register = None;
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return true;
        }

        // Get the character
        // Valid registers: a-z (named), + and * (clipboard), _ (black hole), 0 (yank)
        let unicode = key_event.get_unicode();
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                let is_valid_register = c.is_ascii_lowercase()
                    || c == '+'
                    || c == '*'
                    || c == '_'
                    || c == '0';
                if is_valid_register {
                    self.selected_register = Some(c);
                    crate::verbose_print!("[godot-neovim] \"{}: Register selected", c);
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return true;
                }
            }
        }
        false
    }

    fn handle_insert_mode_input(&mut self, key_event: &Gd<godot::classes::InputEventKey>) {
        // Intercept Escape or Ctrl+[ to exit insert mode (always)
        let is_escape = key_event.get_keycode() == Key::ESCAPE;
        let is_ctrl_bracket =
            key_event.is_ctrl_pressed() && key_event.get_keycode() == Key::BRACKETLEFT;

        if is_escape || is_ctrl_bracket {
            self.send_escape();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Ctrl+B in insert mode: exit insert and enter visual block mode (always)
        let is_ctrl_b = key_event.is_ctrl_pressed() && key_event.get_keycode() == Key::B;
        if is_ctrl_b {
            // First sync buffer and exit insert mode
            self.send_escape();
            // Then enter visual block mode
            let completed = self.send_keys("<C-v>");
            if completed {
                self.last_key.clear();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Check input mode setting
        let input_mode = settings::get_input_mode();
        if input_mode == settings::InputMode::Hybrid {
            // Hybrid mode: Let Godot handle other keys in insert mode (IME support)
            return;
        }

        // Strict mode: Send keys to Neovim
        let nvim_key = self.key_event_to_nvim_notation(key_event);
        if !nvim_key.is_empty() {
            self.send_keys_insert_mode(&nvim_key);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
        }
    }

    fn handle_replace_mode_input(&mut self, key_event: &Gd<godot::classes::InputEventKey>) {
        // Intercept Escape or Ctrl+[ to exit replace mode (always)
        let is_escape = key_event.get_keycode() == Key::ESCAPE;
        let is_ctrl_bracket =
            key_event.is_ctrl_pressed() && key_event.get_keycode() == Key::BRACKETLEFT;

        if is_escape || is_ctrl_bracket {
            self.send_escape();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Check input mode setting
        let input_mode = settings::get_input_mode();
        if input_mode == settings::InputMode::Hybrid {
            // Hybrid mode: implement overwrite behavior
            // Delete character at cursor position, then let Godot insert the new one
            let unicode = key_event.get_unicode();
            if unicode > 0 {
                if let Some(ref mut editor) = self.current_editor {
                    let line = editor.get_caret_line();
                    let col = editor.get_caret_column();
                    let line_text: String = editor.get_line(line).to_string();

                    // Only delete if we're not at end of line
                    if (col as usize) < line_text.chars().count() {
                        // Delete character at cursor
                        editor.select(line, col, line, col + 1);
                        editor.delete_selection();
                    }
                }
            }
            // Let Godot insert the character
            return;
        }

        // Strict mode: Send keys to Neovim
        let nvim_key = self.key_event_to_nvim_notation(key_event);
        if !nvim_key.is_empty() {
            self.send_keys_insert_mode(&nvim_key);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
        }
    }

    fn handle_normal_mode_input(&mut self, key_event: &Gd<godot::classes::InputEventKey>) {
        let keycode = key_event.get_keycode();
        let unicode_char = char::from_u32(key_event.get_unicode());

        // Handle Ctrl+B: visual block in visual mode, page up in normal mode
        if key_event.is_ctrl_pressed() && keycode == Key::B {
            if Self::is_visual_mode(&self.current_mode) {
                // In visual mode: switch to visual block (Ctrl+V alternative since Godot intercepts it)
                let completed = self.send_keys("<C-v>");
                if completed {
                    self.last_key.clear();
                }
            } else {
                // In normal mode: page up
                self.page_up();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'o' in visual mode: toggle selection direction
        if Self::is_visual_mode(&self.current_mode)
            && keycode == Key::O
            && !key_event.is_ctrl_pressed()
            && !key_event.is_shift_pressed()
        {
            // Send 'o' to Neovim to toggle selection direction
            self.send_keys("o");
            // Update selection display (Neovim will swap anchor and cursor)
            if self.current_mode == "v" {
                self.update_visual_selection();
            } else if self.current_mode == "V" {
                self.update_visual_line_selection();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            crate::verbose_print!("[godot-neovim] o: Toggle visual selection direction");
            return;
        }

        // Handle Ctrl+F for page down
        if key_event.is_ctrl_pressed() && keycode == Key::F {
            self.page_down();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+Y/Ctrl+E for viewport scrolling (cursor stays on same line)
        if key_event.is_ctrl_pressed() && (keycode == Key::Y || keycode == Key::E) {
            if keycode == Key::Y {
                self.scroll_viewport_up();
            } else {
                self.scroll_viewport_down();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+A for increment number under cursor
        if key_event.is_ctrl_pressed() && keycode == Key::A {
            self.increment_number(1);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+X for decrement number under cursor
        if key_event.is_ctrl_pressed() && keycode == Key::X {
            self.increment_number(-1);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+O for jump back in jump list
        if key_event.is_ctrl_pressed() && keycode == Key::O {
            self.jump_back();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+I (Tab) for jump forward in jump list
        if key_event.is_ctrl_pressed() && keycode == Key::I {
            self.jump_forward();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+G for file info
        if key_event.is_ctrl_pressed() && keycode == Key::G {
            self.show_file_info();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '/' for search - open Godot's find dialog
        if keycode == Key::SLASH && !key_event.is_ctrl_pressed() && !key_event.is_shift_pressed() {
            self.open_find_dialog();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ':' for command-line mode (use unicode for cross-keyboard support)
        if unicode_char == Some(':') {
            self.open_command_line();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '*' for search forward word under cursor (use unicode for JIS keyboard support)
        if unicode_char == Some('*') {
            self.search_word_forward();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '#' for search backward word under cursor
        if unicode_char == Some('#') {
            self.search_word_backward();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'n' for repeat search forward
        if keycode == Key::N && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.repeat_search(true);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'N' for repeat search backward
        if keycode == Key::N && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.repeat_search(false);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'u' for undo (but not after 'g' - that's 'gu' for lowercase)
        if keycode == Key::U
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            self.undo();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'Ctrl+R' for redo
        if keycode == Key::R && key_event.is_ctrl_pressed() {
            self.redo();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'f' for find char forward
        if keycode == Key::F && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.pending_char_op = Some('f');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'F' for find char backward
        if keycode == Key::F && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.pending_char_op = Some('F');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 't' for till char forward
        if keycode == Key::T && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.pending_char_op = Some('t');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'T' for till char backward
        if keycode == Key::T && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.pending_char_op = Some('T');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ';' for repeat find char same direction
        if keycode == Key::SEMICOLON && !key_event.is_shift_pressed() {
            self.repeat_find_char(true);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ',' for repeat find char opposite direction
        if keycode == Key::COMMA && !key_event.is_shift_pressed() {
            self.repeat_find_char(false);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '%' for matching bracket
        if unicode_char == Some('%') {
            self.jump_to_matching_bracket();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '0' for go to start of line
        if unicode_char == Some('0') && !key_event.is_ctrl_pressed() {
            self.move_to_line_start();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '^' for go to first non-blank
        if unicode_char == Some('^') {
            self.move_to_first_non_blank();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '$' for go to end of line
        if unicode_char == Some('$') {
            self.move_to_line_end();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '{' for previous paragraph
        if unicode_char == Some('{') {
            self.move_to_prev_paragraph();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '}' for next paragraph
        if unicode_char == Some('}') {
            self.move_to_next_paragraph();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'x' for delete char under cursor
        if keycode == Key::X && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.delete_char_forward();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'X' for delete char before cursor
        if keycode == Key::X && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.delete_char_backward();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'Y' for yank to end of line
        if keycode == Key::Y && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.yank_to_eol();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'D' for delete to end of line
        if keycode == Key::D && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.delete_to_eol();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'C' for change to end of line
        if keycode == Key::C && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.change_to_eol();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 's' for substitute char (delete char and enter insert mode)
        if keycode == Key::S && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.substitute_char();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'S' for substitute line (delete line content and enter insert mode)
        if keycode == Key::S && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.substitute_line();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'cc' for substitute line (same as S)
        if keycode == Key::C && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.last_key == "c" {
                self.substitute_line();
                self.last_key.clear();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            } else {
                self.last_key = "c".to_string();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
        }

        // Handle 'r' for replace char
        if keycode == Key::R && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.pending_char_op = Some('r');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'R' for replace mode (continuous overwrite)
        if keycode == Key::R && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.enter_replace_mode();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '~' for toggle case (use unicode for keyboard layout independence)
        if unicode_char == Some('~') {
            self.toggle_case();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'm' for set mark
        if keycode == Key::M && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.pending_mark_op = Some('m');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '\'' (single quote) for jump to mark line
        if unicode_char == Some('\'') && !key_event.is_ctrl_pressed() {
            self.pending_mark_op = Some('\'');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '`' (backtick) for jump to mark position
        if unicode_char == Some('`') && !key_event.is_ctrl_pressed() {
            self.pending_mark_op = Some('`');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'q' for macro recording (start/stop)
        if keycode == Key::Q && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.recording_macro.is_some() {
                // Stop recording
                self.stop_macro_recording();
            } else {
                // Wait for register character
                self.pending_macro_op = Some('q');
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '@' for macro playback
        if unicode_char == Some('@') && !key_event.is_ctrl_pressed() {
            self.pending_macro_op = Some('@');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '"' for register selection
        if unicode_char == Some('"') && !key_event.is_ctrl_pressed() {
            // Use '\0' as marker for "waiting for register char"
            self.selected_register = Some('\0');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '>>' for indent (first '>' sets pending, second '>' executes)
        // Handle '<<' for unindent (first '<' sets pending, second '<' executes)
        // Use unicode for keyboard layout independence
        if unicode_char == Some('>') {
            if self.last_key == ">" {
                self.indent_line();
                self.last_key.clear();
            } else {
                self.last_key = ">".to_string();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        if unicode_char == Some('<') {
            if self.last_key == "<" {
                self.unindent_line();
                self.last_key.clear();
            } else {
                self.last_key = "<".to_string();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '==' for auto-indent current line, '=G' for indent to end of file
        if unicode_char == Some('=') {
            if self.last_key == "=" {
                self.auto_indent_line();
                self.last_key.clear();
            } else {
                self.last_key = "=".to_string();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '=G' (auto-indent to end of file)
        if self.last_key == "="
            && keycode == Key::G
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
        {
            self.auto_indent_to_end();
            self.last_key.clear();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '[p' for paste before with indent adjustment
        if unicode_char == Some('[') {
            self.last_key = "[".to_string();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ']p' for paste after with indent adjustment
        if unicode_char == Some(']') {
            self.last_key = "]".to_string();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle p after [ or ]
        if keycode == Key::P && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.last_key == "[" {
                self.paste_with_indent_before();
                self.last_key.clear();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            } else if self.last_key == "]" {
                self.paste_with_indent_after();
                self.last_key.clear();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
        }

        // Handle '?' for backward search
        if unicode_char == Some('?') && !key_event.is_ctrl_pressed() {
            self.start_search_backward();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'J' for join lines
        if keycode == Key::J && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.join_lines();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+D for half page down
        if key_event.is_ctrl_pressed() && keycode == Key::D {
            self.half_page_down();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+U for half page up
        if key_event.is_ctrl_pressed() && keycode == Key::U {
            self.half_page_up();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle H/M/L based on Godot's visible area (not Neovim's)
        if !key_event.is_ctrl_pressed()
            && !key_event.is_alt_pressed()
            && (keycode == Key::H || keycode == Key::M || keycode == Key::L)
            && key_event.is_shift_pressed()
        {
            // Shift+h/m/l = H/M/L (uppercase)
            match keycode {
                Key::H => self.move_cursor_to_visible_top(),
                Key::M => self.move_cursor_to_visible_middle(),
                Key::L => self.move_cursor_to_visible_bottom(),
                _ => {}
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Z-prefixed commands (ZZ, ZQ) - intercept before forwarding to Neovim
        if keycode == Key::Z && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.last_key == "Z" {
                // Second Z - this is ZZ (save and close)
                self.cmd_save_and_close();
                self.last_key.clear();
            } else {
                // First Z - wait for next key
                self.last_key = "Z".to_string();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ZQ (Z then Q) - close without saving
        if keycode == Key::Q
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key == "Z"
        {
            // ZQ - close without saving (discard changes)
            self.cmd_close_discard();
            self.last_key.clear();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Clear Z prefix if another key is pressed (not Z or Q)
        if self.last_key == "Z" && keycode != Key::Z && keycode != Key::Q {
            self.last_key.clear();
        }

        // Handle register-aware yy (yank line)
        if let Some(reg) = self.selected_register {
            if reg != '\0' {
                // Handle count prefix (digits 1-9, or 0 if count_buffer not empty)
                if let Some(c) = unicode_char {
                    if c.is_ascii_digit() && (c != '0' || !self.count_buffer.is_empty()) {
                        self.count_buffer.push(c);
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                }

                // Register is selected, check for yy
                if keycode == Key::Y
                    && !key_event.is_shift_pressed()
                    && !key_event.is_ctrl_pressed()
                {
                    if self.last_key == "y" {
                        // yy - yank current line(s) to register
                        let count = self.get_and_clear_count();
                        self.yank_lines_to_register(reg, count);
                        self.selected_register = None;
                        self.last_key.clear();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    } else {
                        // First y - wait for second
                        self.last_key = "y".to_string();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                }

                // Handle register-aware p (paste)
                if keycode == Key::P
                    && !key_event.is_shift_pressed()
                    && !key_event.is_ctrl_pressed()
                {
                    self.paste_from_register(reg);
                    self.selected_register = None;
                    self.count_buffer.clear();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }

                // Handle register-aware P (paste before)
                if keycode == Key::P && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed()
                {
                    self.paste_from_register_before(reg);
                    self.selected_register = None;
                    self.count_buffer.clear();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }

                // Handle register-aware dd (delete line and yank)
                if keycode == Key::D
                    && !key_event.is_shift_pressed()
                    && !key_event.is_ctrl_pressed()
                {
                    if self.last_key == "d" {
                        // dd - delete line(s) and store in register
                        let count = self.get_and_clear_count();
                        self.delete_lines_to_register(reg, count);
                        self.selected_register = None;
                        self.last_key.clear();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    } else {
                        // First d - wait for second
                        self.last_key = "d".to_string();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                }

                // Other keys cancel register selection
                if keycode != Key::Y && keycode != Key::D {
                    self.selected_register = None;
                    self.count_buffer.clear();
                }
            }
        }

        // Forward key to Neovim (normal/visual/etc modes)
        if let Some(keys) = self.key_event_to_nvim_string(key_event) {
            // Record key for macro if recording (and not playing back)
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push(keys.clone());
            }

            let completed = self.send_keys(&keys);

            // Handle scroll commands (zz, zt, zb) only if command completed
            let scroll_handled = if completed {
                self.handle_scroll_command(&keys)
            } else {
                false
            };

            // Handle g-prefixed commands
            if completed && self.last_key == "g" {
                let handled = match keys.as_str() {
                    "d" => {
                        // gd - go to definition (use Godot's built-in)
                        self.add_to_jump_list();
                        self.go_to_definition();
                        true
                    }
                    "v" => {
                        // gv - enter visual block mode (alternative to Ctrl+V)
                        self.send_keys("<C-v>");
                        true
                    }
                    "t" => {
                        // gt - go to next tab
                        self.next_script_tab();
                        true
                    }
                    "T" => {
                        // gT - go to previous tab
                        self.prev_script_tab();
                        true
                    }
                    "f" => {
                        // gf - go to file under cursor
                        self.go_to_file_under_cursor();
                        true
                    }
                    "I" => {
                        // gI - insert at column 0
                        self.insert_at_column_zero();
                        true
                    }
                    "i" => {
                        // gi - insert at last insert position
                        self.insert_at_last_position();
                        true
                    }
                    "a" => {
                        // ga - show character info under cursor
                        self.show_char_info();
                        true
                    }
                    "&" => {
                        // g& - repeat last substitution on entire buffer
                        self.repeat_substitute();
                        true
                    }
                    "J" => {
                        // gJ - join lines without space
                        self.join_lines_no_space();
                        true
                    }
                    "p" => {
                        // gp - paste and move cursor after pasted text
                        self.paste_and_move_after();
                        true
                    }
                    "P" => {
                        // gP - paste before and move cursor after pasted text
                        self.paste_before_and_move_after();
                        true
                    }
                    "e" => {
                        // ge - move to end of previous word
                        self.move_to_word_end_backward();
                        true
                    }
                    "x" => {
                        // gx - open URL under cursor in browser
                        self.open_url_under_cursor();
                        true
                    }
                    _ => false,
                };

                if handled {
                    self.last_key.clear();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
            }

            // Track last key for sequence detection, unless scroll command was handled
            if !scroll_handled {
                self.last_key = keys;
            }

            // Consume the event to prevent Godot's default handling
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
        }
    }
}
