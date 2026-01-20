//! Godot Neovim Plugin - Main module

/// Plugin version: Cargo.toml version for release, build datetime for debug
const VERSION: &str = env!("BUILD_VERSION");

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

use crate::lsp::GodotLspClient;
use crate::neovim::NeovimClient;
use crate::settings;
use crate::sync::SyncManager;
use godot::classes::text_edit::CaretType;
use godot::classes::{
    CodeEdit, Control, EditorInterface, EditorPlugin, IEditorPlugin, Label, ProjectSettings,
};
use godot::global::Key;
use godot::prelude::*;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

/// Extension trait for CodeEdit to emit text_changed signal after set_text
/// Godot's set_text() does not emit text_changed signal, causing dirty flag to not be set
pub(crate) trait CodeEditExt {
    fn set_text_and_notify(&mut self, text: &str);
}

impl CodeEditExt for Gd<CodeEdit> {
    fn set_text_and_notify(&mut self, text: &str) {
        // Godot's set_text() updates version counter internally when undo is enabled.
        // ScriptEditor uses get_version() != get_saved_version() for dirty flag.
        self.set_text(text);

        // Emit name_changed signal on ScriptTextEditor to update script list UI.
        // Hierarchy: CodeEdit -> CodeTextEditor -> VSplitContainer -> ScriptTextEditor
        let mut current: Option<Gd<Node>> = self.get_parent();
        while let Some(node) = current {
            if node.get_class() == "ScriptTextEditor".into() {
                let mut script_editor = node;
                script_editor.emit_signal("name_changed", &[]);
                break;
            }
            current = node.get_parent();
        }
    }
}

/// Help query for goto_help()
#[derive(Debug, Clone)]
pub struct HelpQuery {
    /// Class name (e.g., "Node", "Vector2")
    pub class_name: String,
    /// Member name (e.g., "get_name", "position") - None for class-level help
    pub member_name: Option<String>,
    /// Member type for constructing the help query
    pub member_type: HelpMemberType,
}

/// Type of member for help query
#[derive(Debug, Clone, PartialEq)]
pub enum HelpMemberType {
    /// Class documentation (class_name:ClassName)
    Class,
    /// Method documentation (class_method:ClassName:method)
    Method,
    /// Property documentation (class_property:ClassName:property)
    Property,
    /// Signal documentation (class_signal:ClassName:signal)
    Signal,
    /// Constant documentation (class_constant:ClassName:constant)
    Constant,
}

impl HelpQuery {
    /// Convert to goto_help() query string
    pub fn to_help_string(&self) -> String {
        match self.member_type {
            HelpMemberType::Class => format!("class_name:{}", self.class_name),
            HelpMemberType::Method => {
                if let Some(ref member) = self.member_name {
                    format!("class_method:{}:{}", self.class_name, member)
                } else {
                    format!("class_name:{}", self.class_name)
                }
            }
            HelpMemberType::Property => {
                if let Some(ref member) = self.member_name {
                    format!("class_property:{}:{}", self.class_name, member)
                } else {
                    format!("class_name:{}", self.class_name)
                }
            }
            HelpMemberType::Signal => {
                if let Some(ref member) = self.member_name {
                    format!("class_signal:{}:{}", self.class_name, member)
                } else {
                    format!("class_name:{}", self.class_name)
                }
            }
            HelpMemberType::Constant => {
                if let Some(ref member) = self.member_name {
                    format!("class_constant:{}:{}", self.class_name, member)
                } else {
                    format!("class_name:{}", self.class_name)
                }
            }
        }
    }
}

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
    /// Timestamp when last_key was set (for timeout detection)
    #[init(val = None)]
    last_key_time: Option<Instant>,
    /// Flag indicating Insert mode exit is in progress (vscode-neovim style)
    /// When true, keys are buffered in pending_keys_after_exit
    #[init(val = false)]
    is_exiting_insert_mode: bool,
    /// Keys pressed during Insert mode exit (vscode-neovim style)
    /// These are sent after exit completes to prevent key loss
    #[init(val = String::new())]
    pending_keys_after_exit: String,
    /// Command line input buffer for ':' commands
    #[init(val = String::new())]
    command_buffer: String,
    /// Flag indicating command-line mode is active
    #[init(val = false)]
    command_mode: bool,
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
    /// Flag indicating search mode is active (/ or ?)
    #[init(val = false)]
    search_mode: bool,
    /// Search input buffer for '/' and '?' commands
    #[init(val = String::new())]
    search_buffer: String,
    /// Search direction (true = forward /, false = backward ?)
    #[init(val = true)]
    search_forward: bool,
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
    /// Last synced cursor position: (line, col) for detecting external cursor changes
    /// Used to prevent sync loops between Godot and Neovim
    #[init(val = (-1, -1))]
    last_synced_cursor: (i64, i64),
    /// Flag indicating script changed signal was received (for deferred processing)
    /// Uses Cell for interior mutability to avoid borrow conflicts with signal callbacks
    #[init(val = Cell::new(false))]
    script_changed_pending: Cell<bool>,
    /// Pending documentation lookup query (for deferred goto_help to avoid borrow conflicts)
    #[init(val = None)]
    pending_help_query: Option<HelpQuery>,
    /// Pending file path to open (for deferred cmd_edit to avoid borrow conflicts)
    #[init(val = None)]
    pending_file_path: Option<String>,
    /// Expected script path after script change (for verifying correct CodeEdit)
    #[init(val = None)]
    expected_script_path: Option<String>,
    /// Retry count for finding the correct CodeEdit after script change
    #[init(val = 0)]
    script_change_retry_count: u32,
    /// Current script path (for LSP and buffer name)
    #[init(val = String::new())]
    current_script_path: String,
    /// Whether LSP is connected
    #[init(val = false)]
    lsp_connected: bool,
    /// Direct LSP client for Godot LSP server
    #[init(val = None)]
    godot_lsp: Option<Arc<GodotLspClient>>,
    /// Temporary version display flag (cleared on next operation)
    #[init(val = false)]
    show_version: bool,
    /// Buffer synchronization manager (ComradeNeovim-style changedtick sync)
    #[init(val = SyncManager::new())]
    sync_manager: SyncManager,
    /// Flag to skip cursor sync in on_script_changed (set by cmd_close)
    #[init(val = false)]
    cursor_synced_before_close: bool,
    /// Flag to skip on_script_changed processing during :qa (Close All)
    /// Reset when operation completes (detected in process())
    #[init(val = false)]
    closing_all_tabs: bool,
    /// Buffers to delete from Neovim after :qa completes
    /// Collected during closing_all_tabs to avoid sync commands during dialog processing
    #[init(val = Vec::new())]
    pending_buffer_deletions: Vec<String>,
    /// Last Neovim line we synced to (to prevent repeated clamping syncs)
    /// This is separate from last_synced_cursor because we need to track the NEOVIM line,
    /// not the Godot line, to prevent loops when user clicks on clamped line with different columns
    #[init(val = -1)]
    last_nvim_synced_line: i64,
    /// Flag to ignore caret_changed during sync_cursor_from_grid
    /// Prevents RPC calls during caret update (which causes timeout on rapid key presses)
    #[init(val = false)]
    syncing_from_grid: bool,
    /// Flag to skip viewport sync when cursor was changed by user interaction (click)
    /// This prevents Neovim from overriding user's scroll position
    #[init(val = false)]
    user_cursor_sync: bool,
    /// Last known visible line count (for detecting editor resize)
    /// Used to resize Neovim UI when Godot editor size changes
    #[init(val = 0)]
    last_visible_lines: i32,
    /// Flag to skip grid_cursor_goto sync after buffer switch
    /// When buffer is switched, viewport values may be the same as before close,
    /// causing take_viewport() to return None and grid_cursor_goto to be used
    /// This flag prevents incorrect cursor positioning after :q and reopen
    #[init(val = false)]
    skip_grid_cursor_after_switch: bool,
    /// Flag to apply cursor correction after Ctrl+B
    /// With ext_multigrid, Ctrl+B at end of file reports wrong viewport height,
    /// causing cursor to barely move. This flag triggers correction after viewport sync.
    #[init(val = false)]
    pending_page_up_correction: bool,
}

