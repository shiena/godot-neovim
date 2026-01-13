use crate::neovim::NeovimClient;
use crate::settings;
use godot::classes::text_edit::CaretType;
use godot::classes::{
    CodeEdit, Control, EditorInterface, EditorPlugin, IEditorPlugin, Input, InputEventKey, Label,
    ResourceSaver, TabBar,
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
            return;
        }

        // Handle pending character operator (f, F, t, T, r)
        if let Some(op) = self.pending_char_op {
            let keycode = key_event.get_keycode();

            // Cancel on Escape
            if keycode == Key::ESCAPE {
                self.pending_char_op = None;
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
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
                    return;
                }
            }
        }

        // Handle pending mark operation (m, ', `)
        if let Some(op) = self.pending_mark_op {
            let keycode = key_event.get_keycode();

            // Cancel on Escape
            if keycode == Key::ESCAPE {
                self.pending_mark_op = None;
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
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
                        return;
                    }
                }
            }
        }

        // Handle pending macro operation (q for record, @ for play)
        if let Some(op) = self.pending_macro_op {
            let keycode = key_event.get_keycode();

            // Cancel on Escape
            if keycode == Key::ESCAPE {
                self.pending_macro_op = None;
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
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
                    return;
                }
            }
        }

        // Handle pending register selection (waiting for register char after ")
        if self.selected_register == Some('\0') {
            let keycode = key_event.get_keycode();

            // Cancel on Escape
            if keycode == Key::ESCAPE {
                self.selected_register = None;
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }

            // Get the character (must be a-z for named registers)
            let unicode = key_event.get_unicode();
            if unicode > 0 {
                if let Some(c) = char::from_u32(unicode) {
                    if c.is_ascii_lowercase() {
                        self.selected_register = Some(c);
                        crate::verbose_print!("[godot-neovim] \"{}: Register selected", c);
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    }
                }
            }
        }

        // In insert mode, behavior depends on input mode setting
        if self.is_insert_mode() {
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
            let nvim_key = self.key_event_to_nvim_notation(&key_event);
            if !nvim_key.is_empty() {
                self.send_keys_insert_mode(&nvim_key);
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
            }
            return;
        }

        // Handle Ctrl+B: visual block in visual mode, page up in normal mode
        let keycode = key_event.get_keycode();
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

        // Handle '/' for search - open Godot's find dialog
        if keycode == Key::SLASH && !key_event.is_ctrl_pressed() && !key_event.is_shift_pressed() {
            self.open_find_dialog();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ':' for command-line mode (use unicode for cross-keyboard support)
        let unicode_char = char::from_u32(key_event.get_unicode());
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

        // Handle 'r' for replace char
        if keycode == Key::R && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.pending_char_op = Some('r');
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
                // Register is selected, check for yy
                if keycode == Key::Y
                    && !key_event.is_shift_pressed()
                    && !key_event.is_ctrl_pressed()
                {
                    if self.last_key == "y" {
                        // yy - yank current line to register
                        self.yank_line_to_register(reg);
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
                        // dd - delete line and store in register
                        self.delete_line_to_register(reg);
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
                }
            }
        }

        // Forward key to Neovim (normal/visual/etc modes)
        if let Some(keys) = self.key_event_to_nvim_string(&key_event) {
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

            // Track last key for sequence detection, unless scroll command was handled
            if !scroll_handled {
                self.last_key = keys.clone();
            }

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

    fn sync_buffer_to_neovim(&mut self) {
        let Some(ref editor) = self.current_editor else {
            crate::verbose_print!("[godot-neovim] sync_buffer_to_neovim: No current editor");
            return;
        };

        let Some(ref neovim) = self.neovim else {
            crate::verbose_print!("[godot-neovim] sync_buffer_to_neovim: No neovim");
            return;
        };

        let Ok(client) = neovim.lock() else {
            crate::verbose_print!("[godot-neovim] sync_buffer_to_neovim: Failed to lock");
            return;
        };

        // Get text from Godot editor
        let text = editor.get_text().to_string();
        let lines: Vec<String> = text.lines().map(String::from).collect();

        crate::verbose_print!("[godot-neovim] Syncing {} lines to Neovim", lines.len());
        if !lines.is_empty() {
            crate::verbose_print!(
                "[godot-neovim] First line: '{}'",
                lines[0].chars().take(50).collect::<String>()
            );
        }

        // Set buffer content in Neovim
        if let Err(e) = client.set_buffer_lines(0, -1, lines) {
            godot_error!("[godot-neovim] Failed to sync buffer: {}", e);
        } else {
            // Clear Neovim's modified flag since we synced from Godot
            client.set_buffer_not_modified();
            crate::verbose_print!("[godot-neovim] Buffer synced to Neovim successfully");
        }
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
                    let c = char::from_u32(unicode)?;
                    // Apply shift modifier for letters (get_unicode may not include shift)
                    if shift && c.is_ascii_lowercase() {
                        c.to_ascii_uppercase().to_string()
                    } else {
                        c.to_string()
                    }
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

    /// Check if mode is a visual mode (v, V, or Ctrl+V)
    fn is_visual_mode(mode: &str) -> bool {
        matches!(mode, "v" | "V" | "\x16" | "^V" | "CTRL-V")
    }

    /// Mark input as handled on the CodeEdit's viewport
    /// This prevents CodeEdit from processing the input event
    fn consume_input_on_editor(&self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        // Use CodeEdit's viewport, not the plugin's viewport
        if let Some(mut viewport) = editor.get_viewport() {
            viewport.set_input_as_handled();
        }
    }

    /// Convert Godot key event to Neovim notation for insert mode (strict mode)
    fn key_event_to_nvim_notation(&self, key_event: &Gd<InputEventKey>) -> String {
        let keycode = key_event.get_keycode();
        let unicode = key_event.get_unicode();
        let ctrl = key_event.is_ctrl_pressed();
        let alt = key_event.is_alt_pressed();
        let shift = key_event.is_shift_pressed();

        // Handle special keys
        let special = match keycode {
            Key::BACKSPACE => Some("<BS>"),
            Key::TAB => Some("<Tab>"),
            Key::ENTER => Some("<CR>"),
            Key::DELETE => Some("<Del>"),
            Key::HOME => Some("<Home>"),
            Key::END => Some("<End>"),
            Key::PAGEUP => Some("<PageUp>"),
            Key::PAGEDOWN => Some("<PageDown>"),
            Key::UP => Some("<Up>"),
            Key::DOWN => Some("<Down>"),
            Key::LEFT => Some("<Left>"),
            Key::RIGHT => Some("<Right>"),
            Key::F1 => Some("<F1>"),
            Key::F2 => Some("<F2>"),
            Key::F3 => Some("<F3>"),
            Key::F4 => Some("<F4>"),
            Key::F5 => Some("<F5>"),
            Key::F6 => Some("<F6>"),
            Key::F7 => Some("<F7>"),
            Key::F8 => Some("<F8>"),
            Key::F9 => Some("<F9>"),
            Key::F10 => Some("<F10>"),
            Key::F11 => Some("<F11>"),
            Key::F12 => Some("<F12>"),
            _ => None,
        };

        if let Some(key_str) = special {
            // Add modifiers to special keys
            if ctrl || alt || shift {
                let mut modifiers = String::new();
                if ctrl {
                    modifiers.push_str("C-");
                }
                if alt {
                    modifiers.push_str("A-");
                }
                if shift {
                    modifiers.push_str("S-");
                }
                // Convert <Key> to <C-A-S-Key>
                let inner = &key_str[1..key_str.len() - 1];
                return format!("<{}{}>", modifiers, inner);
            }
            return key_str.to_string();
        }

        // Handle printable characters
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                // Ctrl+letter combinations
                if ctrl && !alt {
                    let base_char = c.to_ascii_lowercase();
                    if base_char.is_ascii_alphabetic() {
                        return format!("<C-{}>", base_char);
                    }
                }
                // Alt combinations
                if alt && !ctrl {
                    return format!("<A-{}>", c);
                }
                // Ctrl+Alt combinations
                if ctrl && alt {
                    return format!("<C-A-{}>", c);
                }
                // Regular character (shift is already applied in unicode)
                return c.to_string();
            }
        }

        String::new()
    }

    /// Send keys to Neovim in insert mode and sync buffer (strict mode)
    fn send_keys_insert_mode(&mut self, keys: &str) {
        crate::verbose_print!("[godot-neovim] send_keys_insert_mode: {}", keys);

        let Some(ref neovim) = self.neovim else {
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

        // Get buffer and cursor from Neovim
        let lines = client.get_buffer_lines(0, -1).unwrap_or_default();
        let cursor = client.get_cursor().ok();

        // Release lock before syncing
        drop(client);

        // Sync buffer from Neovim to Godot
        self.sync_buffer_from_neovim(lines, cursor);
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
            crate::verbose_print!(
                "[godot-neovim] Got update: mode={}, cursor=({}, {})",
                mode,
                cursor.0,
                cursor.1
            );

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

            // Handle visual mode selection
            let was_visual = Self::is_visual_mode(&old_mode);
            let is_visual = Self::is_visual_mode(&mode);

            if is_visual {
                // Update visual selection display
                if mode == "V" {
                    self.update_visual_line_selection();
                } else {
                    self.update_visual_selection();
                }
            } else if was_visual {
                // Exiting visual mode - clear selection
                self.clear_visual_selection();
            }
        }
    }

    /// Send Escape to Neovim and force mode to normal
    fn send_escape(&mut self) {
        crate::verbose_print!("[godot-neovim] send_escape");

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

        // Sync buffer and cursor from Godot to Neovim (user was typing in Godot)
        self.sync_buffer_to_neovim();
        self.sync_cursor_to_neovim();

        // Force mode to normal (ESC always returns to normal mode)
        self.current_mode = "n".to_string();

        // Clear any visual selection
        self.clear_visual_selection();

        // Display cursor position (convert 0-indexed to 1-indexed for display)
        let display_cursor = (self.current_cursor.0 + 1, self.current_cursor.1);
        self.update_mode_display_with_cursor("n", Some(display_cursor));

        crate::verbose_print!("[godot-neovim] Escaped to normal mode, buffer synced");
    }

    /// Sync cursor position from Godot editor to Neovim
    fn sync_cursor_to_neovim(&mut self) {
        let Some(ref editor) = self.current_editor else {
            crate::verbose_print!("[godot-neovim] sync_cursor_to_neovim: No current editor");
            return;
        };

        let Some(ref neovim) = self.neovim else {
            crate::verbose_print!("[godot-neovim] sync_cursor_to_neovim: No neovim");
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            crate::verbose_print!("[godot-neovim] sync_cursor_to_neovim: Failed to lock");
            return;
        };

        // Get cursor from Godot (0-indexed)
        let line = editor.get_caret_line();
        let col = editor.get_caret_column();

        // Neovim uses 1-indexed lines, 0-indexed columns
        let nvim_line = (line + 1) as i64;
        let nvim_col = col as i64;

        crate::verbose_print!(
            "[godot-neovim] Syncing cursor to Neovim: line={}, col={}",
            nvim_line,
            nvim_col
        );

        if let Err(e) = client.set_cursor(nvim_line, nvim_col) {
            godot_error!("[godot-neovim] Failed to sync cursor: {}", e);
        }

        // Update cached cursor position
        drop(client);
        self.current_cursor = (line as i64, col as i64);
    }

    /// Send keys to Neovim and update state
    /// Returns true if command completed, false if operator pending
    fn send_keys(&mut self, keys: &str) -> bool {
        crate::verbose_print!("[godot-neovim] send_keys: {}", keys);

        let Some(ref neovim) = self.neovim else {
            crate::verbose_print!("[godot-neovim] No neovim");
            return false;
        };

        let Ok(client) = neovim.try_lock() else {
            godot_warn!("[godot-neovim] Mutex busy, dropping key: {}", keys);
            return false;
        };

        // Send input to Neovim
        if let Err(e) = client.input(keys) {
            godot_error!("[godot-neovim] Failed to send keys: {}", e);
            return false;
        }
        crate::verbose_print!("[godot-neovim] Key sent successfully");

        // Query mode - if blocking (operator-pending or insert mode), handle specially
        let (mode, blocking) = client.get_mode();

        // Track old mode for visual mode transitions before updating
        let old_mode = self.current_mode.clone();

        // Always update current_mode so is_insert_mode() works correctly
        self.current_mode = mode.clone();

        if blocking {
            // Insert mode is "blocking" but we should update mode display
            if mode == "i" {
                crate::verbose_print!("[godot-neovim] Entered insert mode");
                drop(client);
                self.update_mode_display_with_cursor(&mode, None);
                return true;
            }
            // True operator-pending (like waiting for motion after 'd')
            crate::verbose_print!("[godot-neovim] Operator pending, skipping sync");
            return false;
        }

        // Query cursor
        let cursor = client.get_cursor().unwrap_or((1, 0));

        // Get buffer content from Neovim
        let buffer_lines = client.get_buffer_lines(0, -1).ok();

        crate::verbose_print!(
            "[godot-neovim] After key: mode={}, cursor=({}, {}), lines={:?}",
            mode,
            cursor.0,
            cursor.1,
            buffer_lines.as_ref().map(|l| l.len())
        );

        // Release lock before updating UI
        drop(client);

        // Update cursor state
        self.current_cursor = (cursor.0 - 1, cursor.1); // Convert to 0-indexed

        // Sync buffer from Neovim to Godot
        if let Some(lines) = buffer_lines {
            self.sync_buffer_from_neovim(lines, Some(cursor));
        }

        // Update mode display
        self.update_mode_display_with_cursor(&mode, Some(cursor));

        // Handle visual mode selection
        let was_visual = Self::is_visual_mode(&old_mode);
        let is_visual = Self::is_visual_mode(&mode);

        if is_visual {
            // Update visual selection display
            if mode == "V" {
                self.update_visual_line_selection();
            } else {
                self.update_visual_selection();
            }
        } else if was_visual {
            // Exiting visual mode - clear selection
            self.clear_visual_selection();
        }

        true
    }

    /// Update cursor position from Godot editor and refresh display
    fn update_cursor_from_editor(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        // Get cursor from Godot (0-indexed)
        let line = editor.get_caret_line() as i64;
        let col = editor.get_caret_column() as i64;

        self.current_cursor = (line, col);

        // Update display (1-indexed for display)
        let display_cursor = (line + 1, col);
        self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(display_cursor));
    }

    /// Sync cursor from grid position (0-indexed)
    fn sync_cursor_from_grid(&mut self, cursor: (i64, i64)) {
        let Some(ref mut editor) = self.current_editor else {
            crate::verbose_print!("[godot-neovim] sync_cursor_from_grid: No current editor");
            return;
        };

        let (row, col) = cursor;
        crate::verbose_print!(
            "[godot-neovim] sync_cursor_from_grid: Setting cursor to row={}, col={}",
            row,
            col
        );
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
        // Don't update if we're in command-line mode (our own command buffer)
        if self.command_mode {
            return;
        }

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
                "n" | "normal" => Color::from_rgb(0.0, 1.0, 0.5), // Green
                "i" | "insert" => Color::from_rgb(0.3, 0.6, 1.0), // Blue
                "v" | "visual" | "V" | "\x16" => Color::from_rgb(1.0, 0.5, 0.0), // Orange
                "c" | "command" => Color::from_rgb(1.0, 1.0, 0.0), // Yellow
                "R" | "replace" => Color::from_rgb(1.0, 0.3, 0.3), // Red
                _ => Color::from_rgb(0.8, 0.8, 0.8),              // Gray
            };
            label.add_theme_color_override("font_color", color);
        }

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

    fn sync_buffer_from_neovim(&mut self, lines: Vec<String>, cursor: Option<(i64, i64)>) {
        let Some(ref mut editor) = self.current_editor else {
            crate::verbose_print!("[godot-neovim] No current editor for buffer sync");
            return;
        };

        crate::verbose_print!(
            "[godot-neovim] Syncing buffer from Neovim: {} lines",
            lines.len()
        );

        // Update Godot editor only if content changed
        // Normalize line endings and trailing newlines for comparison
        // (Windows uses \r\n, Neovim uses \n; Godot may add trailing newline)
        let new_text = lines.join("\n");
        let current_text = editor
            .get_text()
            .to_string()
            .replace("\r\n", "\n")
            .trim_end_matches('\n')
            .to_string();
        let new_text_normalized = new_text.trim_end_matches('\n');

        if new_text_normalized != current_text {
            crate::verbose_print!("[godot-neovim] Content changed, updating editor");
            editor.set_text(&new_text);
        } else {
            crate::verbose_print!("[godot-neovim] Content unchanged, skipping set_text");
        }

        // Update cursor position
        if let Some((line, col)) = cursor {
            crate::verbose_print!(
                "[godot-neovim] Setting cursor to line {}, col {}",
                line,
                col
            );
            editor.set_caret_line((line - 1) as i32); // Neovim is 1-indexed
            editor.set_caret_column(col as i32);
        }
    }

    /// Handle scroll commands (zz, zt, zb) and g-commands (gd) after sending to Neovim
    /// Returns true if a command was handled
    fn handle_scroll_command(&mut self, keys: &str) -> bool {
        // Check for z-prefixed scroll commands
        if self.last_key == "z" {
            let handled = match keys {
                "z" => {
                    // zz - center viewport on cursor
                    if let Some(ref mut editor) = self.current_editor {
                        editor.center_viewport_to_caret();
                        crate::verbose_print!("[godot-neovim] zz: centered viewport");
                    }
                    true
                }
                "t" => {
                    // zt - cursor line at top
                    if let Some(ref mut editor) = self.current_editor {
                        let line = editor.get_caret_line();
                        editor.set_line_as_first_visible(line);
                        crate::verbose_print!("[godot-neovim] zt: line {} at top", line);
                    }
                    true
                }
                "b" => {
                    // zb - cursor line at bottom
                    if let Some(ref mut editor) = self.current_editor {
                        let line = editor.get_caret_line();
                        // Calculate line to set as first visible to put cursor at bottom
                        let visible_lines = editor.get_visible_line_count();
                        let first_line = (line - visible_lines + 1).max(0);
                        editor.set_line_as_first_visible(first_line);
                        crate::verbose_print!(
                            "[godot-neovim] zb: line {} at bottom (first={})",
                            line,
                            first_line
                        );
                    }
                    true
                }
                _ => false,
            };

            // Clear last_key after handling to prevent re-trigger
            if handled {
                self.last_key.clear();
                return true;
            }
        }

        // Check for g-prefixed commands
        if self.last_key == "g" {
            let handled = match keys {
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
                _ => false,
            };

            if handled {
                self.last_key.clear();
                return true;
            }
        }

        // Check for Z-prefixed commands (uppercase Z)
        if self.last_key == "Z" {
            let handled = match keys {
                "Z" => {
                    // ZZ - save and close
                    self.cmd_save();
                    self.cmd_close();
                    true
                }
                "Q" => {
                    // ZQ - close without saving (discard changes)
                    self.cmd_close_discard();
                    true
                }
                _ => false,
            };

            if handled {
                self.last_key.clear();
                return true;
            }
        }

        false
    }

    /// Move cursor to top of visible area (H command)
    fn move_cursor_to_visible_top(&mut self) {
        let target_line = {
            let Some(ref mut editor) = self.current_editor else {
                return;
            };
            let first_visible = editor.get_first_visible_line();
            editor.set_caret_line(first_visible);
            editor.set_caret_column(0);
            first_visible
        };

        crate::verbose_print!("[godot-neovim] H: moved to line {}", target_line);

        // Sync to Neovim (non-blocking, errors are logged but ignored)
        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
    }

    /// Move cursor to middle of visible area (M command)
    fn move_cursor_to_visible_middle(&mut self) {
        let target_line = {
            let Some(ref mut editor) = self.current_editor else {
                return;
            };
            let first_visible = editor.get_first_visible_line();
            let visible_lines = editor.get_visible_line_count();
            let middle_line = first_visible + visible_lines / 2;
            let line_count = editor.get_line_count();
            let target = middle_line.min(line_count - 1);
            editor.set_caret_line(target);
            editor.set_caret_column(0);
            target
        };

        crate::verbose_print!("[godot-neovim] M: moved to line {}", target_line);

        // Sync to Neovim (non-blocking, errors are logged but ignored)
        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
    }

    /// Move cursor to bottom of visible area (L command)
    fn move_cursor_to_visible_bottom(&mut self) {
        let target_line = {
            let Some(ref mut editor) = self.current_editor else {
                return;
            };
            let last_visible = editor.get_last_full_visible_line();
            let line_count = editor.get_line_count();
            let target = last_visible.min(line_count - 1);
            editor.set_caret_line(target);
            editor.set_caret_column(0);
            target
        };

        crate::verbose_print!("[godot-neovim] L: moved to line {}", target_line);

        // Sync to Neovim (non-blocking, errors are logged but ignored)
        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
    }

    /// Update visual selection in Godot editor
    fn update_visual_selection(&mut self) {
        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Get visual selection from Neovim
        let Some(((start_line, start_col), (end_line, end_col))) = client.get_visual_selection()
        else {
            return;
        };

        // Release lock before updating UI
        drop(client);

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Normalize selection direction (start should be before end)
        let (from_line, from_col, to_line, to_col) =
            if start_line < end_line || (start_line == end_line && start_col <= end_col) {
                (start_line, start_col, end_line, end_col + 1) // +1 to include cursor char
            } else {
                (end_line, end_col, start_line, start_col + 1)
            };

        crate::verbose_print!(
            "[godot-neovim] Visual selection: ({}, {}) -> ({}, {})",
            from_line,
            from_col,
            to_line,
            to_col
        );

        // Update Godot selection
        editor.select(
            from_line as i32,
            from_col as i32,
            to_line as i32,
            to_col as i32,
        );
    }

    /// Update visual line selection in Godot editor (V mode - selects entire lines)
    fn update_visual_line_selection(&mut self) {
        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Get visual selection from Neovim
        let Some(((start_line, _), (end_line, _))) = client.get_visual_selection() else {
            return;
        };

        // Release lock before updating UI
        drop(client);

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Normalize line order
        let (from_line, to_line) = if start_line <= end_line {
            (start_line, end_line)
        } else {
            (end_line, start_line)
        };

        // Get the length of the last line to select entire lines
        let to_line_text = editor.get_line(to_line as i32);
        let to_col = to_line_text.len() as i32;

        crate::verbose_print!(
            "[godot-neovim] Visual line selection: line {} -> {} (col 0 -> {})",
            from_line,
            to_line,
            to_col
        );

        // Select from start of first line to end of last line
        editor.select(from_line as i32, 0, to_line as i32, to_col);
    }

    /// Clear visual selection in Godot editor
    fn clear_visual_selection(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        editor.deselect();
        crate::verbose_print!("[godot-neovim] Visual selection cleared");
    }

    /// Scroll viewport up by one line (Ctrl+Y) - cursor line stays the same
    fn scroll_viewport_up(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let first_visible = editor.get_first_visible_line();
        if first_visible > 0 {
            editor.set_line_as_first_visible(first_visible - 1);
            crate::verbose_print!(
                "[godot-neovim] Ctrl+Y: scrolled up, first_visible={}",
                first_visible - 1
            );
        }
    }

    /// Scroll viewport down by one line (Ctrl+E) - cursor line stays the same
    fn scroll_viewport_down(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let first_visible = editor.get_first_visible_line();
        let line_count = editor.get_line_count();

        if first_visible < line_count - 1 {
            editor.set_line_as_first_visible(first_visible + 1);
            crate::verbose_print!(
                "[godot-neovim] Ctrl+E: scrolled down, first_visible={}",
                first_visible + 1
            );
        }
    }

    /// Open Godot's find dialog by simulating Ctrl+F
    fn open_find_dialog(&self) {
        // Create a Ctrl+F key event
        let mut key_event = InputEventKey::new_gd();
        key_event.set_keycode(Key::F);
        key_event.set_ctrl_pressed(true);
        key_event.set_pressed(true);

        // Parse the input event to trigger Godot's find dialog
        Input::singleton().parse_input_event(&key_event);

        crate::verbose_print!("[godot-neovim] Opened find dialog (simulated Ctrl+F)");
    }

    /// Open command line for input
    fn open_command_line(&mut self) {
        self.command_buffer = ":".to_string();
        self.command_mode = true;
        self.command_history_index = None;
        self.command_history_temp.clear();
        self.update_command_display();
        crate::verbose_print!("[godot-neovim] Command line opened");
    }

    /// Browse command history (older - Up key)
    fn command_history_up(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        match self.command_history_index {
            None => {
                // Start browsing history - save current input
                self.command_history_temp = self
                    .command_buffer
                    .strip_prefix(':')
                    .unwrap_or("")
                    .to_string();
                self.command_history_index = Some(self.command_history.len() - 1);
            }
            Some(idx) => {
                // Move to older entry
                if idx > 0 {
                    self.command_history_index = Some(idx - 1);
                }
            }
        }

        // Update command buffer with history entry
        if let Some(idx) = self.command_history_index {
            if let Some(cmd) = self.command_history.get(idx) {
                self.command_buffer = format!(":{}", cmd);
                self.update_command_display();
            }
        }
    }

    /// Browse command history (newer - Down key)
    fn command_history_down(&mut self) {
        let Some(idx) = self.command_history_index else {
            return;
        };

        if idx < self.command_history.len() - 1 {
            // Move to newer entry
            self.command_history_index = Some(idx + 1);
            if let Some(cmd) = self.command_history.get(idx + 1) {
                self.command_buffer = format!(":{}", cmd);
                self.update_command_display();
            }
        } else {
            // Return to current input
            self.command_history_index = None;
            self.command_buffer = format!(":{}", self.command_history_temp);
            self.update_command_display();
        }
    }

    /// Update command display in mode label
    fn update_command_display(&mut self) {
        if let Some(ref mut label) = self.mode_label {
            label.set_text(&format!(" {} ", self.command_buffer));
            // Yellow color for command mode
            label.add_theme_color_override("font_color", Color::from_rgb(1.0, 1.0, 0.0));
        }
    }

    /// Close command line
    fn close_command_line(&mut self) {
        self.command_buffer.clear();
        self.command_mode = false;
        self.command_history_index = None;
        self.command_history_temp.clear();

        // Restore normal mode display
        let cursor = (self.current_cursor.0 + 1, self.current_cursor.1);
        self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(cursor));

        crate::verbose_print!("[godot-neovim] Command line closed");
    }

    /// Execute the command from command line
    fn execute_command(&mut self) {
        let command = self.command_buffer.clone();

        // Remove the leading ':'
        let cmd = command.strip_prefix(':').unwrap_or(&command).trim();

        // Save to command history (avoid duplicates of last command)
        if !cmd.is_empty() {
            let cmd_string = cmd.to_string();
            if self.command_history.last() != Some(&cmd_string) {
                self.command_history.push(cmd_string);
            }
        }
        // Reset history browsing
        self.command_history_index = None;
        self.command_history_temp.clear();

        crate::verbose_print!("[godot-neovim] Executing command: {}", cmd);

        match cmd {
            "w" => self.cmd_save(),
            "q" => self.cmd_close(),
            "qa" | "qall" => self.cmd_close_all(),
            "wq" | "x" => {
                self.cmd_save();
                self.cmd_close();
            }
            _ => {
                // Check for :{number} - jump to line
                if let Ok(line_num) = cmd.parse::<i32>() {
                    self.cmd_goto_line(line_num);
                }
                // Check for :marks - show marks
                else if cmd == "marks" {
                    self.cmd_show_marks();
                }
                // Check for :registers or :reg - show registers
                else if cmd == "registers" || cmd == "reg" {
                    self.cmd_show_registers();
                }
                // Check for :e[dit] {file} command
                else if cmd.starts_with("e ") || cmd.starts_with("edit ") {
                    let file_path = if cmd.starts_with("edit ") {
                        cmd.strip_prefix("edit ").unwrap_or("").trim()
                    } else {
                        cmd.strip_prefix("e ").unwrap_or("").trim()
                    };
                    self.cmd_edit(file_path);
                }
                // Check for substitution command :%s/old/new/g
                else if cmd.starts_with("%s/") || cmd.starts_with("s/") {
                    self.cmd_substitute(cmd);
                } else {
                    godot_warn!("[godot-neovim] Unknown command: {}", cmd);
                }
            }
        }

        self.close_command_line();
    }

    /// :{number} - Jump to specific line number
    fn cmd_goto_line(&mut self, line_num: i32) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_count = editor.get_line_count();
        // Convert 1-indexed to 0-indexed, clamp to valid range
        let target_line = (line_num - 1).clamp(0, line_count - 1);

        editor.set_caret_line(target_line);

        // Move to first non-blank character (Vim behavior)
        let line_text = editor.get_line(target_line).to_string();
        let first_non_blank = line_text
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);
        editor.set_caret_column(first_non_blank as i32);

        self.sync_cursor_to_neovim();
        crate::verbose_print!("[godot-neovim] :{}: Jumped to line {}", line_num, target_line + 1);
    }

    /// :marks - Show all marks
    fn cmd_show_marks(&self) {
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
    fn cmd_show_registers(&self) {
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

    /// :e[dit] {file} - Open a file in the script editor
    fn cmd_edit(&self, file_path: &str) {
        if file_path.is_empty() {
            godot_warn!("[godot-neovim] :e requires a file path");
            return;
        }

        let mut editor = EditorInterface::singleton();

        // Try to load the resource
        let path = if file_path.starts_with("res://") {
            file_path.to_string()
        } else {
            // Assume relative to res://
            format!("res://{}", file_path)
        };

        // Load the resource
        let resource = godot::classes::ResourceLoader::singleton().load(&path);
        if let Some(res) = resource {
            // Try to cast to Script
            if let Ok(script) = res.try_cast::<godot::classes::Script>() {
                // Use edit_script to open the script
                editor.edit_script(&script);
                crate::verbose_print!("[godot-neovim] :e - Opened script: {}", path);
            } else {
                godot_warn!("[godot-neovim] :e - Not a script file: {}", path);
            }
        } else {
            godot_warn!("[godot-neovim] :e - File not found: {}", path);
        }
    }

    /// :w - Save the current file by simulating Ctrl+S
    fn cmd_save(&self) {
        // Simulate Ctrl+S to save (avoids re-entrant borrow issues)
        let mut key_press = InputEventKey::new_gd();
        key_press.set_keycode(Key::S);
        key_press.set_ctrl_pressed(true);
        key_press.set_pressed(true);
        Input::singleton().parse_input_event(&key_press);

        // Release the key (must be a new instance to avoid same-frame warning)
        let mut key_release = InputEventKey::new_gd();
        key_release.set_keycode(Key::S);
        key_release.set_ctrl_pressed(true);
        key_release.set_pressed(false);
        Input::singleton().parse_input_event(&key_release);

        crate::verbose_print!("[godot-neovim] :w - Save triggered (Ctrl+S)");
    }

    /// ZZ - Save and close (using ResourceSaver for synchronous save)
    fn cmd_save_and_close(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(mut script_editor) = editor.get_script_editor() {
            if let Some(current_script) = script_editor.get_current_script() {
                let path = current_script.get_path();
                if !path.is_empty() {
                    // Save the script using ResourceSaver (synchronous)
                    let result = ResourceSaver::singleton()
                        .save_ex(&current_script)
                        .path(&path)
                        .done();
                    if result == godot::global::Error::OK {
                        crate::verbose_print!("[godot-neovim] ZZ - Saved: {}", path);
                    } else {
                        godot_warn!("[godot-neovim] ZZ - Failed to save: {}", path);
                    }
                }
            }
        }

        // Now close the tab
        self.cmd_close();
    }

    /// :q - Close the current script tab by simulating Ctrl+W
    fn cmd_close(&mut self) {
        // Clear current editor reference before closing to avoid accessing freed instance
        self.current_editor = None;

        // Simulate Ctrl+W key press
        let mut key_press = InputEventKey::new_gd();
        key_press.set_keycode(Key::W);
        key_press.set_ctrl_pressed(true);
        key_press.set_pressed(true);
        Input::singleton().parse_input_event(&key_press);

        // Release the key
        let mut key_release = InputEventKey::new_gd();
        key_release.set_keycode(Key::W);
        key_release.set_ctrl_pressed(true);
        key_release.set_pressed(false);
        Input::singleton().parse_input_event(&key_release);

        crate::verbose_print!("[godot-neovim] :q - Close triggered (Ctrl+W)");
    }

    /// ZQ - Close without saving (discard changes)
    fn cmd_close_discard(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(mut script_editor) = editor.get_script_editor() {
            // Reload the script from disk and sync the CodeEdit
            if let Some(mut current_script) = script_editor.get_current_script() {
                let path = current_script.get_path();
                if !path.is_empty() {
                    // Reload the script from disk
                    let _ = current_script.reload();

                    // Also update the CodeEdit to match the reloaded script
                    if let Some(mut code_edit) = self.current_editor.clone() {
                        let source = current_script.get_source_code();
                        code_edit.set_text(&source);
                        // Mark as saved to clear the unsaved state
                        code_edit.tag_saved_version();
                        crate::verbose_print!(
                            "[godot-neovim] ZQ - Synced CodeEdit and tagged as saved: {}",
                            path
                        );
                    }
                }
            }
        }

        // Now close the tab (should not prompt since changes are discarded)
        self.current_editor = None;

        // Simulate Ctrl+W key press
        let mut key_press = InputEventKey::new_gd();
        key_press.set_keycode(Key::W);
        key_press.set_ctrl_pressed(true);
        key_press.set_pressed(true);
        Input::singleton().parse_input_event(&key_press);

        let mut key_release = InputEventKey::new_gd();
        key_release.set_keycode(Key::W);
        key_release.set_ctrl_pressed(true);
        key_release.set_pressed(false);
        Input::singleton().parse_input_event(&key_release);

        crate::verbose_print!("[godot-neovim] ZQ - Close triggered (discard changes)");
    }

    /// Find TabBar in the ScriptEditor hierarchy
    fn find_tab_bar(&self, node: Gd<Control>) -> Option<Gd<TabBar>> {
        // Check if this node is a TabBar
        if let Ok(tab_bar) = node.clone().try_cast::<TabBar>() {
            // Make sure it has tabs (script tabs, not other TabBars)
            if tab_bar.get_tab_count() > 0 {
                crate::verbose_print!(
                    "[godot-neovim] Found TabBar with {} tabs",
                    tab_bar.get_tab_count()
                );
                return Some(tab_bar);
            }
        }

        // Search children
        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Ok(control) = child.try_cast::<Control>() {
                    if let Some(tab_bar) = self.find_tab_bar(control) {
                        return Some(tab_bar);
                    }
                }
            }
        }

        None
    }

    /// Debug: Print node hierarchy to find TabBar
    #[allow(dead_code)]
    fn debug_print_hierarchy(&self, node: Gd<Control>, depth: i32) {
        let indent = "  ".repeat(depth as usize);
        let class_name = node.get_class();
        let node_name = node.get_name();
        crate::verbose_print!("{}[{}] {}", indent, class_name, node_name);

        let count = node.get_child_count();
        for i in 0..count {
            if let Some(child) = node.get_child(i) {
                if let Ok(control) = child.try_cast::<Control>() {
                    if depth < 5 {
                        // Limit depth to avoid too much output
                        self.debug_print_hierarchy(control, depth + 1);
                    }
                }
            }
        }
    }

    /// gt - Go to next script tab by simulating Ctrl+Tab
    fn next_script_tab(&self) {
        let mut key_event = InputEventKey::new_gd();
        key_event.set_keycode(Key::TAB);
        key_event.set_ctrl_pressed(true);
        key_event.set_pressed(true);
        Input::singleton().parse_input_event(&key_event);
        crate::verbose_print!("[godot-neovim] gt - Next tab (Ctrl+Tab)");
    }

    /// gT - Go to previous script tab by simulating Ctrl+Shift+Tab
    fn prev_script_tab(&self) {
        let mut key_event = InputEventKey::new_gd();
        key_event.set_keycode(Key::TAB);
        key_event.set_ctrl_pressed(true);
        key_event.set_shift_pressed(true);
        key_event.set_pressed(true);
        Input::singleton().parse_input_event(&key_event);
        crate::verbose_print!("[godot-neovim] gT - Previous tab (Ctrl+Shift+Tab)");
    }

    /// :qa/:qall - Close all script tabs
    fn cmd_close_all(&mut self) {
        // Clear references before closing to avoid accessing freed instances
        self.current_editor = None;
        self.mode_label = None;

        // Get the number of open scripts
        let editor = EditorInterface::singleton();
        let script_count = if let Some(script_editor) = editor.get_script_editor() {
            script_editor.get_open_scripts().len()
        } else {
            0
        };

        // Close each script by simulating Ctrl+W multiple times
        for _ in 0..script_count {
            let mut key_event = InputEventKey::new_gd();
            key_event.set_keycode(Key::W);
            key_event.set_ctrl_pressed(true);
            key_event.set_pressed(true);
            Input::singleton().parse_input_event(&key_event);
        }

        crate::verbose_print!(
            "[godot-neovim] :qa - Close all triggered ({} scripts)",
            script_count
        );
    }

    /// :s/old/new/g or :%s/old/new/g - Substitute
    fn cmd_substitute(&mut self, cmd: &str) {
        // Parse the substitute command
        // Format: [%]s/pattern/replacement/[g]
        let cmd = cmd.strip_prefix('%').unwrap_or(cmd);
        let cmd = cmd.strip_prefix("s/").unwrap_or(cmd);

        let parts: Vec<&str> = cmd.split('/').collect();
        if parts.len() < 2 {
            godot_warn!("[godot-neovim] Invalid substitute command");
            return;
        }

        let pattern = parts[0];
        let replacement = parts[1];
        let _flags = parts.get(2).unwrap_or(&"");

        crate::verbose_print!(
            "[godot-neovim] Substitute: '{}' -> '{}'",
            pattern,
            replacement
        );

        // Get current editor and perform replacement
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Get all text, replace, and set back
        let text = editor.get_text().to_string();
        let new_text = text.replace(pattern, replacement);

        if text != new_text {
            // Save cursor position
            let line = editor.get_caret_line();
            let col = editor.get_caret_column();

            editor.set_text(&new_text);

            // Restore cursor position (clamped to valid range)
            let max_line = editor.get_line_count() - 1;
            editor.set_caret_line(line.min(max_line));
            editor.set_caret_column(col);

            // Sync to Neovim
            self.sync_buffer_to_neovim();

            crate::verbose_print!("[godot-neovim] Substitution complete");
        } else {
            crate::verbose_print!("[godot-neovim] No matches found for '{}'", pattern);
        }
    }

    /// Get the word under cursor from the Godot editor
    /// Returns None if no word is found at cursor position
    fn get_word_under_cursor(&self) -> Option<String> {
        let editor = self.current_editor.as_ref()?;

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();

        if line_text.is_empty() || col_idx >= line_text.chars().count() {
            return None;
        }

        let chars: Vec<char> = line_text.chars().collect();

        // Check if cursor is on a word character
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

        if !is_word_char(chars[col_idx]) {
            return None;
        }

        // Find word start
        let mut start = col_idx;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }

        // Find word end
        let mut end = col_idx;
        while end < chars.len() && is_word_char(chars[end]) {
            end += 1;
        }

        let word: String = chars[start..end].iter().collect();
        if word.is_empty() {
            None
        } else {
            Some(word)
        }
    }

    /// Search forward for word under cursor (*)
    fn search_word_forward(&mut self) {
        // Add to jump list before searching
        self.add_to_jump_list();

        let Some(word) = self.get_word_under_cursor() else {
            crate::verbose_print!("[godot-neovim] *: No word under cursor");
            return;
        };

        // Save for n/N repeat
        self.last_search_word = word.clone();
        self.last_search_forward = true;

        crate::verbose_print!("[godot-neovim] *: Searching forward for '{}'", word);

        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let current_col = editor.get_caret_column();
        let line_count = editor.get_line_count();

        // Search from current position forward
        for line_idx in current_line..line_count {
            let line_text = editor.get_line(line_idx).to_string();
            let search_start = if line_idx == current_line {
                // On current line, search after current word
                (current_col as usize) + 1
            } else {
                0
            };

            if let Some(found) = self.find_word_in_line(&line_text, &word, search_start, true) {
                self.move_cursor_to(line_idx, found as i32);
                return;
            }
        }

        // Wrap around to beginning of file
        for line_idx in 0..=current_line {
            let line_text = editor.get_line(line_idx).to_string();
            let search_end = if line_idx == current_line {
                current_col as usize
            } else {
                line_text.len()
            };

            if let Some(found) = self.find_word_in_line(&line_text, &word, 0, true) {
                if line_idx < current_line || found < search_end {
                    self.move_cursor_to(line_idx, found as i32);
                    return;
                }
            }
        }

        crate::verbose_print!("[godot-neovim] *: No more matches for '{}'", word);
    }

    /// Search backward for word under cursor (#)
    fn search_word_backward(&mut self) {
        // Add to jump list before searching
        self.add_to_jump_list();

        let Some(word) = self.get_word_under_cursor() else {
            crate::verbose_print!("[godot-neovim] #: No word under cursor");
            return;
        };

        // Save for n/N repeat
        self.last_search_word = word.clone();
        self.last_search_forward = false;

        crate::verbose_print!("[godot-neovim] #: Searching backward for '{}'", word);

        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let current_col = editor.get_caret_column() as usize;
        let line_count = editor.get_line_count();

        // Search from current position backward
        for line_idx in (0..=current_line).rev() {
            let line_text = editor.get_line(line_idx).to_string();

            if let Some(found) = self.find_word_in_line_backward(
                &line_text,
                &word,
                current_line,
                line_idx,
                current_col,
            ) {
                self.move_cursor_to(line_idx, found as i32);
                return;
            }
        }

        // Wrap around to end of file
        for line_idx in (current_line..line_count).rev() {
            let line_text = editor.get_line(line_idx).to_string();

            if let Some(found) = self.find_word_in_line(&line_text, &word, 0, false) {
                // Find last occurrence
                let mut last = found;
                let mut search_from = found + 1;
                while let Some(next) = self.find_word_in_line(&line_text, &word, search_from, true)
                {
                    if line_idx == current_line && next >= current_col {
                        break;
                    }
                    last = next;
                    search_from = next + 1;
                }
                self.move_cursor_to(line_idx, last as i32);
                return;
            }
        }

        crate::verbose_print!("[godot-neovim] #: No more matches for '{}'", word);
    }

    /// Find word boundary match in line starting from given position
    fn find_word_in_line(
        &self,
        line: &str,
        word: &str,
        start: usize,
        forward: bool,
    ) -> Option<usize> {
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        let chars: Vec<char> = line.chars().collect();
        let word_chars: Vec<char> = word.chars().collect();
        let word_len = word_chars.len();

        if word_len == 0 || chars.len() < word_len {
            return None;
        }

        let search_range: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(start..=chars.len().saturating_sub(word_len))
        } else {
            Box::new((0..=chars.len().saturating_sub(word_len)).rev())
        };

        for i in search_range {
            // Check if the substring matches
            let mut matches = true;
            for (j, wc) in word_chars.iter().enumerate() {
                if chars[i + j] != *wc {
                    matches = false;
                    break;
                }
            }

            if !matches {
                continue;
            }

            // Check word boundaries
            let before_ok = i == 0 || !is_word_char(chars[i - 1]);
            let after_ok = i + word_len >= chars.len() || !is_word_char(chars[i + word_len]);

            if before_ok && after_ok {
                return Some(i);
            }
        }

        None
    }

    /// Find word in line for backward search, handling current line specially
    fn find_word_in_line_backward(
        &self,
        line: &str,
        word: &str,
        current_line: i32,
        line_idx: i32,
        current_col: usize,
    ) -> Option<usize> {
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        let chars: Vec<char> = line.chars().collect();
        let word_chars: Vec<char> = word.chars().collect();
        let word_len = word_chars.len();

        if word_len == 0 || chars.len() < word_len {
            return None;
        }

        // Determine the end position for search
        let end_pos = if line_idx == current_line {
            current_col.saturating_sub(1)
        } else {
            chars.len().saturating_sub(word_len)
        };

        // Search backward from end_pos
        for i in (0..=end_pos.min(chars.len().saturating_sub(word_len))).rev() {
            // Check if the substring matches
            let mut matches = true;
            for (j, wc) in word_chars.iter().enumerate() {
                if chars[i + j] != *wc {
                    matches = false;
                    break;
                }
            }

            if !matches {
                continue;
            }

            // Check word boundaries
            let before_ok = i == 0 || !is_word_char(chars[i - 1]);
            let after_ok = i + word_len >= chars.len() || !is_word_char(chars[i + word_len]);

            if before_ok && after_ok {
                return Some(i);
            }
        }

        None
    }

    /// Move cursor to specified position and sync with Neovim
    fn move_cursor_to(&mut self, line: i32, col: i32) {
        if let Some(ref mut editor) = self.current_editor {
            editor.set_caret_line(line);
            editor.set_caret_column(col);
            crate::verbose_print!("[godot-neovim] Moved cursor to {}:{}", line + 1, col);
        }

        // Update cached cursor position
        self.current_cursor = (line as i64, col as i64);

        // Sync to Neovim
        self.sync_cursor_to_neovim();

        // Update display
        let display_cursor = (line as i64 + 1, col as i64);
        self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(display_cursor));
    }

    /// Repeat search in given direction (n/N commands)
    fn repeat_search(&mut self, same_direction: bool) {
        if self.last_search_word.is_empty() {
            crate::verbose_print!("[godot-neovim] n/N: No previous search");
            return;
        }

        let forward = if same_direction {
            self.last_search_forward
        } else {
            !self.last_search_forward
        };

        crate::verbose_print!(
            "[godot-neovim] {}: Repeating search for '{}' {}",
            if same_direction { "n" } else { "N" },
            self.last_search_word,
            if forward { "forward" } else { "backward" }
        );

        let word = self.last_search_word.clone();
        if forward {
            self.search_word_forward_internal(&word);
        } else {
            self.search_word_backward_internal(&word);
        }
    }

    /// Internal search forward (used by * and n)
    fn search_word_forward_internal(&mut self, word: &str) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let current_col = editor.get_caret_column();
        let line_count = editor.get_line_count();

        // Search from current position forward
        for line_idx in current_line..line_count {
            let line_text = editor.get_line(line_idx).to_string();
            let search_start = if line_idx == current_line {
                (current_col as usize) + 1
            } else {
                0
            };

            if let Some(found) = self.find_word_in_line(&line_text, word, search_start, true) {
                self.move_cursor_to(line_idx, found as i32);
                return;
            }
        }

        // Wrap around to beginning of file
        for line_idx in 0..=current_line {
            let line_text = editor.get_line(line_idx).to_string();
            let search_end = if line_idx == current_line {
                current_col as usize
            } else {
                line_text.len()
            };

            if let Some(found) = self.find_word_in_line(&line_text, word, 0, true) {
                if line_idx < current_line || found < search_end {
                    self.move_cursor_to(line_idx, found as i32);
                    return;
                }
            }
        }
    }

    /// Internal search backward (used by # and N)
    fn search_word_backward_internal(&mut self, word: &str) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let current_col = editor.get_caret_column() as usize;
        let line_count = editor.get_line_count();

        // Search from current position backward
        for line_idx in (0..=current_line).rev() {
            let line_text = editor.get_line(line_idx).to_string();

            if let Some(found) = self.find_word_in_line_backward(
                &line_text,
                word,
                current_line,
                line_idx,
                current_col,
            ) {
                self.move_cursor_to(line_idx, found as i32);
                return;
            }
        }

        // Wrap around to end of file
        for line_idx in (current_line..line_count).rev() {
            let line_text = editor.get_line(line_idx).to_string();

            if let Some(found) = self.find_word_in_line(&line_text, word, 0, false) {
                let mut last = found;
                let mut search_from = found + 1;
                while let Some(next) = self.find_word_in_line(&line_text, word, search_from, true) {
                    if line_idx == current_line && next >= current_col {
                        break;
                    }
                    last = next;
                    search_from = next + 1;
                }
                self.move_cursor_to(line_idx, last as i32);
                return;
            }
        }
    }

    /// Find character forward on current line (f/t commands)
    fn find_char_forward(&mut self, c: char, till: bool) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        // Search for character after cursor
        for (i, &ch) in chars.iter().enumerate().skip(col_idx + 1) {
            if ch == c {
                let target_col = if till { i - 1 } else { i };
                self.move_cursor_to(line_idx, target_col as i32);

                // Save for ; and ,
                self.last_find_char = Some(c);
                self.last_find_forward = true;
                self.last_find_till = till;

                crate::verbose_print!(
                    "[godot-neovim] {}{}: Found '{}' at col {}",
                    if till { "t" } else { "f" },
                    c,
                    c,
                    target_col
                );
                return;
            }
        }

        crate::verbose_print!("[godot-neovim] f/t: Character '{}' not found", c);
    }

    /// Find character backward on current line (F/T commands)
    fn find_char_backward(&mut self, c: char, till: bool) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        // Search for character before cursor
        for i in (0..col_idx).rev() {
            if chars[i] == c {
                let target_col = if till { i + 1 } else { i };
                self.move_cursor_to(line_idx, target_col as i32);

                // Save for ; and ,
                self.last_find_char = Some(c);
                self.last_find_forward = false;
                self.last_find_till = till;

                crate::verbose_print!(
                    "[godot-neovim] {}{}: Found '{}' at col {}",
                    if till { "T" } else { "F" },
                    c,
                    c,
                    target_col
                );
                return;
            }
        }

        crate::verbose_print!("[godot-neovim] F/T: Character '{}' not found", c);
    }

    /// Repeat last f/F/t/T command (; and , commands)
    fn repeat_find_char(&mut self, same_direction: bool) {
        let Some(c) = self.last_find_char else {
            crate::verbose_print!("[godot-neovim] ;/,: No previous find");
            return;
        };

        let forward = if same_direction {
            self.last_find_forward
        } else {
            !self.last_find_forward
        };
        let till = self.last_find_till;

        if forward {
            self.find_char_forward(c, till);
        } else {
            self.find_char_backward(c, till);
        }
    }

    /// Jump to matching bracket (% command)
    fn jump_to_matching_bracket(&mut self) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        if col_idx >= chars.len() {
            return;
        }

        let current_char = chars[col_idx];
        let (target_char, search_forward) = match current_char {
            '(' => (')', true),
            ')' => ('(', false),
            '[' => (']', true),
            ']' => ('[', false),
            '{' => ('}', true),
            '}' => ('{', false),
            '<' => ('>', true),
            '>' => ('<', false),
            _ => {
                crate::verbose_print!("[godot-neovim] %: Not on a bracket");
                return;
            }
        };

        let line_count = editor.get_line_count();
        let mut depth = 1;

        if search_forward {
            // Search forward
            let mut line = line_idx;
            let mut col = col_idx + 1;

            while line < line_count {
                let text = editor.get_line(line).to_string();
                let line_chars: Vec<char> = text.chars().collect();

                while col < line_chars.len() {
                    if line_chars[col] == current_char {
                        depth += 1;
                    } else if line_chars[col] == target_char {
                        depth -= 1;
                        if depth == 0 {
                            self.move_cursor_to(line, col as i32);
                            crate::verbose_print!("[godot-neovim] %: Jump to {}:{}", line + 1, col);
                            return;
                        }
                    }
                    col += 1;
                }
                line += 1;
                col = 0;
            }
        } else {
            // Search backward
            let mut line = line_idx;
            let mut col = col_idx as i32 - 1;

            while line >= 0 {
                let text = editor.get_line(line).to_string();
                let line_chars: Vec<char> = text.chars().collect();

                if col < 0 {
                    col = line_chars.len() as i32 - 1;
                }

                while col >= 0 {
                    if line_chars[col as usize] == current_char {
                        depth += 1;
                    } else if line_chars[col as usize] == target_char {
                        depth -= 1;
                        if depth == 0 {
                            self.move_cursor_to(line, col);
                            crate::verbose_print!("[godot-neovim] %: Jump to {}:{}", line + 1, col);
                            return;
                        }
                    }
                    col -= 1;
                }
                line -= 1;
                if line >= 0 {
                    col = editor.get_line(line).len() as i32 - 1;
                }
            }
        }

        crate::verbose_print!("[godot-neovim] %: Matching bracket not found");
    }

    /// Undo (u command)
    fn undo(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Save current cursor position before undo
        let saved_line = editor.get_caret_line();
        let saved_col = editor.get_caret_column();

        editor.undo();

        // Godot's undo may move cursor to old position - restore to near the current position
        // Vim behavior: cursor moves to the line where the change was undone
        // Since we don't know where the change was, keep cursor at saved position if valid
        let line_count = editor.get_line_count();
        let target_line = saved_line.min(line_count - 1);
        let line_length = editor.get_line(target_line).len() as i32;
        let target_col = saved_col.min(line_length.max(0));
        editor.set_caret_line(target_line);
        editor.set_caret_column(target_col);

        crate::verbose_print!(
            "[godot-neovim] u: Undo (cursor kept at line {})",
            target_line + 1
        );

        // Sync buffer to Neovim after undo
        self.sync_buffer_to_neovim();
        self.sync_cursor_to_neovim();
    }

    /// Redo (Ctrl+R command)
    fn redo(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Save current cursor position before redo
        let saved_line = editor.get_caret_line();
        let saved_col = editor.get_caret_column();

        editor.redo();

        // Keep cursor at saved position if valid
        let line_count = editor.get_line_count();
        let target_line = saved_line.min(line_count - 1);
        let line_length = editor.get_line(target_line).len() as i32;
        let target_col = saved_col.min(line_length.max(0));
        editor.set_caret_line(target_line);
        editor.set_caret_column(target_col);

        crate::verbose_print!(
            "[godot-neovim] Ctrl+R: Redo (cursor kept at line {})",
            target_line + 1
        );

        // Sync buffer to Neovim after redo
        self.sync_buffer_to_neovim();
        self.sync_cursor_to_neovim();
    }

    /// Move to start of line (0 command)
    fn move_to_line_start(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line = editor.get_caret_line();
        self.move_cursor_to(line, 0);
        crate::verbose_print!("[godot-neovim] 0: Moved to start of line");
    }

    /// Move to first non-blank character (^ command)
    fn move_to_first_non_blank(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();

        let first_non_blank = line_text
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);

        self.move_cursor_to(line_idx, first_non_blank as i32);
        crate::verbose_print!(
            "[godot-neovim] ^: Moved to first non-blank at col {}",
            first_non_blank
        );
    }

    /// Move to end of line ($ command)
    fn move_to_line_end(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();
        let line_len = line_text.chars().count();

        // Vim's $ goes to last character, not past it
        let target_col = if line_len > 0 { line_len - 1 } else { 0 };
        self.move_cursor_to(line_idx, target_col as i32);
        crate::verbose_print!(
            "[godot-neovim] $: Moved to end of line at col {}",
            target_col
        );
    }

    /// Move to previous paragraph ({ command)
    fn move_to_prev_paragraph(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();

        // Skip current empty lines
        let mut line = current_line - 1;
        while line > 0 {
            let text = editor.get_line(line).to_string();
            if text.trim().is_empty() {
                line -= 1;
            } else {
                break;
            }
        }

        // Find previous empty line
        while line > 0 {
            let text = editor.get_line(line).to_string();
            if text.trim().is_empty() {
                break;
            }
            line -= 1;
        }

        self.move_cursor_to(line.max(0), 0);
        crate::verbose_print!("[godot-neovim] {{: Moved to line {}", line + 1);
    }

    /// Move to next paragraph (} command)
    fn move_to_next_paragraph(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let current_line = editor.get_caret_line();
        let line_count = editor.get_line_count();

        // Skip current non-empty lines
        let mut line = current_line + 1;
        while line < line_count {
            let text = editor.get_line(line).to_string();
            if !text.trim().is_empty() {
                line += 1;
            } else {
                break;
            }
        }

        // Skip empty lines
        while line < line_count {
            let text = editor.get_line(line).to_string();
            if text.trim().is_empty() {
                line += 1;
            } else {
                break;
            }
        }

        self.move_cursor_to(line.min(line_count - 1), 0);
        crate::verbose_print!("[godot-neovim] }}: Moved to line {}", line + 1);
    }

    /// Delete character under cursor (x command)
    fn delete_char_forward(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column();
        let line_text = editor.get_line(line_idx).to_string();

        if (col_idx as usize) < line_text.chars().count() {
            let mut chars: Vec<char> = line_text.chars().collect();
            chars.remove(col_idx as usize);
            let new_line: String = chars.into_iter().collect();

            // Update editor
            editor.set_line(line_idx, &new_line);

            // Adjust cursor if needed
            let new_len = new_line.chars().count();
            if col_idx as usize >= new_len && new_len > 0 {
                editor.set_caret_column((new_len - 1) as i32);
            }

            self.sync_buffer_to_neovim();
            crate::verbose_print!("[godot-neovim] x: Deleted char at col {}", col_idx);
        }
    }

    /// Delete character before cursor (X command)
    fn delete_char_backward(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column();

        if col_idx > 0 {
            let line_text = editor.get_line(line_idx).to_string();
            let mut chars: Vec<char> = line_text.chars().collect();
            chars.remove((col_idx - 1) as usize);
            let new_line: String = chars.into_iter().collect();

            // Update editor
            editor.set_line(line_idx, &new_line);
            editor.set_caret_column(col_idx - 1);

            self.sync_buffer_to_neovim();
            crate::verbose_print!("[godot-neovim] X: Deleted char at col {}", col_idx - 1);
        }
    }

    /// Replace character under cursor (r command)
    fn replace_char(&mut self, c: char) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column();
        let line_text = editor.get_line(line_idx).to_string();

        if (col_idx as usize) < line_text.chars().count() {
            let mut chars: Vec<char> = line_text.chars().collect();
            chars[col_idx as usize] = c;
            let new_line: String = chars.into_iter().collect();

            editor.set_line(line_idx, &new_line);
            self.sync_buffer_to_neovim();
            crate::verbose_print!("[godot-neovim] r{}: Replaced char at col {}", c, col_idx);
        }
    }

    /// Toggle case of character under cursor (~ command)
    fn toggle_case(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column();
        let line_text = editor.get_line(line_idx).to_string();

        if (col_idx as usize) < line_text.chars().count() {
            let mut chars: Vec<char> = line_text.chars().collect();
            let c = chars[col_idx as usize];
            chars[col_idx as usize] = if c.is_uppercase() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c.to_uppercase().next().unwrap_or(c)
            };
            let new_line: String = chars.into_iter().collect();

            editor.set_line(line_idx, &new_line);

            // Move cursor forward (like Vim)
            let line_len = new_line.chars().count();
            if (col_idx as usize) < line_len - 1 {
                editor.set_caret_column(col_idx + 1);
            }

            self.sync_buffer_to_neovim();
            crate::verbose_print!("[godot-neovim] ~: Toggled case at col {}", col_idx);
        }
    }

    /// Set a mark at current position (m{a-z})
    fn set_mark(&mut self, mark: char) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line = editor.get_caret_line();
        let col = editor.get_caret_column();
        self.marks.insert(mark, (line, col));
        crate::verbose_print!(
            "[godot-neovim] m{}: Set mark at line {}, col {}",
            mark,
            line + 1,
            col
        );
    }

    /// Jump to mark line ('{a-z})
    fn jump_to_mark_line(&mut self, mark: char) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        let Some((line, _)) = self.marks.get(&mark).copied() else {
            crate::verbose_print!("[godot-neovim] '{}: Mark not set", mark);
            return;
        };

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_count = editor.get_line_count();
        let target_line = line.min(line_count - 1);
        editor.set_caret_line(target_line);

        // Move to first non-blank character (Vim behavior for ')
        let line_text = editor.get_line(target_line).to_string();
        let first_non_blank = line_text
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);
        editor.set_caret_column(first_non_blank as i32);

        self.sync_cursor_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] '{}: Jumped to line {}",
            mark,
            target_line + 1
        );
    }

    /// Jump to exact mark position (`{a-z})
    fn jump_to_mark_position(&mut self, mark: char) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        let Some((line, col)) = self.marks.get(&mark).copied() else {
            crate::verbose_print!("[godot-neovim] `{}: Mark not set", mark);
            return;
        };

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_count = editor.get_line_count();
        let target_line = line.min(line_count - 1);
        editor.set_caret_line(target_line);

        let line_length = editor.get_line(target_line).len() as i32;
        let target_col = col.min(line_length.max(0));
        editor.set_caret_column(target_col);

        self.sync_cursor_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] `{}: Jumped to line {}, col {}",
            mark,
            target_line + 1,
            target_col
        );
    }

    /// Start recording a macro to the specified register
    fn start_macro_recording(&mut self, register: char) {
        self.recording_macro = Some(register);
        self.macro_buffer.clear();
        crate::verbose_print!("[godot-neovim] q{}: Started recording macro", register);
    }

    /// Stop recording the current macro and save it
    fn stop_macro_recording(&mut self) {
        if let Some(register) = self.recording_macro.take() {
            let keys = std::mem::take(&mut self.macro_buffer);
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
    fn play_macro(&mut self, register: char) {
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
    fn replay_last_macro(&mut self) {
        if let Some(register) = self.last_macro {
            crate::verbose_print!("[godot-neovim] @@: Replaying macro '{}'", register);
            self.play_macro(register);
        } else {
            crate::verbose_print!("[godot-neovim] @@: No macro played yet");
        }
    }

    /// Yank current line to named register
    fn yank_line_to_register(&mut self, register: char) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();

        // Store with newline (line yank)
        self.registers.insert(register, format!("{}\n", line_text));
        crate::verbose_print!(
            "[godot-neovim] \"{}: Yanked line {} to register",
            register,
            line_idx + 1
        );
    }

    /// Delete current line and store in named register
    fn delete_line_to_register(&mut self, register: char) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();
        let line_count = editor.get_line_count();

        // Store with newline (line delete)
        self.registers.insert(register, format!("{}\n", line_text));

        // Delete the line
        if line_count > 1 {
            // Remove the line by setting text
            let mut lines: Vec<String> = Vec::new();
            for i in 0..line_count {
                if i != line_idx {
                    lines.push(editor.get_line(i).to_string());
                }
            }
            editor.set_text(&lines.join("\n"));

            // Adjust cursor position
            let new_line_count = editor.get_line_count();
            let target_line = line_idx.min(new_line_count - 1);
            editor.set_caret_line(target_line);

            // Move to first non-blank
            let target_text = editor.get_line(target_line).to_string();
            let first_non_blank = target_text
                .chars()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0);
            editor.set_caret_column(first_non_blank as i32);
        } else {
            // Last line - just clear it
            editor.set_line(0, "");
            editor.set_caret_column(0);
        }

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] \"{}: Deleted line {} to register",
            register,
            line_idx + 1
        );
    }

    /// Paste from named register (after cursor/below line)
    fn paste_from_register(&mut self, register: char) {
        let Some(content) = self.registers.get(&register).cloned() else {
            crate::verbose_print!("[godot-neovim] \"{}: Register empty", register);
            return;
        };

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Check if it's a line paste (ends with newline)
        if content.ends_with('\n') {
            // Line paste - insert below current line
            let line_idx = editor.get_caret_line();
            let line_count = editor.get_line_count();
            let paste_content = content.trim_end_matches('\n');

            // Build new text with inserted line
            let mut lines: Vec<String> = Vec::new();
            for i in 0..line_count {
                lines.push(editor.get_line(i).to_string());
                if i == line_idx {
                    lines.push(paste_content.to_string());
                }
            }
            editor.set_text(&lines.join("\n"));

            // Move cursor to the pasted line
            editor.set_caret_line(line_idx + 1);
            let first_non_blank = paste_content
                .chars()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0);
            editor.set_caret_column(first_non_blank as i32);
        } else {
            // Character paste - insert after cursor
            let line_idx = editor.get_caret_line();
            let col_idx = editor.get_caret_column();
            let line_text = editor.get_line(line_idx).to_string();

            let mut chars: Vec<char> = line_text.chars().collect();
            let insert_pos = ((col_idx + 1) as usize).min(chars.len());
            for (i, c) in content.chars().enumerate() {
                chars.insert(insert_pos + i, c);
            }
            let new_line: String = chars.into_iter().collect();
            editor.set_line(line_idx, &new_line);

            // Move cursor to end of pasted content
            editor.set_caret_column(insert_pos as i32 + content.len() as i32 - 1);
        }

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] \"{}p: Pasted from register", register);
    }

    /// Paste from named register (before cursor/above line)
    fn paste_from_register_before(&mut self, register: char) {
        let Some(content) = self.registers.get(&register).cloned() else {
            crate::verbose_print!("[godot-neovim] \"{}: Register empty", register);
            return;
        };

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Check if it's a line paste (ends with newline)
        if content.ends_with('\n') {
            // Line paste - insert above current line
            let line_idx = editor.get_caret_line();
            let line_count = editor.get_line_count();
            let paste_content = content.trim_end_matches('\n');

            // Build new text with inserted line
            let mut lines: Vec<String> = Vec::new();
            for i in 0..line_count {
                if i == line_idx {
                    lines.push(paste_content.to_string());
                }
                lines.push(editor.get_line(i).to_string());
            }
            editor.set_text(&lines.join("\n"));

            // Move cursor to the pasted line
            editor.set_caret_line(line_idx);
            let first_non_blank = paste_content
                .chars()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0);
            editor.set_caret_column(first_non_blank as i32);
        } else {
            // Character paste - insert before cursor
            let line_idx = editor.get_caret_line();
            let col_idx = editor.get_caret_column();
            let line_text = editor.get_line(line_idx).to_string();

            let mut chars: Vec<char> = line_text.chars().collect();
            let insert_pos = (col_idx as usize).min(chars.len());
            for (i, c) in content.chars().enumerate() {
                chars.insert(insert_pos + i, c);
            }
            let new_line: String = chars.into_iter().collect();
            editor.set_line(line_idx, &new_line);

            // Move cursor to end of pasted content
            editor.set_caret_column(insert_pos as i32 + content.len() as i32 - 1);
        }

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] \"{}P: Pasted from register (before)",
            register
        );
    }

    /// Indent current line (>> command)
    fn indent_line(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();

        // Add a tab at the beginning
        let new_line = format!("\t{}", line_text);
        editor.set_line(line_idx, &new_line);

        // Move cursor to first non-blank
        let first_non_blank = new_line
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);
        editor.set_caret_column(first_non_blank as i32);

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] >>: Indented line {}", line_idx + 1);
    }

    /// Unindent current line (<< command)
    fn unindent_line(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_text = editor.get_line(line_idx).to_string();

        // Remove leading whitespace (one level)
        let new_line = if let Some(stripped) = line_text.strip_prefix('\t') {
            stripped.to_string()
        } else if let Some(stripped) = line_text.strip_prefix("    ") {
            stripped.to_string()
        } else if let Some(stripped) = line_text.strip_prefix("  ") {
            stripped.to_string()
        } else if let Some(stripped) = line_text.strip_prefix(' ') {
            stripped.to_string()
        } else {
            line_text
        };

        editor.set_line(line_idx, &new_line);

        // Move cursor to first non-blank
        let first_non_blank = new_line
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);
        editor.set_caret_column(first_non_blank as i32);

        self.sync_buffer_to_neovim();
        crate::verbose_print!("[godot-neovim] <<: Unindented line {}", line_idx + 1);
    }

    /// Increment or decrement the number under/after cursor (Ctrl+A / Ctrl+X)
    fn increment_number(&mut self, delta: i32) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        // Find number at or after cursor
        let mut num_start = None;
        let mut num_end = None;

        // Search for number starting at or after cursor position
        for i in col_idx..chars.len() {
            if chars[i].is_ascii_digit() {
                // Found start of number, check for negative sign before it
                if i > 0 && chars[i - 1] == '-' {
                    num_start = Some(i - 1);
                } else {
                    num_start = Some(i);
                }
                // Find end of number
                for j in i..=chars.len() {
                    if j == chars.len() || !chars[j].is_ascii_digit() {
                        num_end = Some(j);
                        break;
                    }
                }
                break;
            }
        }

        // If no number found after cursor, search from beginning
        if num_start.is_none() {
            for i in 0..col_idx.min(chars.len()) {
                if chars[i].is_ascii_digit() {
                    if i > 0 && chars[i - 1] == '-' {
                        num_start = Some(i - 1);
                    } else {
                        num_start = Some(i);
                    }
                    for j in i..=chars.len() {
                        if j == chars.len() || !chars[j].is_ascii_digit() {
                            num_end = Some(j);
                            break;
                        }
                    }
                    break;
                }
            }
        }

        let (start, end) = match (num_start, num_end) {
            (Some(s), Some(e)) => (s, e),
            _ => {
                crate::verbose_print!("[godot-neovim] Ctrl+A/X: No number found");
                return;
            }
        };

        // Parse the number
        let num_str: String = chars[start..end].iter().collect();
        let Ok(num) = num_str.parse::<i64>() else {
            crate::verbose_print!("[godot-neovim] Ctrl+A/X: Failed to parse number");
            return;
        };

        // Calculate new value
        let new_num = num + delta as i64;
        let new_num_str = new_num.to_string();

        // Build new line
        let prefix: String = chars[..start].iter().collect();
        let suffix: String = chars[end..].iter().collect();
        let new_line = format!("{}{}{}", prefix, new_num_str, suffix);

        editor.set_line(line_idx, &new_line);

        // Position cursor at end of number
        let new_end = start + new_num_str.len();
        editor.set_caret_column((new_end - 1) as i32);

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] Ctrl+{}: {} -> {}",
            if delta > 0 { "A" } else { "X" },
            num,
            new_num
        );
    }

    /// Add current position to jump list
    fn add_to_jump_list(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line = editor.get_caret_line();
        let col = editor.get_caret_column();

        // Don't add duplicate consecutive entries
        if let Some(&last) = self.jump_list.last() {
            if last == (line, col) {
                return;
            }
        }

        // If we're not at the end of the list, truncate
        if self.jump_list_pos < self.jump_list.len() {
            self.jump_list.truncate(self.jump_list_pos);
        }

        self.jump_list.push((line, col));
        self.jump_list_pos = self.jump_list.len();

        // Limit jump list size
        const MAX_JUMP_LIST: usize = 100;
        if self.jump_list.len() > MAX_JUMP_LIST {
            self.jump_list.remove(0);
            self.jump_list_pos = self.jump_list.len();
        }
    }

    /// Jump back in jump list (Ctrl+O)
    fn jump_back(&mut self) {
        if self.jump_list.is_empty() {
            crate::verbose_print!("[godot-neovim] Ctrl+O: Jump list empty");
            return;
        }

        // Save current position before jumping
        if self.jump_list_pos == self.jump_list.len() {
            self.add_to_jump_list();
            self.jump_list_pos = self.jump_list.len();
        }

        if self.jump_list_pos == 0 {
            crate::verbose_print!("[godot-neovim] Ctrl+O: Already at oldest position");
            return;
        }

        self.jump_list_pos -= 1;
        let (line, col) = self.jump_list[self.jump_list_pos];

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_count = editor.get_line_count();
        let target_line = line.min(line_count - 1);
        editor.set_caret_line(target_line);

        let line_length = editor.get_line(target_line).len() as i32;
        let target_col = col.min(line_length.max(0));
        editor.set_caret_column(target_col);

        self.sync_cursor_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] Ctrl+O: Jumped back to line {}, col {}",
            target_line + 1,
            target_col
        );
    }

    /// Jump forward in jump list (Ctrl+I)
    fn jump_forward(&mut self) {
        if self.jump_list.is_empty() || self.jump_list_pos >= self.jump_list.len() - 1 {
            crate::verbose_print!("[godot-neovim] Ctrl+I: No newer position");
            return;
        }

        self.jump_list_pos += 1;
        let (line, col) = self.jump_list[self.jump_list_pos];

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_count = editor.get_line_count();
        let target_line = line.min(line_count - 1);
        editor.set_caret_line(target_line);

        let line_length = editor.get_line(target_line).len() as i32;
        let target_col = col.min(line_length.max(0));
        editor.set_caret_column(target_col);

        self.sync_cursor_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] Ctrl+I: Jumped forward to line {}, col {}",
            target_line + 1,
            target_col
        );
    }

    /// Join current line with next line (J command)
    fn join_lines(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let line_count = editor.get_line_count();

        if line_idx >= line_count - 1 {
            crate::verbose_print!("[godot-neovim] J: Already on last line");
            return;
        }

        let current_line = editor.get_line(line_idx).to_string();
        let next_line = editor.get_line(line_idx + 1).to_string();

        // Join with a space, trimming leading whitespace from next line
        let current_trimmed = current_line.trim_end();
        let next_trimmed = next_line.trim_start();

        let new_line = if current_trimmed.is_empty() {
            next_trimmed.to_string()
        } else if next_trimmed.is_empty() {
            current_trimmed.to_string()
        } else {
            format!("{} {}", current_trimmed, next_trimmed)
        };

        // Position cursor at the join point
        let join_col = current_trimmed.chars().count();

        // Update text
        editor.set_line(line_idx, &new_line);

        // Remove the next line
        // Need to get full text, remove the line, and set it back
        let full_text = editor.get_text().to_string();
        let lines: Vec<&str> = full_text.lines().collect();
        let mut new_lines: Vec<&str> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if i as i32 == line_idx {
                new_lines.push(&new_line);
            } else if i as i32 != line_idx + 1 {
                new_lines.push(line);
            }
        }
        let new_text = new_lines.join("\n");
        editor.set_text(&new_text);

        // Restore cursor position
        editor.set_caret_line(line_idx);
        editor.set_caret_column(join_col as i32);

        self.sync_buffer_to_neovim();
        crate::verbose_print!(
            "[godot-neovim] J: Joined lines {} and {}",
            line_idx + 1,
            line_idx + 2
        );
    }

    /// Move half page down (Ctrl+D command)
    fn half_page_down(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let visible_lines = editor.get_visible_line_count();
        let half_page = visible_lines / 2;
        let current_line = editor.get_caret_line();
        let line_count = editor.get_line_count();

        let target_line = (current_line + half_page).min(line_count - 1);
        editor.set_caret_line(target_line);

        // Also scroll the viewport
        let first_visible = editor.get_first_visible_line();
        let new_first = (first_visible + half_page).min(line_count - visible_lines);
        if new_first > first_visible {
            editor.set_line_as_first_visible(new_first.max(0));
        }

        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
        crate::verbose_print!("[godot-neovim] Ctrl+D: Moved to line {}", target_line + 1);
    }

    /// Move half page up (Ctrl+U command)
    fn half_page_up(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let visible_lines = editor.get_visible_line_count();
        let half_page = visible_lines / 2;
        let current_line = editor.get_caret_line();

        let target_line = (current_line - half_page).max(0);
        editor.set_caret_line(target_line);

        // Also scroll the viewport
        let first_visible = editor.get_first_visible_line();
        let new_first = (first_visible - half_page).max(0);
        if new_first < first_visible {
            editor.set_line_as_first_visible(new_first);
        }

        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
        crate::verbose_print!("[godot-neovim] Ctrl+U: Moved to line {}", target_line + 1);
    }

    /// Move full page down (Ctrl+F command)
    fn page_down(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let visible_lines = editor.get_visible_line_count();
        let current_line = editor.get_caret_line();
        let line_count = editor.get_line_count();

        let target_line = (current_line + visible_lines).min(line_count - 1);
        editor.set_caret_line(target_line);

        // Also scroll the viewport
        let first_visible = editor.get_first_visible_line();
        let new_first = (first_visible + visible_lines).min(line_count - visible_lines);
        if new_first > first_visible {
            editor.set_line_as_first_visible(new_first.max(0));
        }

        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
        crate::verbose_print!("[godot-neovim] Ctrl+F: Moved to line {}", target_line + 1);
    }

    /// Move full page up (Ctrl+B command)
    fn page_up(&mut self) {
        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        let visible_lines = editor.get_visible_line_count();
        let current_line = editor.get_caret_line();

        let target_line = (current_line - visible_lines).max(0);
        editor.set_caret_line(target_line);

        // Also scroll the viewport
        let first_visible = editor.get_first_visible_line();
        let new_first = (first_visible - visible_lines).max(0);
        if new_first < first_visible {
            editor.set_line_as_first_visible(new_first);
        }

        self.sync_cursor_to_neovim();
        self.update_cursor_from_editor();
        crate::verbose_print!("[godot-neovim] Ctrl+B: Moved to line {}", target_line + 1);
    }

    /// Go to definition (gd command) - uses Godot's built-in
    fn go_to_definition(&self) {
        // Simulate F12 or Ctrl+Click to go to definition
        let mut key_event = InputEventKey::new_gd();
        key_event.set_keycode(Key::F12);
        key_event.set_pressed(true);

        Input::singleton().parse_input_event(&key_event);
        crate::verbose_print!("[godot-neovim] gd: Go to definition (F12)");
    }

    /// Go to file under cursor (gf command)
    fn go_to_file_under_cursor(&mut self) {
        // Add to jump list before jumping
        self.add_to_jump_list();

        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();

        // Extract file path from around cursor position
        // Look for patterns like: "res://path/to/file.gd", 'path/file.gd', path/file
        let chars: Vec<char> = line_text.chars().collect();

        if col_idx >= chars.len() {
            crate::verbose_print!("[godot-neovim] gf: Cursor at end of line");
            return;
        }

        // Find start and end of path-like text
        let path_chars = |c: char| {
            c.is_alphanumeric() || c == '/' || c == '.' || c == '_' || c == '-' || c == ':'
        };

        let mut start = col_idx;
        while start > 0 && path_chars(chars[start - 1]) {
            start -= 1;
        }

        let mut end = col_idx;
        while end < chars.len() && path_chars(chars[end]) {
            end += 1;
        }

        if start == end {
            crate::verbose_print!("[godot-neovim] gf: No file path under cursor");
            return;
        }

        let path: String = chars[start..end].iter().collect();
        crate::verbose_print!("[godot-neovim] gf: Extracted path: {}", path);

        // Try to open the file
        self.cmd_edit(&path);
    }
}
