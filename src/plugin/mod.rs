//! Godot Neovim Plugin - Main module

/// Plugin version: Cargo.toml version for release, build datetime for debug
const VERSION: &str = env!("BUILD_VERSION");

mod commands;
mod editing;
mod editor;
pub(crate) mod filetype;
mod input;
mod keys;
mod macros;
mod marks;
mod motions;
mod neovim;
mod recovery;
mod registers;
mod search;
mod state;
mod ui;
mod visual;

use crate::lsp::GodotLspClient;
use crate::neovim::NeovimClient;
use crate::settings;
use crate::sync::SyncManager;
use godot::classes::{
    CodeEdit, ConfirmationDialog, EditorInterface, EditorPlugin, IEditorPlugin, Label,
    ProjectSettings,
};
use godot::global::Key;
use godot::prelude::*;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

/// Type of editor currently active
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum EditorType {
    /// ScriptEditor (GDScript, C#, etc.)
    #[default]
    Script,
    /// ShaderEditor (gdshader)
    Shader,
    /// TextFile editor (txt, md, json, etc.)
    TextFile,
    /// Unknown or no editor
    Unknown,
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
    /// Neovim client for ScriptEditor
    #[init(val = None)]
    script_neovim: Option<Mutex<NeovimClient>>,
    /// Neovim client for ShaderEditor (separate instance)
    #[init(val = None)]
    shader_neovim: Option<Mutex<NeovimClient>>,
    #[init(val = None)]
    mode_label: Option<Gd<Label>>,
    /// Separate mode label for ShaderEditor (independent from ScriptEditor)
    #[init(val = None)]
    shader_mode_label: Option<Gd<Label>>,
    #[init(val = None)]
    current_editor: Option<Gd<CodeEdit>>,
    /// Type of the current editor (Script, Shader, Unknown)
    #[init(val = EditorType::Unknown)]
    current_editor_type: EditorType,
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
    /// Script switch ID for cancellation (incremented on each script change)
    /// Used to detect and skip stale deferred operations when rapid switching occurs
    #[init(val = 0)]
    script_switch_id: u64,
    /// Pending script switch ID (the ID when deferred call was initiated)
    /// If this doesn't match script_switch_id, the operation is stale
    #[init(val = 0)]
    pending_switch_id: u64,
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
    /// Flag to grab focus after script change (set by on_script_close)
    #[init(val = false)]
    focus_after_script_change: bool,
    /// Flag to grab focus on ShaderEditor after closing a shader tab
    #[init(val = false)]
    pub(super) focus_shader_after_close: bool,
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
    /// Flag indicating mouse is being dragged (for visual mode sync on release)
    #[init(val = false)]
    mouse_dragging: bool,
    /// Flag to skip Neovim's visual selection update after mouse drag sync
    /// Set when syncing mouse selection to Neovim, cleared after sync completes
    #[init(val = false)]
    mouse_selection_syncing: bool,
    /// Visual mode subtype: 'v' for char, 'V' for line, '\x16' for block
    /// Neovim returns "visual" for all visual modes, so we track the key pressed
    #[init(val = 'v')]
    visual_mode_type: char,
    /// Timestamps of recent timeout errors for recovery detection
    #[init(val = Vec::new())]
    timeout_timestamps: Vec<Instant>,
    /// Recovery dialog is currently shown
    #[init(val = false)]
    recovery_dialog_open: bool,
    /// Recovery dialog reference
    #[init(val = None)]
    recovery_dialog: Option<Gd<ConfirmationDialog>>,
    /// Timestamp of last key sent to Neovim (for detecting no-response)
    #[init(val = None)]
    last_key_send_time: Option<Instant>,
    /// Number of pending keys without response
    #[init(val = 0)]
    pending_key_count: u32,
}