#[godot_api]
impl IEditorPlugin for GodotNeovimPlugin {
    fn enter_tree(&mut self) {
        crate::verbose_print!("[godot-neovim] v{} loaded", VERSION);

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
                // Get addons path for Lua plugin
                let addons_path = ProjectSettings::singleton()
                    .globalize_path("res://addons/godot-neovim")
                    .to_string();

                if let Err(e) = client.start(Some(&addons_path)) {
                    godot_error!("[godot-neovim] Failed to start Neovim: {}", e);
                    return;
                }

                self.neovim = Some(Mutex::new(client));

                // Create LSP client only if use_thread is enabled in editor settings
                // (LSP server won't respond without threading enabled)
                let use_thread = EditorInterface::singleton()
                    .get_editor_settings()
                    .map(|settings| {
                        settings
                            .get_setting("network/language_server/use_thread")
                            .try_to::<bool>()
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);

                if use_thread {
                    let lsp_client = Arc::new(GodotLspClient::new());
                    self.godot_lsp = Some(lsp_client);
                    self.lsp_connected = true;
                    crate::verbose_print!(
                        "[godot-neovim] LSP client initialized (use_thread=true)"
                    );
                } else {
                    crate::verbose_print!("[godot-neovim] LSP disabled (use_thread=false)");
                }
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

        // Try to find existing CodeEdit (indicates hot reload if found)
        self.find_current_code_edit();
        if self.current_editor.is_some() {
            crate::verbose_print!(
                "[godot-neovim] Found existing CodeEdit - triggering full reinitialization (hot reload)"
            );
            // Trigger full reinitialization via deferred call
            // This uses the same flow as on_script_changed for consistent behavior
            self.script_changed_pending.set(true);
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

        // Disconnect and clear LSP client
        if let Some(ref lsp) = self.godot_lsp {
            lsp.disconnect();
        }
        self.godot_lsp = None;

        // Neovim client will be stopped when dropped (with timeout)
        self.neovim = None;

        crate::verbose_print!("[godot-neovim] Plugin exit complete");
    }

    fn process(&mut self, _delta: f64) {
        // Event-driven reset for closing_all_tabs flag
        // Only reset when ALL scripts are closed (not when cancelled)
        if self.closing_all_tabs {
            let editor = EditorInterface::singleton();
            if let Some(script_editor) = editor.get_script_editor() {
                let open_scripts = script_editor.get_open_scripts();

                // Only reset when all scripts are closed
                // If user cancels, the flag will be reset on next user input
                if open_scripts.is_empty() {
                    crate::verbose_print!(
                        "[godot-neovim] :qa - All scripts closed, resetting flag"
                    );
                    self.closing_all_tabs = false;

                    // Delete pending buffers from Neovim (deferred from on_script_close)
                    let pending = std::mem::take(&mut self.pending_buffer_deletions);
                    for path in pending {
                        self.delete_neovim_buffer(&path);
                    }
                }
            }
        }

        // Handle deferred script change (set by on_script_changed to avoid borrow conflicts)
        if self.script_changed_pending.get() {
            self.script_changed_pending.set(false);
            self.handle_script_changed();
        }

        // Handle deferred documentation lookup (K command)
        // goto_help() triggers editor_script_changed signal synchronously, which would
        // cause a borrow conflict. We temporarily disconnect from the signal, call
        // goto_help(), then reconnect and manually trigger the handler.
        if let Some(query) = self.pending_help_query.take() {
            let editor_interface = EditorInterface::singleton();
            if let Some(mut script_editor) = editor_interface.get_script_editor() {
                // Temporarily disconnect from signal to avoid borrow conflict
                let callable = self.base().callable("on_script_changed");
                script_editor.disconnect("editor_script_changed", &callable);

                // Now safe to call goto_help() with the constructed query
                let help_string = query.to_help_string();
                script_editor.goto_help(&help_string);
                crate::verbose_print!(
                    "[godot-neovim] K: Opening help with query '{}' (deferred)",
                    help_string
                );

                // Reconnect to signal
                script_editor.connect("editor_script_changed", &callable);

                // Manually handle the script change since we missed the signal
                self.handle_script_changed();
            }
        }

        // Handle deferred file open (gf command)
        // cmd_edit() triggers editor_script_changed signal synchronously, which would
        // cause a borrow conflict. We temporarily disconnect from the signal.
        if let Some(path) = self.pending_file_path.take() {
            let editor_interface = EditorInterface::singleton();
            if let Some(mut script_editor) = editor_interface.get_script_editor() {
                // Temporarily disconnect from signal to avoid borrow conflict
                let callable = self.base().callable("on_script_changed");
                script_editor.disconnect("editor_script_changed", &callable);

                // Set expected script path for verification
                // Convert relative path to res:// path if needed
                let expected_path = if path.starts_with("res://") {
                    path.clone()
                } else {
                    format!("res://{}", path)
                };
                self.expected_script_path = Some(expected_path.clone());
                self.script_change_retry_count = 0;
                crate::verbose_print!(
                    "[godot-neovim] gf: Expected script path: '{}'",
                    expected_path
                );

                // Now safe to open the file
                crate::verbose_print!("[godot-neovim] gf: Opening file '{}' (deferred)", path);
                self.cmd_edit(&path);

                // Reconnect to signal
                script_editor.connect("editor_script_changed", &callable);

                // Manually trigger handle_script_changed since we missed the signal
                crate::verbose_print!(
                    "[godot-neovim] gf: Triggering manual script change handling"
                );
                self.handle_script_changed();
            }
        }

        // Check for pending updates from Neovim redraw events
        self.process_neovim_updates();

        // Check for key sequence timeout (like Neovim's timeoutlen)
        // Only applies in Normal mode - Insert/Replace/Visual modes don't use operator-pending
        // If last_key has been pending too long, cancel it
        if !self.is_insert_mode() && !self.is_replace_mode() && !self.is_in_visual_mode() {
            if let Some(key_time) = self.last_key_time {
                let timeoutlen = crate::settings::get_timeoutlen();
                if key_time.elapsed().as_millis() > timeoutlen as u128 {
                    if !self.last_key.is_empty() {
                        crate::verbose_print!(
                            "[godot-neovim] Key sequence timeout: '{}' ({}ms elapsed)",
                            self.last_key,
                            key_time.elapsed().as_millis()
                        );
                        // Cancel Neovim's pending operator
                        if let Some(ref neovim) = self.neovim {
                            if let Ok(client) = neovim.try_lock() {
                                let _ = client.input("<Esc>");
                            }
                        }
                        // Clear directly here (not using clear_last_key() to avoid double clearing last_key_time)
                        self.last_key.clear();
                    }
                    self.last_key_time = None;

                    // Also clear related pending states on timeout
                    self.selected_register = None;
                    self.count_buffer.clear();
                }
            }
        }
    }

    fn input(&mut self, event: Gd<godot::classes::InputEvent>) {
        // Reset closing_all_tabs flag on user input (after :qa cancel)
        // This reconnects to the current script after dialog processing completes
        if self.closing_all_tabs {
            self.closing_all_tabs = false;
            // Clear pending deletions - user cancelled, so don't delete buffers
            self.pending_buffer_deletions.clear();
            crate::verbose_print!("[godot-neovim] :qa - Resetting flag on user input (cancelled)");
            // Reconnect to current script
            self.handle_script_changed();
        }

        // Handle mouse click events - sync cursor position after click
        if let Ok(mouse_event) = event
            .clone()
            .try_cast::<godot::classes::InputEventMouseButton>()
        {
            // Only handle left mouse button press when editor has focus
            if mouse_event.is_pressed()
                && mouse_event.get_button_index() == godot::global::MouseButton::LEFT
                && self.editor_has_focus()
            {
                // Use deferred call to sync cursor after Godot updates caret position
                self.base_mut()
                    .call_deferred("sync_cursor_to_neovim_deferred", &[]);
            }
            return;
        }

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

        let keycode = key_event.get_keycode();
        crate::verbose_print!(
            "[godot-neovim] input: mode={}, key={:?}, keycode_ord={}, BRACKETLEFT_ord={}",
            self.current_mode,
            keycode,
            keycode.ord(),
            Key::BRACKETLEFT.ord()
        );

        // Handle command-line mode input
        if self.command_mode {
            self.handle_command_mode_input(&key_event);
            return;
        }

        // Handle search mode input (/ or ?)
        if self.search_mode {
            self.handle_search_mode_input(&key_event);
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

            // Connect to script close signal (for Neovim buffer cleanup)
            let close_callable = self.base().callable("on_script_close");
            script_editor.connect("script_close", &close_callable);
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

    fn connect_caret_changed_signal(&mut self) {
        // Create callable first to avoid borrow conflicts
        let callable = self.base().callable("on_caret_changed");

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Connect to caret_changed signal to detect mouse clicks and other cursor changes
        // Check if already connected to avoid duplicate connections
        if !editor.is_connected("caret_changed", &callable) {
            editor.connect("caret_changed", &callable);
            crate::verbose_print!("[godot-neovim] Connected to caret_changed signal");
        }
    }

    fn disconnect_caret_changed_signal(&mut self) {
        // Create callable first to avoid borrow conflicts
        let callable = self.base().callable("on_caret_changed");

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Disconnect from caret_changed signal before closing
        if editor.is_connected("caret_changed", &callable) {
            editor.disconnect("caret_changed", &callable);
            crate::verbose_print!("[godot-neovim] Disconnected from caret_changed signal");
        }
    }

    fn connect_resized_signal(&mut self) {
        // Create callable first to avoid borrow conflicts
        let callable = self.base().callable("on_editor_resized");

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Connect to resized signal to detect editor size changes
        // (window resize, panel toggle, dock width changes, etc.)
        if !editor.is_connected("resized", &callable) {
            editor.connect("resized", &callable);
            crate::verbose_print!("[godot-neovim] Connected to resized signal");
        }
    }

    fn disconnect_resized_signal(&mut self) {
        // Create callable first to avoid borrow conflicts
        let callable = self.base().callable("on_editor_resized");

        let Some(ref mut editor) = self.current_editor else {
            return;
        };

        // Disconnect from resized signal before closing
        if editor.is_connected("resized", &callable) {
            editor.disconnect("resized", &callable);
            crate::verbose_print!("[godot-neovim] Disconnected from resized signal");
        }
    }

    #[func]
    fn on_editor_resized(&mut self) {
        // Resize Neovim UI to match new editor size
        let Some(ref editor) = self.current_editor else {
            return;
        };

        let visible_lines = editor.get_visible_line_count();
        if visible_lines != self.last_visible_lines && visible_lines > 0 {
            self.last_visible_lines = visible_lines;

            // Clear user_cursor_sync flag since resize might trigger caret_changed
            // but we still want to apply viewport changes from Neovim after resize
            self.user_cursor_sync = false;

            let Some(ref neovim) = self.neovim else {
                return;
            };

            let Ok(client) = neovim.try_lock() else {
                return;
            };

            let width = 120i64;
            let height = (visible_lines as i64).max(10);
            crate::verbose_print!(
                "[godot-neovim] Resize on editor resize: visible_lines={}, height={}",
                visible_lines,
                height
            );
            client.ui_try_resize(width, height);
        }
    }

    #[func]
    fn on_caret_changed(&mut self) {
        // Skip if syncing from grid (to prevent RPC during caret update)
        // This happens when set_caret_line/column are called from sync_cursor_from_grid
        if self.syncing_from_grid {
            return;
        }

        // Skip sync in Insert/Replace modes - cursor moves with every keystroke
        // and Neovim isn't receiving the input, so syncing is meaningless and causes freezes
        if self.is_insert_mode() || self.is_replace_mode() {
            return;
        }

        // Skip sync in Visual mode - Neovim controls the selection
        // Godot's CodeEdit caret_changed fires when selection updates, causing sync loops
        // Following Master-Slave design: Neovim is master, Godot only reflects its state
        if self.is_in_visual_mode() {
            return;
        }

        let Some(ref editor) = self.current_editor else {
            return;
        };

        // Check if editor is still valid (may have been freed)
        if !editor.is_instance_valid() {
            return;
        }

        // Get current cursor position from Godot editor
        let line = editor.get_caret_line();
        let col = editor.get_caret_column();

        // Check if cursor actually changed (to prevent sync loops)
        if self.last_synced_cursor == (line as i64, col as i64) {
            return;
        }

        // Set flag to skip viewport sync from Neovim
        // This prevents Neovim from overriding user's scroll position when clicking
        self.user_cursor_sync = true;

        // Update last_synced_cursor and sync to Neovim
        self.last_synced_cursor = (line as i64, col as i64);
        self.sync_cursor_to_neovim();

        // Update mode label with new cursor position
        // Display uses 1-indexed line number
        let display_cursor = (line as i64 + 1, col as i64);
        self.update_mode_display_with_cursor(&self.current_mode.clone(), Some(display_cursor));
    }

    #[func]
    fn sync_cursor_to_neovim_deferred(&mut self) {
        // Called after mouse click to sync cursor position
        // Deferred to ensure Godot has updated the caret position first
        // Note: We sync even in insert mode because clicking should move
        // the insertion point in Neovim too

        let Some(ref editor) = self.current_editor else {
            return;
        };

        let line = editor.get_caret_line();
        let col = editor.get_caret_column();

        // Update last_synced_cursor and sync to Neovim
        self.last_synced_cursor = (line as i64, col as i64);
        self.sync_cursor_to_neovim();
    }

    #[func]
    fn on_settings_changed(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(editor_settings) = editor.get_editor_settings() {
            settings::on_settings_changed(&editor_settings);
        }
    }

    #[func]
    fn on_script_changed(&mut self, script: Option<Gd<godot::classes::Script>>) {
        // Skip processing during :qa (Close All) to avoid errors
        // Flag will be reset by process() when operation completes
        if self.closing_all_tabs {
            crate::verbose_print!("[godot-neovim] Skipping on_script_changed (closing all tabs)");
            return;
        }

        // Sync cursor to Neovim before switching files
        // Skip if cursor was already synced by cmd_close (to avoid overwriting with wrong position)
        if self.cursor_synced_before_close {
            self.cursor_synced_before_close = false;
            crate::verbose_print!(
                "[godot-neovim] Skipping cursor sync (already synced before close)"
            );
        } else if !self.current_script_path.is_empty() {
            // This ensures Neovim remembers the cursor position for the current buffer
            // Check if editor is still valid (it may have been freed when closing a script)
            if let Some(ref editor) = self.current_editor {
                if editor.is_instance_valid() {
                    let line = editor.get_caret_line() as i64 + 1; // 1-indexed for Neovim
                    let col = editor.get_caret_column() as i64;
                    if let Some(ref neovim) = self.neovim {
                        if let Ok(client) = neovim.try_lock() {
                            let _ = client.set_cursor(line, col);
                            crate::verbose_print!(
                                "[godot-neovim] Synced cursor to Neovim for {}: ({}, {})",
                                self.current_script_path,
                                line,
                                col
                            );
                        }
                    }
                }
            }
        }

        // Handle null script (e.g., when all scripts are closed)
        let Some(script) = script else {
            crate::verbose_print!(
                "[godot-neovim] on_script_changed: null script, clearing references"
            );
            self.current_editor = None;
            self.mode_label = None;
            self.current_script_path.clear();
            return;
        };

        // Store the expected script path for verification in deferred handler
        let script_path = script.get_path().to_string();
        crate::verbose_print!("[godot-neovim] on_script_changed: {}", script_path);
        self.expected_script_path = Some(script_path);
        self.script_change_retry_count = 0;

        // Only set flag - actual handling deferred to process() to avoid borrow conflicts
        // when signals are emitted during input processing (e.g., K command opening docs)
        self.script_changed_pending.set(true);
    }

    /// Called when a script is closed in Godot
    /// Deletes the corresponding buffer from Neovim
    #[func]
    fn on_script_close(&mut self, script: Gd<godot::classes::Script>) {
        let path = script.get_path().to_string();
        if path.is_empty() {
            return;
        }

        crate::verbose_print!("[godot-neovim] on_script_close: {}", path);

        // During :qa, defer buffer deletion to avoid Viewport errors
        // Neovim commands during dialog processing can cause issues
        if self.closing_all_tabs {
            self.pending_buffer_deletions.push(path);
            crate::verbose_print!(
                "[godot-neovim] on_script_close: Deferred deletion (closing_all_tabs)"
            );
            return;
        }

        // Delete the buffer from Neovim immediately
        self.delete_neovim_buffer(&path);
    }

    /// Delete a buffer from Neovim by path
    fn delete_neovim_buffer(&self, path: &str) {
        let Some(ref neovim) = self.neovim else {
            return;
        };

        let Ok(client) = neovim.try_lock() else {
            return;
        };

        // Use bwipeout to completely remove buffer (including undo history)
        // This matches vscode-neovim's behavior with force=true
        let cmd = format!("silent! bwipeout! {}", path);
        if let Err(e) = client.command(&cmd) {
            crate::verbose_print!("[godot-neovim] Failed to delete buffer {}: {}", path, e);
        } else {
            crate::verbose_print!("[godot-neovim] Deleted buffer from Neovim: {}", path);
        }
    }

    #[func]
    fn handle_script_changed_deferred(&mut self) {
        crate::verbose_print!("[godot-neovim] Script changed (deferred processing)");

        // Verify we're on the expected script before syncing
        let editor = EditorInterface::singleton();
        let Some(mut script_editor) = editor.get_script_editor() else {
            crate::verbose_print!("[godot-neovim] No script editor found");
            return;
        };

        // Get the current script path from ScriptEditor (source of truth)
        let current_script_path = script_editor
            .get_current_script()
            .map(|s| s.get_path().to_string())
            .unwrap_or_default();

        crate::verbose_print!(
            "[godot-neovim] Current script: '{}', Expected: '{}'",
            current_script_path,
            self.expected_script_path.as_deref().unwrap_or("(none)")
        );

        // If we have an expected path and it doesn't match, retry up to 3 times
        if let Some(ref expected_path) = self.expected_script_path {
            if !current_script_path.is_empty()
                && !expected_path.is_empty()
                && current_script_path != *expected_path
            {
                self.script_change_retry_count += 1;
                if self.script_change_retry_count < 3 {
                    crate::verbose_print!(
                        "[godot-neovim] Script mismatch, retrying ({}/3)...",
                        self.script_change_retry_count
                    );
                    // Retry in the next deferred call
                    self.base_mut()
                        .call_deferred("handle_script_changed_deferred", &[]);
                    return;
                }
                crate::verbose_print!(
                    "[godot-neovim] Script mismatch after retries, proceeding anyway"
                );
            }
        }

        self.find_current_code_edit();

        // Verify CodeEdit content matches current script
        // If mismatch, retry instead of syncing wrong buffer
        if let Some(ref editor) = self.current_editor {
            let editor_lines = editor.get_line_count();
            let editor_first_line = if editor_lines > 0 {
                editor.get_line(0).to_string()
            } else {
                String::new()
            };

            // Get current script's source for comparison
            if let Some(current_script) = script_editor.get_current_script() {
                let script_source = current_script.get_source_code().to_string();
                let script_first_line = script_source.lines().next().unwrap_or("");
                let script_lines = script_source.lines().count();

                crate::verbose_print!(
                    "[godot-neovim] Verification - CodeEdit: {} lines, first='{}'; Script: {} lines, first='{}'",
                    editor_lines,
                    editor_first_line.chars().take(30).collect::<String>(),
                    script_lines,
                    script_first_line.chars().take(30).collect::<String>()
                );

                // If content doesn't match, the CodeEdit is stale - retry
                let content_matches = editor_first_line.trim() == script_first_line.trim()
                    || editor_lines as usize == script_lines.max(1);

                if !content_matches {
                    self.script_change_retry_count += 1;
                    if self.script_change_retry_count < 5 {
                        crate::verbose_print!(
                            "[godot-neovim] Content mismatch, retrying ({}/5)...",
                            self.script_change_retry_count
                        );
                        // Clear expected path to prevent path-based retry
                        self.expected_script_path = None;
                        // Retry
                        self.base_mut()
                            .call_deferred("handle_script_changed_deferred", &[]);
                        return;
                    }
                    crate::verbose_print!(
                        "[godot-neovim] Content mismatch after retries, proceeding anyway"
                    );
                }
            }
        }

        // Clear the expected path
        self.expected_script_path = None;

        // Update current script path for LSP
        self.current_script_path = current_script_path.clone();

        self.reposition_mode_label();

        // Switch to Neovim buffer for this file (creates if not exists)
        // Returns cursor position from Neovim for existing buffers
        if let Some((line, col)) = self.switch_to_neovim_buffer() {
            // Apply cursor position from Neovim to Godot editor
            if let Some(ref mut editor) = self.current_editor {
                let line_count = editor.get_line_count();
                let safe_line = (line as i32).min(line_count - 1).max(0);
                let line_length = editor.get_line(safe_line).len() as i32;
                let safe_col = (col as i32).min(line_length).max(0);

                crate::verbose_print!(
                    "[godot-neovim] Applying cursor from Neovim: ({}, {}) -> ({}, {})",
                    line,
                    col,
                    safe_line,
                    safe_col
                );

                // Set syncing_from_grid to prevent on_caret_changed from setting user_cursor_sync
                // This ensures zz/zt/zb viewport commands work after buffer switch
                self.syncing_from_grid = true;
                editor.set_caret_line(safe_line);
                editor.set_caret_column(safe_col);
                self.syncing_from_grid = false;
            }
        }

        self.update_cursor_from_editor();
        self.sync_cursor_to_neovim();
    }

    fn handle_script_changed(&mut self) {
        // Use call_deferred to ensure Godot has fully switched to the new script
        // before we try to find the CodeEdit and sync buffer
        self.base_mut()
            .call_deferred("handle_script_changed_deferred", &[]);
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
            // Verify the CodeEdit is still valid (may have been freed when script closed)
            if !code_edit.is_instance_valid() {
                self.current_editor = None;
                return;
            }
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
        // Clear the reference first to avoid use-after-free when script is closed
        self.current_editor = None;

        let editor = EditorInterface::singleton();
        if let Some(script_editor) = editor.get_script_editor() {
            // Try to find the currently focused CodeEdit first
            if let Some(code_edit) =
                self.find_focused_code_edit(script_editor.clone().upcast::<Control>())
            {
                crate::verbose_print!("[godot-neovim] Found focused CodeEdit");
                self.current_editor = Some(code_edit);
                self.connect_caret_changed_signal();
                self.connect_resized_signal();
                return;
            }
            // Fallback: find visible CodeEdit
            if let Some(code_edit) = self.find_visible_code_edit(script_editor.upcast::<Control>())
            {
                crate::verbose_print!("[godot-neovim] Found visible CodeEdit");
                self.current_editor = Some(code_edit);
                self.connect_caret_changed_signal();
                self.connect_resized_signal();
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
    /// Neovim mode_change events can send "i" or "insert" depending on context
    fn is_insert_mode(&self) -> bool {
        self.current_mode == "i" || self.current_mode == "insert"
    }

    /// Check if currently in replace mode
    /// Neovim mode_change events can send "R" or "replace" depending on context
    fn is_replace_mode(&self) -> bool {
        self.current_mode == "R" || self.current_mode == "replace"
    }

    /// Check if mode is a visual mode (v, V, or Ctrl+V)
    fn is_visual_mode(mode: &str) -> bool {
        matches!(mode, "v" | "V" | "\x16" | "^V" | "CTRL-V" | "visual")
    }

    /// Check if currently in visual mode (instance method)
    fn is_in_visual_mode(&self) -> bool {
        Self::is_visual_mode(&self.current_mode)
    }

    /// Check if mode is operator-pending mode (d, c, y, etc. waiting for motion)
    fn is_operator_pending_mode(mode: &str) -> bool {
        matches!(mode, "operator" | "no")
    }

    /// Clear all pending input states to ensure mutual exclusivity
    /// Call this before setting any pending state
    fn clear_pending_input_states(&mut self) {
        self.command_mode = false;
        self.search_mode = false;
        self.pending_char_op = None;
        self.pending_mark_op = None;
        self.pending_macro_op = None;
        // Clear register waiting state (Some('\0')) but preserve selected register
        if self.selected_register == Some('\0') {
            self.selected_register = None;
        }
    }

    /// Set last_key with timestamp for timeout tracking
    fn set_last_key(&mut self, key: impl Into<String>) {
        self.last_key = key.into();
        self.last_key_time = Some(Instant::now());
    }

    /// Clear last_key and its timestamp
    pub(super) fn clear_last_key(&mut self) {
        self.last_key.clear();
        self.last_key_time = None;
    }

    /// Cancel any pending operator in Neovim and clear local state
    /// Call this before executing local commands that would conflict with pending operators
    fn cancel_pending_operator(&mut self) {
        if !self.last_key.is_empty() {
            crate::verbose_print!(
                "[godot-neovim] Cancelling pending operator: '{}'",
                self.last_key
            );
            // Send Escape to cancel Neovim's pending operator via channel
            if let Some(ref neovim) = self.neovim {
                if let Ok(client) = neovim.try_lock() {
                    if !client.send_key_via_channel("<Esc>") {
                        crate::verbose_print!(
                            "[godot-neovim] Failed to send <Esc> for pending operator cancellation"
                        );
                    }
                } else {
                    crate::verbose_print!(
                        "[godot-neovim] Mutex busy, could not send <Esc> for pending operator cancellation"
                    );
                }
            }
            // Always clear local state even if Neovim Escape is queued
            self.clear_last_key();
        }
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

    #[allow(dead_code)]
    fn update_mode_display(&mut self, mode: &str) {
        self.update_mode_display_with_cursor(mode, None);
    }

    fn update_mode_display_with_cursor(&mut self, mode: &str, cursor: Option<(i64, i64)>) {
        // Clear version display flag (any operation returns to normal display)
        self.show_version = false;

        let Some(ref mut label) = self.mode_label else {
            return;
        };

        // Check if label is still valid (may have been freed when script was closed)
        if !label.is_instance_valid() {
            self.mode_label = None;
            return;
        }

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
            "n" => Color::from_rgb(0.0, 1.0, 0.5), // Green for normal
            "i" | "insert" => Color::from_rgb(0.4, 0.6, 1.0), // Blue for insert
            "R" | "replace" => Color::from_rgb(1.0, 0.3, 0.3), // Red for replace
            "v" | "V" | "\x16" | "^V" | "CTRL-V" | "visual" | "visual-line" | "visual-block" => {
                Color::from_rgb(1.0, 0.6, 0.2) // Orange for visual
            }
            "c" | "command" => Color::from_rgb(1.0, 1.0, 0.4), // Yellow for command
            _ => Color::from_rgb(1.0, 1.0, 1.0),               // White for unknown
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

    /// Update status label to show version
    pub(crate) fn update_version_display(&mut self) {
        let Some(ref mut label) = self.mode_label else {
            return;
        };

        // Check if label is still valid (may have been freed when script was closed)
        if !label.is_instance_valid() {
            self.mode_label = None;
            return;
        }

        let display_text = format!(" godot-neovim v{} ", VERSION);
        label.set_text(&display_text);
        // White color for version display
        label.add_theme_color_override("font_color", Color::from_rgb(1.0, 1.0, 1.0));
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

    fn handle_search_mode_input(&mut self, key_event: &Gd<godot::classes::InputEventKey>) {
        let keycode = key_event.get_keycode();

        if keycode == Key::ESCAPE {
            self.close_search_mode();
        } else if keycode == Key::ENTER {
            self.execute_search();
        } else if keycode == Key::BACKSPACE {
            // Remove last character (but keep the '/' or '?')
            if self.search_buffer.len() > 1 {
                self.search_buffer.pop();
                self.update_search_display();
            }
        } else {
            // Append character to search buffer
            let unicode = key_event.get_unicode();
            if unicode > 0 {
                if let Some(c) = char::from_u32(unicode) {
                    self.search_buffer.push(c);
                    self.update_search_display();
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

        // Ignore modifier-only key presses (SHIFT, CTRL, ALT, META keys themselves)
        // These are pressed before the actual character key and should not cancel the operation
        if matches!(
            keycode,
            Key::SHIFT | Key::CTRL | Key::ALT | Key::META | Key::CAPSLOCK | Key::NUMLOCK
        ) {
            // Don't consume the event, but don't cancel either - wait for actual character
            return false;
        }

        // Cancel on Escape or any modifier key combination (Ctrl+X, Alt+X, etc.)
        if keycode == Key::ESCAPE
            || key_event.is_ctrl_pressed()
            || key_event.is_alt_pressed()
            || key_event.is_meta_pressed()
        {
            self.pending_char_op = None;
            crate::verbose_print!(
                "[godot-neovim] Cancelled pending char op '{}' due to modifier/escape",
                op
            );
            // Don't consume the event - let it be processed normally
            return false;
        }

        // Get the character
        let unicode = key_event.get_unicode();
        if unicode > 0 {
            if let Some(c) = char::from_u32(unicode) {
                self.pending_char_op = None;
                // Build the key sequence for f/F/t/T
                let keys = match op {
                    'f' | 'F' | 't' | 'T' => Some(format!("{}{}", op, c)),
                    _ => None,
                };

                match op {
                    'f' => self.find_char_forward(c, false),
                    'F' => self.find_char_backward(c, false),
                    't' => self.find_char_forward(c, true),
                    'T' => self.find_char_backward(c, true),
                    'r' => self.replace_char(c),
                    _ => {}
                }

                // Send to Neovim and record to local macro buffer
                if let Some(keys) = keys {
                    self.send_keys(&keys);
                    // Record to local macro buffer (early return skips normal recording)
                    if self.recording_macro.is_some() && !self.playing_macro {
                        self.macro_buffer.push(keys);
                    }
                }
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return true;
            }
        }

        // Non-printable key pressed - cancel the pending operation
        self.pending_char_op = None;
        crate::verbose_print!(
            "[godot-neovim] Cancelled pending char op '{}' due to non-printable key",
            op
        );
        false
    }

    fn handle_pending_mark_op(&mut self, key_event: &Gd<godot::classes::InputEventKey>) -> bool {
        let Some(op) = self.pending_mark_op else {
            return false;
        };

        let keycode = key_event.get_keycode();

        // Ignore modifier-only key presses (SHIFT, CTRL, ALT, META keys themselves)
        if matches!(
            keycode,
            Key::SHIFT | Key::CTRL | Key::ALT | Key::META | Key::CAPSLOCK | Key::NUMLOCK
        ) {
            return false;
        }

        // Cancel on Escape or any modifier key combination
        if keycode == Key::ESCAPE
            || key_event.is_ctrl_pressed()
            || key_event.is_alt_pressed()
            || key_event.is_meta_pressed()
        {
            self.pending_mark_op = None;
            crate::verbose_print!(
                "[godot-neovim] Cancelled pending mark op '{}' due to modifier/escape",
                op
            );
            // Don't consume the event - let it be processed normally
            return false;
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
                // Non a-z character - cancel and let it be processed normally
                self.pending_mark_op = None;
                crate::verbose_print!(
                    "[godot-neovim] Cancelled pending mark op '{}' - invalid mark char '{}'",
                    op,
                    c
                );
                return false;
            }
        }

        // Non-printable key pressed - cancel the pending operation
        self.pending_mark_op = None;
        crate::verbose_print!(
            "[godot-neovim] Cancelled pending mark op '{}' due to non-printable key",
            op
        );
        false
    }

    fn handle_pending_macro_op(&mut self, key_event: &Gd<godot::classes::InputEventKey>) -> bool {
        let Some(op) = self.pending_macro_op else {
            return false;
        };

        let keycode = key_event.get_keycode();

        // Ignore modifier-only key presses (SHIFT, CTRL, ALT, META keys themselves)
        if matches!(
            keycode,
            Key::SHIFT | Key::CTRL | Key::ALT | Key::META | Key::CAPSLOCK | Key::NUMLOCK
        ) {
            return false;
        }

        // Cancel on Escape or any modifier key combination
        if keycode == Key::ESCAPE
            || key_event.is_ctrl_pressed()
            || key_event.is_alt_pressed()
            || key_event.is_meta_pressed()
        {
            self.pending_macro_op = None;
            crate::verbose_print!(
                "[godot-neovim] Cancelled pending macro op '{}' due to modifier/escape",
                op
            );
            // Don't consume the event - let it be processed normally
            return false;
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
                        } else {
                            crate::verbose_print!(
                                "[godot-neovim] Macro recording cancelled - invalid register '{}'",
                                c
                            );
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
                        } else {
                            crate::verbose_print!(
                                "[godot-neovim] Macro playback cancelled - invalid register '{}'",
                                c
                            );
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

        // Non-printable key pressed - cancel the pending operation
        self.pending_macro_op = None;
        crate::verbose_print!(
            "[godot-neovim] Cancelled pending macro op '{}' due to non-printable key",
            op
        );
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
                let is_valid_register =
                    c.is_ascii_lowercase() || c == '+' || c == '*' || c == '_' || c == '0';
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
        // Intercept Escape or Ctrl+[ to exit insert mode
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

        // Ctrl+B in insert mode: exit insert and enter visual block mode
        let is_ctrl_b = key_event.is_ctrl_pressed() && key_event.get_keycode() == Key::B;
        if is_ctrl_b {
            // First sync buffer and exit insert mode
            self.send_escape();
            // Then enter visual block mode
            let completed = self.send_keys("<C-v>");
            if completed {
                self.clear_last_key();
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Ctrl/Alt modified keys are sent to Neovim for Vim insert mode commands
        // (Ctrl+w, Ctrl+u, Ctrl+r, Ctrl+o, etc.)
        let ctrl = key_event.is_ctrl_pressed();
        let alt = key_event.is_alt_pressed();
        if ctrl || alt {
            let nvim_key = self.key_event_to_nvim_notation(key_event);
            if !nvim_key.is_empty() {
                self.send_keys(&nvim_key);
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
            }
        }

        // Normal character input: let Godot handle it (IME/autocomplete support)
    }

    fn handle_replace_mode_input(&mut self, key_event: &Gd<godot::classes::InputEventKey>) {
        // Intercept Escape or Ctrl+[ to exit replace mode
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

        // Ctrl/Alt modified keys are sent to Neovim
        let ctrl = key_event.is_ctrl_pressed();
        let alt = key_event.is_alt_pressed();
        if ctrl || alt {
            let nvim_key = self.key_event_to_nvim_notation(key_event);
            if !nvim_key.is_empty() {
                self.send_keys(&nvim_key);
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
            }
            return;
        }

        // Implement overwrite behavior for replace mode
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
    }

    fn handle_normal_mode_input(&mut self, key_event: &Gd<godot::classes::InputEventKey>) {
        let keycode = key_event.get_keycode();
        let unicode_char = char::from_u32(key_event.get_unicode());

        // Handle Ctrl+B: visual block in visual mode, page up in normal mode
        if key_event.is_ctrl_pressed() && keycode == Key::B {
            self.cancel_pending_operator();
            if Self::is_visual_mode(&self.current_mode) {
                // In visual mode: switch to visual block (Ctrl+V alternative since Godot intercepts it)
                let completed = self.send_keys("<C-v>");
                if completed {
                    self.clear_last_key();
                }
            } else {
                // In normal mode: page up - send to Neovim, viewport syncs via win_viewport
                // Set flag to correct cursor position after viewport sync
                // (ext_multigrid reports wrong viewport height at end of file)
                self.pending_page_up_correction = true;
                self.send_keys("<C-b>");
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

        // Handle Ctrl+F for page down - send to Neovim, viewport syncs via win_viewport
        if key_event.is_ctrl_pressed() && keycode == Key::F {
            self.cancel_pending_operator();
            self.send_keys("<C-f>");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+Y/Ctrl+E for viewport scrolling (cursor stays on same line)
        if key_event.is_ctrl_pressed() && (keycode == Key::Y || keycode == Key::E) {
            self.cancel_pending_operator();
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
        // Neovim Master: send to Neovim for proper jumplist support
        if key_event.is_ctrl_pressed() && keycode == Key::O {
            self.send_keys("<C-o>");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+I (Tab) for jump forward in jump list
        // Neovim Master: send to Neovim for proper jumplist support
        if key_event.is_ctrl_pressed() && keycode == Key::I {
            self.send_keys("<C-i>");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+G for file info
        if key_event.is_ctrl_pressed() && keycode == Key::G {
            self.cancel_pending_operator();
            self.show_file_info();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '/' for forward search mode
        if unicode_char == Some('/') && !key_event.is_ctrl_pressed() {
            self.open_search_mode(true);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '?' for backward search mode
        if unicode_char == Some('?') && !key_event.is_ctrl_pressed() {
            self.open_search_mode(false);
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

        // Handle '*' for search forward word under cursor (send to Neovim)
        if unicode_char == Some('*') {
            self.search_word("*");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '#' for search backward word under cursor (send to Neovim)
        if unicode_char == Some('#') {
            self.search_word("#");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'n' for repeat search forward (send to Neovim)
        if keycode == Key::N && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.search_next(true);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'N' for repeat search backward (send to Neovim)
        if keycode == Key::N && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.search_next(false);
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'u' for undo - send to Neovim (Neovim Master design)
        // (but not after 'g' - that's 'gu' for lowercase)
        if keycode == Key::U
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            self.send_keys("u");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'Ctrl+R' for redo - send to Neovim (Neovim Master design)
        if keycode == Key::R && key_event.is_ctrl_pressed() {
            self.send_keys("<C-r>");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'f' for find char forward (but not after 'g' - that's 'gf' for go to file)
        if keycode == Key::F
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            self.clear_pending_input_states();
            self.pending_char_op = Some('f');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'F' for find char backward
        if keycode == Key::F && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
            self.pending_char_op = Some('F');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 't' for till char forward (but not after 'g' - that's gt for tab navigation,
        // and not after 'z' - that's zt for scroll cursor to top)
        if keycode == Key::T
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
            && self.last_key != "z"
        {
            self.clear_pending_input_states();
            self.pending_char_op = Some('t');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'T' for till char backward (but not after 'g' - that's gT for tab navigation)
        if keycode == Key::T
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            self.clear_pending_input_states();
            self.pending_char_op = Some('T');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ';' for repeat find char same direction
        if keycode == Key::SEMICOLON && !key_event.is_shift_pressed() {
            self.repeat_find_char(true);
            self.send_keys(";");
            // Record to local macro buffer (early return skips normal recording)
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push(";".to_string());
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ',' for repeat find char opposite direction
        if keycode == Key::COMMA && !key_event.is_shift_pressed() {
            self.repeat_find_char(false);
            self.send_keys(",");
            // Record to local macro buffer (early return skips normal recording)
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push(",".to_string());
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '%' for matching bracket
        if unicode_char == Some('%') {
            self.jump_to_matching_bracket();
            self.send_keys("%");
            // Record to local macro buffer (early return skips normal recording)
            if self.recording_macro.is_some() && !self.playing_macro {
                self.macro_buffer.push("%".to_string());
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle count prefix (1-9, or 0 if count_buffer not empty)
        // This tracks the count locally while also sending to Neovim
        if let Some(c) = unicode_char {
            if c.is_ascii_digit() && (c != '0' || !self.count_buffer.is_empty()) {
                self.count_buffer.push(c);
                self.send_keys(&c.to_string());
                // Reset timeout to prevent <Esc> being sent during count input
                self.last_key_time = Some(std::time::Instant::now());
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
        }

        // Handle '0' for go to start of line (only when not part of a count)
        // Skip if last_key is "g" (g0 is handled separately for display line)
        if unicode_char == Some('0') && !key_event.is_ctrl_pressed() && self.last_key != "g" {
            self.move_to_line_start();
            self.send_keys("0"); // Also send to Neovim
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '^' for go to first non-blank
        // Skip if last_key is "g" (g^ is handled separately for display line)
        if unicode_char == Some('^') && self.last_key != "g" {
            self.move_to_first_non_blank();
            self.send_keys("^"); // Also send to Neovim
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '$' for go to end of line
        // Skip if last_key is "g" (g$ is handled separately for display line)
        if unicode_char == Some('$') && self.last_key != "g" {
            self.move_to_line_end();
            self.send_keys("$"); // Also send to Neovim
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '{' for previous paragraph (send to Neovim for proper cursor positioning)
        // Skip if last_key is '[' or ']' - these are [{ / ]{ commands handled later
        if unicode_char == Some('{') && self.last_key != "[" && self.last_key != "]" {
            self.send_keys("{");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '}' for next paragraph (send to Neovim for proper cursor positioning)
        // Skip if last_key is '[' or ']' - these are [} / ]} commands handled later
        if unicode_char == Some('}') && self.last_key != "[" && self.last_key != "]" {
            self.send_keys("}");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'x' for delete char under cursor (but not after 'g' - that's 'gx' for open URL)
        // Neovim Master: send to Neovim only, reflect via nvim_buf_lines_event
        if keycode == Key::X
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            self.send_keys("x");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'X' for delete char before cursor
        // Neovim Master: send to Neovim only
        if keycode == Key::X && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys("X");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'Y' for yank to end of line
        // Neovim Master: send to Neovim only
        if keycode == Key::Y && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys("Y");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'D' for delete to end of line
        // Neovim Master: send to Neovim only, reflect via nvim_buf_lines_event
        if keycode == Key::D && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys("D");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'C' for change to end of line
        // Neovim Master: send to Neovim only
        if keycode == Key::C && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys("C");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 's' for substitute char (delete char and enter insert mode)
        // Neovim Master: send to Neovim only
        if keycode == Key::S && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys("s");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'S' for substitute line (delete line content and enter insert mode)
        // Neovim Master: send to Neovim only
        if keycode == Key::S && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.send_keys("S");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'cc' for substitute line (same as S)
        // Neovim Master: send to Neovim only
        if keycode == Key::C && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.last_key == "c" {
                self.send_keys("c"); // Send second 'c' to complete 'cc'
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            } else {
                self.set_last_key("c");
                self.send_keys("c"); // Send first 'c' to Neovim
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
        }

        // Handle 'r' for replace char
        if keycode == Key::R && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
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
            self.clear_pending_input_states();
            self.pending_mark_op = Some('m');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '\'' (single quote) for jump to mark line
        if unicode_char == Some('\'') && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
            self.pending_mark_op = Some('\'');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '`' (backtick) for jump to mark position
        if unicode_char == Some('`') && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
            self.pending_mark_op = Some('`');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'q' for macro recording (start/stop) - but not after 'g' (that's gq for format)
        if keycode == Key::Q
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            if self.recording_macro.is_some() {
                // Stop recording
                self.stop_macro_recording();
            } else {
                // Wait for register character
                self.clear_pending_input_states();
                self.pending_macro_op = Some('q');
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '@' for macro playback
        if unicode_char == Some('@') && !key_event.is_ctrl_pressed() {
            self.clear_pending_input_states();
            self.pending_macro_op = Some('@');
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '"' for register selection
        if unicode_char == Some('"') && !key_event.is_ctrl_pressed() {
            // Use '\0' as marker for "waiting for register char"
            self.clear_pending_input_states();
            // Clear last_key to prevent timeout from clearing selected_register
            self.clear_last_key();
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
                self.clear_last_key();
            } else {
                self.set_last_key(">");
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        if unicode_char == Some('<') {
            if self.last_key == "<" {
                self.unindent_line();
                self.clear_last_key();
            } else {
                self.set_last_key("<");
            }
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'g' prefix - don't send to Neovim yet, wait for next key
        // (like '[' and ']' prefixes)
        // Note: Skip if last_key is already "g" to allow 'gg' to be processed
        if unicode_char == Some('g')
            && !key_event.is_ctrl_pressed()
            && !key_event.is_shift_pressed()
            && self.last_key != "g"
        {
            self.set_last_key("g");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '[' prefix - don't send to Neovim yet, wait for next key
        // Use keycode for keyboard layout independence (JP keyboard may have different unicode)
        // Skip if last_key is already '[' or ']' (to allow [[, ]], [], ][ sequences)
        if keycode == Key::BRACKETLEFT
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "["
            && self.last_key != "]"
        {
            self.set_last_key("[");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle ']' prefix - don't send to Neovim yet, wait for next key
        // Use keycode for keyboard layout independence (JP keyboard may have different unicode)
        // Skip if last_key is already '[' or ']' (to allow [[, ]], [], ][ sequences)
        if keycode == Key::BRACKETRIGHT
            && !key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "["
            && self.last_key != "]"
        {
            self.set_last_key("]");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle p after [ or ]
        if keycode == Key::P && !key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            if self.last_key == "[" {
                self.paste_with_indent_before();
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            } else if self.last_key == "]" {
                self.paste_with_indent_after();
                self.clear_last_key();
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

        // Handle 'K' for documentation lookup
        if keycode == Key::K && key_event.is_shift_pressed() && !key_event.is_ctrl_pressed() {
            self.open_documentation();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle '[' commands
        // Use keycode for keyboard layout independence (JP keyboard support)
        if self.last_key == "[" {
            // [[ - jump to previous '{' at start of line (send to Neovim)
            if keycode == Key::BRACKETLEFT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.send_keys("[[");
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
            // [] - jump to previous '}' at start of line (send to Neovim)
            if keycode == Key::BRACKETRIGHT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.send_keys("[]");
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
            match unicode_char {
                Some('{') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("[{");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some('(') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("[(");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some('m') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("[m");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some('\0') | None => {
                    // Modifier-only key (SHIFT, etc.) or NUL char - don't clear last_key
                }
                _ => {
                    // Not a recognized [ command, clear and continue
                    self.clear_last_key();
                }
            }
        }

        // Handle ']' commands
        // Use keycode for keyboard layout independence (JP keyboard support)
        if self.last_key == "]" {
            // ]] - jump to next '{' at start of line (send to Neovim)
            if keycode == Key::BRACKETRIGHT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.send_keys("]]");
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
            // ][ - jump to next '}' at start of line (send to Neovim)
            if keycode == Key::BRACKETLEFT
                && !key_event.is_shift_pressed()
                && !key_event.is_ctrl_pressed()
            {
                self.send_keys("][");
                self.clear_last_key();
                if let Some(mut viewport) = self.base().get_viewport() {
                    viewport.set_input_as_handled();
                }
                return;
            }
            match unicode_char {
                Some('}') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("]}");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some(')') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("])");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some('m') => {
                    // Neovim Master: send to Neovim for proper jumplist support
                    self.send_keys("]m");
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
                Some('\0') | None => {
                    // Modifier-only key (SHIFT, etc.) or NUL char - don't clear last_key
                }
                _ => {
                    // Not a recognized ] command, clear and continue
                    self.clear_last_key();
                }
            }
        }

        // Handle gqq (format current line)
        if self.last_key == "gq" && keycode == Key::Q && !key_event.is_shift_pressed() {
            self.format_current_line();
            self.clear_last_key();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle 'J' for join lines - send to Neovim (Neovim Master design)
        // Neovim will process the join and send buffer changes via nvim_buf_lines_event
        // Note: Skip if last_key is "g" to allow 'gJ' to be processed in g-prefix block
        if keycode == Key::J
            && key_event.is_shift_pressed()
            && !key_event.is_ctrl_pressed()
            && self.last_key != "g"
        {
            self.send_keys("J");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+D for half page down - send to Neovim for viewport sync
        if key_event.is_ctrl_pressed() && keycode == Key::D {
            self.cancel_pending_operator();
            self.send_keys("<C-d>");
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Handle Ctrl+U for half page up - send to Neovim for viewport sync
        if key_event.is_ctrl_pressed() && keycode == Key::U {
            self.cancel_pending_operator();
            self.send_keys("<C-u>");
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
            // H/M/L are valid motions in all contexts:
            // - Normal mode: move cursor
            // - Visual mode: extend selection
            // - Operator-pending mode (d, c, y + H/M/L): complete the operation
            // Do NOT cancel pending operator - let Neovim handle it
            // Shift+h/m/l = H/M/L (uppercase) - send to Neovim for viewport-aware handling
            match keycode {
                Key::H => {
                    self.send_keys("H");
                }
                Key::M => {
                    self.send_keys("M");
                }
                Key::L => {
                    self.send_keys("L");
                }
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
                self.clear_last_key();
            } else {
                // First Z - wait for next key
                self.set_last_key("Z");
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
            self.clear_last_key();
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }

        // Clear Z prefix if another key is pressed (not Z or Q)
        if self.last_key == "Z" && keycode != Key::Z && keycode != Key::Q {
            self.clear_last_key();
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
                        self.clear_last_key();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    } else {
                        // First y - wait for second
                        self.set_last_key("y");
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
                        self.clear_last_key();
                        if let Some(mut viewport) = self.base().get_viewport() {
                            viewport.set_input_as_handled();
                        }
                        return;
                    } else {
                        // First d - wait for second
                        self.set_last_key("d");
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
            // Intercept g-prefix commands
            // Note: 'g' is NOT sent to Neovim when typed - we wait for the second key
            // and send the full command (like 'ge', 'gj', etc.) or 'g' + second key for unhandled commands
            if self.last_key == "g" {
                let handled = match keys.as_str() {
                    "x" => {
                        // gx - open URL under cursor (Godot-specific, don't send to Neovim)
                        let saved_cursor = self
                            .current_editor
                            .as_ref()
                            .map(|e| (e.get_caret_line(), e.get_caret_column()));
                        if let (Some((line, col)), Some(ref mut editor)) =
                            (saved_cursor, self.current_editor.as_mut())
                        {
                            editor.set_caret_line(line);
                            editor.set_caret_column(col);
                        }
                        self.open_url_under_cursor();
                        true
                    }
                    "f" => {
                        // gf - go to file under cursor (Godot-specific, don't send to Neovim)
                        self.go_to_file_under_cursor();
                        true
                    }
                    "d" => {
                        // gd - go to definition (Godot LSP, don't send to Neovim)
                        let saved_cursor = self
                            .current_editor
                            .as_ref()
                            .map(|editor| (editor.get_caret_line(), editor.get_caret_column()));
                        if let (Some((line, col)), Some(ref mut editor)) =
                            (saved_cursor, self.current_editor.as_mut())
                        {
                            editor.set_caret_line(line);
                            editor.set_caret_column(col);
                        }
                        self.add_to_jump_list();
                        self.go_to_definition_lsp();
                        true
                    }
                    "I" => {
                        // gI - insert at column 0 (Neovim Master design)
                        // insert_at_column_zero() sends gI to Neovim
                        self.insert_at_column_zero();
                        true
                    }
                    "i" => {
                        // gi - insert at last insert position (Neovim Master design)
                        // insert_at_last_position() sends gi to Neovim
                        self.insert_at_last_position();
                        true
                    }
                    "a" => {
                        // ga - show character info under cursor (Godot-specific display)
                        self.show_char_info();
                        true
                    }
                    "&" => {
                        // g& - repeat last substitution on entire buffer
                        // Note: repeat_substitute() handles buffer sync internally
                        self.repeat_substitute();
                        true
                    }
                    "J" => {
                        // gJ - join lines without space
                        // Use Lua function from init.lua to handle comments option
                        self.send_keys("<Cmd>lua require('godot_neovim').join_no_space()<CR>");
                        true
                    }
                    "p" => {
                        // gp - paste and move cursor after pasted text
                        // Send to Neovim to preserve undo history and use Neovim registers
                        self.send_keys("gp");
                        true
                    }
                    "P" => {
                        // gP - paste before and move cursor after pasted text
                        // Send to Neovim to preserve undo history and use Neovim registers
                        self.send_keys("gP");
                        true
                    }
                    "e" => {
                        // ge - move to end of previous word
                        self.move_to_word_end_backward();
                        self.send_keys("ge"); // Sync to Neovim
                        true
                    }
                    "j" => {
                        // gj - move down by display line (wrapped line)
                        // Local handling uses Godot's wrap info, cursor synced internally
                        self.move_display_line_down();
                        true
                    }
                    "k" => {
                        // gk - move up by display line (wrapped line)
                        // Local handling uses Godot's wrap info, cursor synced internally
                        self.move_display_line_up();
                        true
                    }
                    "t" => {
                        // gt - go to next tab (Godot-specific, don't send to Neovim)
                        self.next_script_tab();
                        true
                    }
                    "T" => {
                        // gT - go to previous tab (Godot-specific, don't send to Neovim)
                        self.prev_script_tab();
                        true
                    }
                    "v" => {
                        // gv - enter visual block mode (alternative to Ctrl+V)
                        self.send_keys("<C-v>");
                        true
                    }
                    "0" => {
                        // g0 - move to start of display line (wrapped line)
                        // Local handling uses Godot's wrap info, cursor synced internally
                        self.move_to_display_line_start();
                        true
                    }
                    "$" => {
                        // g$ - move to end of display line (wrapped line)
                        // Local handling uses Godot's wrap info, cursor synced internally
                        self.move_to_display_line_end();
                        true
                    }
                    "^" => {
                        // g^ - move to first non-blank of display line
                        // Local handling uses Godot's wrap info, cursor synced internally
                        self.move_to_display_line_first_non_blank();
                        true
                    }
                    _ => {
                        // Unhandled g-command: send 'g' + second key to Neovim
                        // (e.g., gg, g_, etc.)
                        self.send_keys(&format!("g{}", keys));
                        true
                    }
                };

                if handled {
                    self.clear_last_key();
                    if let Some(mut viewport) = self.base().get_viewport() {
                        viewport.set_input_as_handled();
                    }
                    return;
                }
            }

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

            // Handle gq (format operator) - needs to wait for motion
            if completed && self.last_key == "g" && keys == "q" {
                self.set_last_key("gq");
                // Don't return - let normal key handling continue for motion
            }

            // Track last key for sequence detection, unless:
            // - scroll command was handled, or
            // - we entered insert/replace/visual mode (no sequence expected in those modes)
            if !scroll_handled
                && !self.is_insert_mode()
                && !self.is_replace_mode()
                && !self.is_in_visual_mode()
            {
                self.set_last_key(keys);
            }

            // Consume the event to prevent Godot's default handling
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
        }
    }
}