#[godot_api]
impl IEditorPlugin for GodotNeovimPlugin {
    fn enter_tree(&mut self) {
        crate::verbose_print!("[godot-neovim] v{} loaded", VERSION);

        // Add to group for GDScript discovery
        self.base_mut().add_to_group("godot_neovim");

        // Initialize settings first
        settings::initialize_settings();

        // Validate Neovim path
        let validation = settings::validate_current_path();
        if !validation.is_valid() {
            godot_warn!("[godot-neovim] Neovim validation failed, plugin may not work correctly");
        }

        // Get addons path for Lua plugin
        let addons_path = ProjectSettings::singleton()
            .globalize_path("res://addons/godot-neovim")
            .to_string();

        // Initialize Neovim client for ScriptEditor
        match NeovimClient::new() {
            Ok(mut client) => {
                if let Err(e) = client.start(Some(&addons_path)) {
                    godot_error!(
                        "[godot-neovim] Failed to start Neovim for ScriptEditor: {}",
                        e
                    );
                    return;
                }
                self.script_neovim = Some(Mutex::new(client));
                crate::verbose_print!("[godot-neovim] ScriptEditor Neovim initialized");
            }
            Err(e) => {
                godot_error!(
                    "[godot-neovim] Failed to create Neovim client for ScriptEditor: {}",
                    e
                );
                return;
            }
        }

        // Initialize Neovim client for ShaderEditor (separate instance)
        match NeovimClient::new() {
            Ok(mut client) => {
                if let Err(e) = client.start(Some(&addons_path)) {
                    godot_error!(
                        "[godot-neovim] Failed to start Neovim for ShaderEditor: {}",
                        e
                    );
                    // Continue with ScriptEditor only
                } else {
                    self.shader_neovim = Some(Mutex::new(client));
                    crate::verbose_print!("[godot-neovim] ShaderEditor Neovim initialized");
                }
            }
            Err(e) => {
                godot_warn!(
                    "[godot-neovim] Failed to create Neovim client for ShaderEditor: {}",
                    e
                );
                // Continue with ScriptEditor only
            }
        }

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
            crate::verbose_print!("[godot-neovim] LSP client initialized (use_thread=true)");
        } else {
            crate::verbose_print!("[godot-neovim] LSP disabled (use_thread=false)");
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

        // Cleanup mode labels (check if still valid before freeing)
        if let Some(mut label) = self.mode_label.take() {
            if label.is_instance_valid() {
                label.queue_free();
            }
        }
        if let Some(mut label) = self.shader_mode_label.take() {
            if label.is_instance_valid() {
                label.queue_free();
            }
        }

        // Disconnect from gui_input signal
        self.disconnect_gui_input_signal();

        // Clear current editor reference
        self.current_editor = None;

        // Disconnect and clear LSP client
        if let Some(ref lsp) = self.godot_lsp {
            lsp.disconnect();
        }
        self.godot_lsp = None;

        // Neovim clients will be stopped when dropped (with timeout)
        self.script_neovim = None;
        self.shader_neovim = None;

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
                    // These are always from ScriptEditor since on_script_close is ScriptEditor-only
                    let pending = std::mem::take(&mut self.pending_buffer_deletions);
                    for path in pending {
                        self.delete_neovim_buffer(&path, EditorType::Script);
                    }
                }
            }
        }

        // Handle deferred shader focus after close
        // ShaderEditor doesn't have on_script_close signal, so we handle focus here
        if self.focus_shader_after_close {
            self.focus_shader_after_close = false;
            self.focus_shader_editor_code_edit();
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
                        if let Some(neovim) = self.get_current_neovim() {
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

        // Handle mouse click events - Godot controls selection, sync to Neovim on release
        if let Ok(mouse_event) = event
            .clone()
            .try_cast::<godot::classes::InputEventMouseButton>()
        {
            // Only handle left mouse button when editor has focus
            if mouse_event.get_button_index() == godot::global::MouseButton::LEFT
                && self.editor_has_focus()
            {
                if mouse_event.is_pressed() {
                    // Start tracking drag
                    self.mouse_dragging = true;
                    // Reset mouse selection sync flag (new drag/click started)
                    self.mouse_selection_syncing = false;

                    // Enable selecting - let Godot handle selection natively
                    if let Some(ref mut editor) = self.current_editor {
                        editor.set_selecting_enabled(true);
                    }
                } else if self.mouse_dragging {
                    // Mouse release after drag/click - sync to Neovim
                    self.mouse_dragging = false;

                    // Use deferred call to handle sync after Godot finalizes selection
                    self.base_mut()
                        .call_deferred("sync_mouse_selection_to_neovim", &[]);
                }
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
        if self.get_current_neovim().is_none() {
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

        // Clear user_cursor_sync flag to allow viewport sync from Neovim
        // This flag might be set from previous mouse interactions
        self.user_cursor_sync = false;

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
    /// Signal emitted when a key is sent to Neovim
    /// Can be used by GDScript to implement custom key sequence display
    #[signal]
    fn key_sent(key: GString);

    #[func]
    fn on_editor_resized(&mut self) {
        // Resize Neovim UI to match new editor size
        let Some(ref editor) = self.current_editor else {
            return;
        };

        // Safety check: editor might be freed during close operations
        // even though the signal is connected (timing issue with call_deferred)
        if !editor.is_instance_valid() {
            crate::verbose_print!(
                "[godot-neovim] on_editor_resized: editor is no longer valid, skipping"
            );
            return;
        }

        let visible_lines = editor.get_visible_line_count();
        if visible_lines != self.last_visible_lines && visible_lines > 0 {
            self.last_visible_lines = visible_lines;

            // Clear user_cursor_sync flag since resize might trigger caret_changed
            // but we still want to apply viewport changes from Neovim after resize
            self.user_cursor_sync = false;

            let Some(neovim) = self.get_current_neovim() else {
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

        // Skip during mouse drag - will sync on release
        if self.mouse_dragging {
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

        // Set flag to skip viewport sync from Neovim
        // This prevents Neovim from overriding user's scroll position when clicking
        self.user_cursor_sync = true;

        // Update last_synced_cursor and sync to Neovim
        self.last_synced_cursor = (line as i64, col as i64);
        self.sync_cursor_to_neovim();
    }

    /// Sync mouse selection to Neovim on mouse release
    /// If there's a selection (drag), enter visual mode and sync selection range
    /// If no selection (click), just sync cursor position
    #[func]
    fn sync_mouse_selection_to_neovim(&mut self) {
        // Clear command-line/search mode on mouse click/drag
        // This ensures the buffer is cleared when re-entering these modes
        if self.command_mode {
            self.close_command_line();
        }
        if self.search_mode {
            self.close_search_mode();
        }

        let in_visual_mode = self.is_in_visual_mode();

        // Get selection info from editor first to avoid borrow conflicts
        let selection_info = {
            let Some(ref editor) = self.current_editor else {
                return;
            };

            if editor.has_selection() {
                Some((
                    editor.get_selection_from_line(),
                    editor.get_selection_from_column(),
                    editor.get_selection_to_line(),
                    editor.get_selection_to_column(),
                ))
            } else {
                None
            }
        };

        let cursor_pos = {
            let Some(ref editor) = self.current_editor else {
                return;
            };
            (editor.get_caret_line(), editor.get_caret_column())
        };

        // Set flag to skip viewport sync from Neovim
        self.user_cursor_sync = true;

        if let Some((from_line, from_col, to_line, to_col)) = selection_info {
            // Drag occurred - sync selection to Neovim as visual mode
            crate::verbose_print!(
                "[godot-neovim] Mouse drag selection: ({}, {}) -> ({}, {})",
                from_line + 1,
                from_col,
                to_line + 1,
                to_col
            );

            // Clamp line numbers to Neovim buffer bounds
            // Godot CodeEdit may have extra empty line after last line
            let nvim_line_count = self.sync_manager.get_line_count();
            if nvim_line_count <= 0 {
                return;
            }
            let max_line = nvim_line_count - 1;
            let safe_from_line = from_line.min(max_line).max(0);
            let safe_to_line = to_line.min(max_line).max(0);

            // Set flag to skip Neovim's visual selection update
            self.mouse_selection_syncing = true;

            // Update last synced cursor to selection end
            self.last_synced_cursor = (safe_to_line as i64, to_col as i64);

            // Use Lua function to atomically set visual selection
            // This ensures ordering: move to start -> enter visual mode -> move to end
            if let Some(neovim) = self.get_current_neovim() {
                if let Ok(client) = neovim.try_lock() {
                    // Lua function expects 1-indexed line numbers
                    match client.set_visual_selection(
                        (safe_from_line + 1) as i64,
                        from_col as i64,
                        (safe_to_line + 1) as i64,
                        to_col as i64,
                    ) {
                        Ok(mode) => {
                            crate::verbose_print!(
                                "[godot-neovim] Visual selection set via Lua, mode: {}",
                                mode
                            );
                        }
                        Err(e) => {
                            crate::verbose_print!(
                                "[godot-neovim] Failed to set visual selection: {}",
                                e
                            );
                        }
                    }
                }
            }

            // Re-apply Godot selection (Neovim response may overwrite it)
            if let Some(ref mut ed) = self.current_editor {
                ed.select(from_line, from_col, to_line, to_col);
            }
        } else {
            // Simple click - just sync cursor position
            let (line, col) = cursor_pos;

            crate::verbose_print!("[godot-neovim] Mouse click at ({}, {})", line + 1, col);

            // If in visual mode, exit first
            if in_visual_mode {
                self.send_keys("<Esc>");
            }

            // Sync cursor position
            self.last_synced_cursor = (line as i64, col as i64);
            self.sync_cursor_to_neovim();
        }
    }

    #[func]
    fn on_settings_changed(&mut self) {
        let editor = EditorInterface::singleton();
        if let Some(editor_settings) = editor.get_editor_settings() {
            settings::on_settings_changed(&editor_settings);
        }

        // Sync indent settings to Neovim when editor settings change
        self.sync_indent_settings_to_neovim();
    }

    /// Sync current editor's indent settings to Neovim
    /// Reads directly from EditorSettings to ensure we get the latest values
    /// (CodeEdit may not have applied the new settings yet when settings_changed fires)
    fn sync_indent_settings_to_neovim(&mut self) {
        // Read directly from EditorSettings instead of CodeEdit
        // CodeEdit may have stale values when settings_changed signal fires
        let editor_interface = EditorInterface::singleton();
        let Some(editor_settings) = editor_interface.get_editor_settings() else {
            return;
        };

        // text_editor/behavior/indent/type: 0 = tabs, 1 = spaces
        let indent_type = editor_settings
            .get_setting("text_editor/behavior/indent/type")
            .to::<i32>();
        let use_spaces = indent_type == 1;

        let indent_size = editor_settings
            .get_setting("text_editor/behavior/indent/size")
            .to::<i32>();

        crate::verbose_print!(
            "[godot-neovim] Syncing indent settings: spaces={}, size={}",
            use_spaces,
            indent_size
        );

        // Sync to both Neovim instances
        if let Some(ref neovim) = self.script_neovim {
            if let Ok(client) = neovim.try_lock() {
                if let Err(e) = client.set_indent_options(use_spaces, indent_size) {
                    crate::verbose_print!(
                        "[godot-neovim] Failed to sync indent to ScriptEditor Neovim: {}",
                        e
                    );
                }
            }
        }
        if let Some(ref neovim) = self.shader_neovim {
            if let Ok(client) = neovim.try_lock() {
                if let Err(e) = client.set_indent_options(use_spaces, indent_size) {
                    crate::verbose_print!(
                        "[godot-neovim] Failed to sync indent to ShaderEditor Neovim: {}",
                        e
                    );
                }
            }
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
                    let char_col = editor.get_caret_column();
                    // Convert character column to byte column for Neovim
                    let line_text = editor.get_line(editor.get_caret_line()).to_string();
                    let byte_col = Self::char_col_to_byte_col(&line_text, char_col) as i64;
                    // Use script_neovim directly since on_script_changed is from ScriptEditor
                    if let Some(ref neovim) = self.script_neovim {
                        if let Ok(client) = neovim.try_lock() {
                            let _ = client.set_cursor(line, byte_col);
                            crate::verbose_print!(
                                "[godot-neovim] Synced cursor to Neovim for {}: ({}, {}) (char_col={})",
                                self.current_script_path,
                                line,
                                byte_col,
                                char_col
                            );
                        }
                    }
                }
            }
        }

        // Handle null script (e.g., when all scripts are closed or TextFile is opened)
        // TextFile resources are not Script, so they appear as null here
        let script_path = match script {
            Some(s) => {
                let path = s.get_path().to_string();
                crate::verbose_print!("[godot-neovim] on_script_changed: {}", path);
                if path.is_empty() {
                    None
                } else {
                    Some(path)
                }
            }
            None => {
                crate::verbose_print!(
                    "[godot-neovim] on_script_changed: null script (possibly TextFile or all closed)"
                );
                // Don't clear references immediately - might be TextFile
                // The deferred handler will try to get path from ItemList
                None
            }
        };

        // Store the expected script path for verification in deferred handler
        // For TextFile resources, this will be None and we'll use ItemList fallback
        self.expected_script_path = script_path;
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
        // on_script_close is only connected to ScriptEditor
        self.delete_neovim_buffer(&path, EditorType::Script);

        // Set flag to grab focus after script change processing completes
        // This ensures focus is set after the new CodeEdit is visible
        self.focus_after_script_change = true;
    }

    #[func]
    fn handle_script_changed_deferred(&mut self) {
        // Check if this operation is stale (a newer switch was initiated)
        // This prevents processing outdated switches during rapid tab changes
        if self.pending_switch_id != self.script_switch_id {
            crate::verbose_print!(
                "[godot-neovim] Script change cancelled (stale: pending={}, current={})",
                self.pending_switch_id,
                self.script_switch_id
            );
            return;
        }

        crate::verbose_print!("[godot-neovim] Script changed (deferred processing)");

        self.find_current_code_edit();

        // For ShaderEditor, skip ScriptEditor-based verification since it doesn't apply
        // ShaderEditor doesn't use on_script_changed signal, so path is already set in find_current_code_edit
        if self.current_editor_type == EditorType::Shader {
            crate::verbose_print!(
                "[godot-neovim] ShaderEditor detected, using path: '{}'",
                self.current_script_path
            );
            // Clear expected path and proceed to buffer sync
            self.expected_script_path = None;
        } else {
            // Verify we're on the expected script before syncing (ScriptEditor only)
            let editor = EditorInterface::singleton();
            let Some(mut script_editor) = editor.get_script_editor() else {
                crate::verbose_print!("[godot-neovim] No script editor found");
                return;
            };

            // Get the current script path from ScriptEditor (source of truth)
            // Try multiple methods:
            // 1. get_current_script() - works for Script resources
            // 2. expected_script_path from on_script_changed signal
            // 3. Tab tooltip fallback for TextFile resources
            let current_script_path = script_editor
                .get_current_script()
                .map(|s| s.get_path().to_string())
                .filter(|p| !p.is_empty())
                .or_else(|| {
                    // Fallback 1: use expected_script_path from signal
                    // This is useful when get_current_script() returns null
                    self.expected_script_path.clone()
                })
                .or_else(|| {
                    // Fallback 2: try to get path from ScriptEditor's tab
                    self.get_script_editor_current_tab_path(&script_editor)
                })
                .unwrap_or_default();

            crate::verbose_print!(
                "[godot-neovim] Current script: '{}', Expected: '{}'",
                current_script_path,
                self.expected_script_path.as_deref().unwrap_or("(none)")
            );

            // If no path found from any source, all scripts are closed
            if current_script_path.is_empty() {
                crate::verbose_print!(
                    "[godot-neovim] No script path found (all scripts closed), clearing references"
                );
                self.current_editor = None;
                self.mode_label = None;
                self.current_script_path.clear();
                self.expected_script_path = None;
                return;
            }

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

            // Update current script path for LSP (ScriptEditor only)
            self.current_script_path = current_script_path.clone();

            // Verify CodeEdit content matches current script (ScriptEditor only)
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
        }

        // Clear the expected path
        self.expected_script_path = None;

        self.reposition_mode_label();

        // Switch to Neovim buffer for this file (creates if not exists)
        // Returns cursor position from Neovim and whether buffer was newly created
        if let Some((line, col, is_new)) = self.switch_to_neovim_buffer() {
            if is_new {
                // New buffer (Godot startup): keep Godot's cursor position
                // Godot restores cursor from previous session, sync it to Neovim
                crate::verbose_print!("[godot-neovim] New buffer: keeping Godot cursor position");
            } else {
                // Existing buffer: apply Neovim's cursor position to Godot
                if let Some(ref mut editor) = self.current_editor {
                    let line_count = editor.get_line_count();
                    let safe_line = (line as i32).min(line_count - 1).max(0);
                    let line_text = editor.get_line(safe_line).to_string();
                    // Convert byte column from Neovim to character column for Godot
                    let char_col = Self::byte_col_to_char_col(&line_text, col as i32);
                    let line_char_count = line_text.chars().count() as i32;
                    let safe_col = char_col.min(line_char_count).max(0);

                    crate::verbose_print!(
                        "[godot-neovim] Applying cursor from Neovim: ({}, {}) -> ({}, {}) (byte_col={}, char_col={})",
                        line,
                        col,
                        safe_line,
                        safe_col,
                        col,
                        char_col
                    );

                    // Set syncing_from_grid to prevent on_caret_changed from setting user_cursor_sync
                    // This ensures zz/zt/zb viewport commands work after buffer switch
                    self.syncing_from_grid = true;
                    editor.set_caret_line(safe_line);
                    editor.set_caret_column(safe_col);
                    self.syncing_from_grid = false;
                }
            }
        }

        self.update_cursor_from_editor();
        self.sync_cursor_to_neovim();

        // If script was closed, grab focus on the new CodeEdit
        if self.focus_after_script_change {
            self.focus_after_script_change = false;
            if let Some(ref mut editor) = self.current_editor {
                if editor.is_instance_valid() {
                    editor.grab_focus();
                    crate::verbose_print!("[godot-neovim] Focused CodeEdit after script close");
                }
            }
        }
    }

    /// Handle input from CodeEdit's gui_input signal
    /// Float windows don't receive input through EditorPlugin.input(), so we connect to gui_input signal
    /// This allows us to intercept and consume input before CodeEdit processes it
    #[func]
    fn on_codeedit_gui_input(&mut self, event: Gd<godot::classes::InputEvent>) {
        // Only process if this is a float window
        // Main window input is handled by EditorPlugin.input()
        if !self.is_in_float_window() {
            return;
        }

        // Reset closing_all_tabs flag on user input (after :qa cancel)
        if self.closing_all_tabs {
            self.closing_all_tabs = false;
            self.pending_buffer_deletions.clear();
            crate::verbose_print!("[godot-neovim] :qa - Resetting flag on gui_input (cancelled)");
            self.handle_script_changed();
        }

        // Handle mouse click events
        if let Ok(mouse_event) = event
            .clone()
            .try_cast::<godot::classes::InputEventMouseButton>()
        {
            if mouse_event.get_button_index() == godot::global::MouseButton::LEFT {
                if mouse_event.is_pressed() {
                    self.mouse_dragging = true;
                    self.mouse_selection_syncing = false;
                    if let Some(ref mut editor) = self.current_editor {
                        editor.set_selecting_enabled(true);
                    }
                } else if self.mouse_dragging {
                    self.mouse_dragging = false;
                    self.base_mut()
                        .call_deferred("sync_mouse_selection_to_neovim", &[]);
                }
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

        // Check if Neovim is connected
        if self.get_current_neovim().is_none() {
            crate::verbose_print!("[godot-neovim] gui_input: No neovim");
            return;
        }

        // Clear user_cursor_sync flag to allow viewport sync from Neovim
        self.user_cursor_sync = false;

        // Accept the event to prevent CodeEdit from processing it
        // This must be done in Normal/Visual modes to prevent characters from being typed
        // In Insert/Replace modes, we let CodeEdit handle the input normally
        let should_consume = !self.is_insert_mode() && !self.is_replace_mode();
        if should_consume {
            if let Some(ref mut editor) = self.current_editor {
                editor.accept_event();
            }
        }

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

    /// Check if current CodeEdit is in a float window
    fn is_in_float_window(&self) -> bool {
        let Some(ref editor) = self.current_editor else {
            return false;
        };

        let Some(window) = editor.get_window() else {
            return false;
        };

        let main_window = EditorInterface::singleton()
            .get_base_control()
            .and_then(|c| c.get_window());

        match main_window {
            Some(ref main) => window.instance_id() != main.instance_id(),
            None => false,
        }
    }

    /// Handle input from float windows (legacy - kept for compatibility)
    /// Now using gui_input signal instead
    #[func]
    fn on_float_window_input(&mut self, _event: Gd<godot::classes::InputEvent>) {
        // Legacy handler - now using gui_input signal via on_codeedit_gui_input
    }

    /// Recovery dialog: Save all files and restart Neovim
    #[func]
    fn on_recovery_save_restart(&mut self) {
        crate::verbose_print!("[godot-neovim] Recovery: Save & Restart selected");
        self.save_all_open_scripts();
        self.restart_neovim();
        self.cleanup_recovery_dialog();
    }

    /// Recovery dialog: Cancel (do nothing)
    #[func]
    fn on_recovery_cancel(&mut self) {
        crate::verbose_print!("[godot-neovim] Recovery: Cancel selected");
        self.cleanup_recovery_dialog();
    }

    /// Recovery dialog: Handle custom action (Restart without Saving)
    #[func]
    fn on_recovery_custom_action(&mut self, action: GString) {
        let action_str = action.to_string();
        crate::verbose_print!("[godot-neovim] Recovery: Custom action: {}", action_str);
        if action_str == "restart_no_save" {
            self.restart_neovim();
        }
        self.cleanup_recovery_dialog();
    }
}

/// Private helper methods for Neovim instance management
impl GodotNeovimPlugin {
    /// Get Neovim client for a specific editor type
    /// Use this when you need to borrow other fields mutably while holding the neovim reference
    pub(super) fn neovim_for(&self, editor_type: EditorType) -> Option<&Mutex<NeovimClient>> {
        match editor_type {
            EditorType::Shader => self.shader_neovim.as_ref(),
            _ => self.script_neovim.as_ref(),
        }
    }

    /// Get the current Neovim client based on current_editor_type
    /// Note: This borrows self, so you cannot mutably borrow other fields while holding the result
    pub(super) fn get_current_neovim(&self) -> Option<&Mutex<NeovimClient>> {
        self.neovim_for(self.current_editor_type)
    }
}

// Note: handle_normal_mode_input moved to input/normal.rs
