use crate::neovim::{NeovimHandler, NeovimState};
use crate::settings;
use godot::prelude::godot_warn;
use nvim_rs::create::tokio as create;
use nvim_rs::{Neovim, UiAttachOptions};
use std::fmt;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::process::Command;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Minimum required Neovim version (major, minor, patch)
const NEOVIM_REQUIRED_VERSION: (u64, u64, u64) = (0, 9, 0);

/// Default timeout for RPC commands (milliseconds)
const RPC_TIMEOUT_MS: u64 = 100;

/// Extended timeout for operations that may trigger dialogs (e.g., swap file)
const RPC_EXTENDED_TIMEOUT_MS: u64 = 500;

/// Fallback Lua code when external plugin is not available
/// Prefer using the external lua/godot_neovim/init.lua file
const LUA_FALLBACK_CODE: &str = r#"
-- godot_neovim: Buffer management for Godot integration (fallback)
_G.godot_neovim = {}

-- Register buffer with initial content (clears undo history)
function _G.godot_neovim.buffer_register(bufnr, lines)
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end
    local saved_ul = vim.bo[bufnr].undolevels
    vim.bo[bufnr].undolevels = -1
    vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)
    vim.bo[bufnr].undolevels = saved_ul
    vim.bo[bufnr].modified = false
    return vim.api.nvim_buf_get_changedtick(bufnr)
end

-- Update buffer content (preserves undo history for 'u' command)
function _G.godot_neovim.buffer_update(bufnr, lines)
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end
    vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)
    return vim.api.nvim_buf_get_changedtick(bufnr)
end
"#;

type Writer = nvim_rs::compat::tokio::Compat<tokio::process::ChildStdin>;

/// Neovim version information
#[derive(Debug, Clone, Default)]
pub struct NeovimVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl NeovimVersion {
    /// Check if version meets minimum requirements
    pub fn meets_requirement(&self, major: u64, minor: u64, patch: u64) -> bool {
        if self.major > major {
            return true;
        }
        if self.major < major {
            return false;
        }
        // major is equal
        if self.minor > minor {
            return true;
        }
        if self.minor < minor {
            return false;
        }
        // minor is equal
        self.patch >= patch
    }
}

impl fmt::Display for NeovimVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Result from switching to a buffer
#[derive(Debug, Clone)]
pub struct SwitchBufferResult {
    /// Buffer number in Neovim
    pub bufnr: i64,
    /// Current changedtick
    pub tick: i64,
    /// Whether this is a newly created buffer
    pub is_new: bool,
    /// Whether buffer is attached for notifications
    pub attached: bool,
    /// Cursor position (line, col) - line is 1-indexed, col is 0-indexed
    pub cursor: (i64, i64),
}

/// Manages connection to Neovim process
pub struct NeovimClient {
    runtime: Runtime,
    neovim: Arc<Mutex<Option<Neovim<Writer>>>>,
    handler: NeovimHandler,
    nvim_path: String,
    /// Start Neovim with --clean flag (no plugins or user config)
    clean: bool,
    /// Shared state from handler (mode, cursor position)
    state: Arc<Mutex<NeovimState>>,
    /// Flag indicating new updates from redraw events
    has_updates: Arc<AtomicBool>,
    /// IO handler task - must be kept alive for events to be received
    #[allow(dead_code)]
    io_handle: Option<tokio::task::JoinHandle<Result<(), Box<nvim_rs::error::LoopError>>>>,
    /// Key input channel sender (unbounded for no key drops)
    key_input_tx: Option<UnboundedSender<String>>,
    /// Key input processor task handle
    #[allow(dead_code)]
    key_input_handle: Option<tokio::task::JoinHandle<()>>,
}

impl NeovimClient {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Use current-thread runtime so all tasks run on the same thread
        // This ensures io_handler is processed during block_on calls
        let runtime = Builder::new_current_thread().enable_all().build()?;
        let nvim_path = settings::get_neovim_path();
        let clean = settings::get_neovim_clean();
        let handler = NeovimHandler::new();
        let state = handler.get_state();
        let has_updates = handler.get_updates_flag();
        Ok(Self {
            runtime,
            neovim: Arc::new(Mutex::new(None)),
            handler,
            nvim_path,
            clean,
            state,
            has_updates,
            io_handle: None,
            key_input_tx: None,
            key_input_handle: None,
        })
    }

    /// Check if there are pending updates from redraw events
    #[allow(dead_code)]
    pub fn has_pending_updates(&self) -> bool {
        self.has_updates.load(Ordering::SeqCst)
    }

    /// Take pending updates (clears the flag) and return current state
    /// Prefers actual_cursor (from CursorMoved autocmd) over grid cursor (from redraw)
    /// because actual_cursor is byte position, while grid cursor is screen position
    #[allow(dead_code)]
    pub fn take_state(&self) -> Option<(String, (i64, i64))> {
        if !self.has_updates.swap(false, Ordering::SeqCst) {
            return None;
        }

        // Try to get state without blocking
        self.runtime.block_on(async {
            let mut state = self.state.lock().await;
            // Prefer actual_cursor (byte position) over grid cursor (screen position)
            // This is important for files with tab characters
            let cursor = if let Some(actual) = state.actual_cursor.take() {
                actual
            } else {
                state.cursor
            };
            Some((state.mode.clone(), cursor))
        })
    }

    /// Take viewport changes (topline, botline, curline, curcol) if viewport has changed
    /// Returns None if viewport hasn't changed since last call
    /// The curline/curcol are the buffer cursor positions from win_viewport
    pub fn take_viewport(&self) -> Option<(i64, i64, i64, i64)> {
        self.runtime.block_on(async {
            let mut state = self.state.lock().await;
            if state.viewport_changed {
                state.viewport_changed = false;
                Some((
                    state.viewport_topline,
                    state.viewport_botline,
                    state.viewport_curline,
                    state.viewport_curcol,
                ))
            } else {
                None
            }
        })
    }

    /// Force viewport_changed flag to true
    /// Used after buffer switch to ensure next viewport event is processed
    /// even if the values haven't changed from previous buffer
    pub fn force_viewport_changed(&self) {
        self.runtime.block_on(async {
            let mut state = self.state.lock().await;
            state.viewport_changed = true;
        })
    }

    /// Resize Neovim's UI to match Godot editor's visible area
    /// This is important for viewport commands (zz, zt, zb) to work correctly
    pub fn ui_try_resize(&self, width: i64, height: i64) {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                if let Err(e) = neovim.ui_try_resize(width, height).await {
                    // Log error but don't fail - resize is best-effort
                    crate::verbose_print!("[godot-neovim] Failed to resize UI: {}", e);
                } else {
                    crate::verbose_print!(
                        "[godot-neovim] Resized Neovim UI to {}x{}",
                        width,
                        height
                    );
                }
            }
        });
    }

    /// Get current state (always returns, doesn't check updates flag)
    #[allow(dead_code)]
    pub fn get_state(&self) -> (String, (i64, i64)) {
        self.runtime.block_on(async {
            let mut state = self.state.lock().await;
            let cursor = if let Some(actual) = state.actual_cursor.take() {
                actual
            } else {
                state.cursor
            };
            (state.mode.clone(), cursor)
        })
    }

    /// Poll the runtime to process pending async events (like redraw notifications)
    /// This must be called regularly (e.g., every frame) to receive events
    pub fn poll(&self) {
        self.runtime.block_on(async {
            // Give the runtime a chance to process IO events
            // 1ms allows enough time for:
            // 1. spawn() tasks to execute (input_async)
            // 2. Neovim to process input and send redraw events
            // 3. IO handler to receive and process events
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        });
    }

    /// Check if IO handler is still running
    #[allow(dead_code)]
    pub fn is_io_running(&self) -> bool {
        self.io_handle.as_ref().is_some_and(|h| !h.is_finished())
    }

    /// Update the Neovim executable path
    #[allow(dead_code)]
    pub fn set_neovim_path(&mut self, path: String) {
        self.nvim_path = path;
    }

    /// Get the current Neovim executable path
    #[allow(dead_code)]
    pub fn get_neovim_path(&self) -> &str {
        &self.nvim_path
    }

    /// Start Neovim process and establish connection
    /// If addons_path is provided, loads the Lua plugin from that directory
    pub fn start(
        &mut self,
        addons_path: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let handler = self.handler.clone();
        let neovim_arc = self.neovim.clone();
        let nvim_path = self.nvim_path.clone();
        let clean = self.clean;
        let addons_path_owned = addons_path.map(String::from);

        crate::verbose_print!(
            "[godot-neovim] Starting Neovim: {} (clean={}, addons_path={:?})",
            nvim_path,
            clean,
            addons_path
        );

        let io_handle = self.runtime.block_on(async {
            let mut cmd = create_nvim_command(&nvim_path, clean);

            let (neovim, io_handler, _child) = create::new_child_cmd(&mut cmd, handler).await?;

            // Attach UI to receive redraw events
            // ext_multigrid enables win_viewport events for viewport synchronization
            let mut ui_opts = UiAttachOptions::new();
            ui_opts.set_rgb(true);
            ui_opts.set_linegrid_external(true);
            ui_opts.set_multigrid_external(true);
            neovim
                .ui_attach(80, 24, &ui_opts)
                .await
                .map_err(|e| format!("Failed to attach UI: {}", e))?;

            crate::verbose_print!("[godot-neovim] UI attached successfully");

            // Disable swap files and handle E325 ATTENTION errors in headless mode
            // - noswapfile: Don't create new swap files
            // - shortmess+=A: Suppress swap file warnings
            // - SwapExists autocmd: Auto-select 'edit anyway' if swap exists
            neovim
                .command(
                    "set noswapfile shortmess+=A | autocmd SwapExists * let v:swapchoice = 'e'",
                )
                .await
                .map_err(|e| format!("Failed to configure swapfile handling: {}", e))?;

            // Check Neovim version before storing
            let version = get_neovim_version(&neovim).await;
            let (req_major, req_minor, req_patch) = NEOVIM_REQUIRED_VERSION;

            if let Some(ref ver) = version {
                crate::verbose_print!("[godot-neovim] Neovim version: {}", ver);

                if !ver.meets_requirement(req_major, req_minor, req_patch) {
                    let msg = format!(
                        "Neovim version {} is below minimum required {}.{}.{}. Some features may not work correctly.",
                        ver, req_major, req_minor, req_patch
                    );
                    godot_warn!("[godot-neovim] {}", msg);
                }
            } else {
                crate::verbose_print!("[godot-neovim] Could not determine Neovim version");
            }

            // Initialize godot_neovim Lua module
            // Prefer external plugin if addons_path is provided
            if let Some(ref path) = addons_path_owned {
                // Escape backslashes for Lua string (Windows paths)
                let lua_path = path.replace('\\', "/");
                let init_code = format!(
                    r#"
                    -- Add addons path to runtimepath
                    vim.opt.runtimepath:append("{}")
                    -- Load the godot_neovim module
                    require('godot_neovim')
                    "#,
                    lua_path
                );
                neovim
                    .exec_lua(&init_code, vec![])
                    .await
                    .map_err(|e| format!("Failed to load Lua plugin from {}: {}", path, e))?;
                crate::verbose_print!(
                    "[godot-neovim] Lua module loaded from external file: {}",
                    path
                );
            } else {
                // Fallback to embedded Lua code
                neovim
                    .exec_lua(LUA_FALLBACK_CODE, vec![])
                    .await
                    .map_err(|e| format!("Failed to initialize Lua module: {}", e))?;
                crate::verbose_print!("[godot-neovim] Lua module initialized (embedded fallback)");
            }

            let mut nvim_lock = neovim_arc.lock().await;
            *nvim_lock = Some(neovim);

            crate::verbose_print!("[godot-neovim] Neovim started successfully");
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(io_handler)
        })?;

        self.io_handle = Some(io_handle);

        // Create unbounded channel for key input (no key drops)
        let (tx, mut rx) = unbounded_channel::<String>();
        self.key_input_tx = Some(tx);

        // Spawn key input processor task
        let neovim_arc = self.neovim.clone();
        let key_input_handle = self.runtime.spawn(async move {
            while let Some(keys) = rx.recv().await {
                let nvim_lock = neovim_arc.lock().await;
                if let Some(neovim) = nvim_lock.as_ref() {
                    if let Err(e) = neovim.input(&keys).await {
                        // Log error but continue processing
                        // Note: Can't use godot_error here (tokio thread)
                        eprintln!("[godot-neovim] Failed to send key '{}': {}", keys, e);
                    }
                }
                // Release lock before next iteration
                drop(nvim_lock);
            }
        });
        self.key_input_handle = Some(key_input_handle);

        crate::verbose_print!(
            "[godot-neovim] IO handler spawned, has_updates={}",
            self.has_updates.load(std::sync::atomic::Ordering::SeqCst)
        );
        crate::verbose_print!("[godot-neovim] Key input channel initialized (unbounded)");

        Ok(())
    }

    /// Stop Neovim process
    pub fn stop(&mut self) {
        // Abort the key input handler first
        if let Some(handle) = self.key_input_handle.take() {
            handle.abort();
            crate::verbose_print!("[godot-neovim] Key input handler aborted");
        }
        // Clear the key input sender
        self.key_input_tx = None;

        // Abort the IO handler to prevent blocking on read
        if let Some(handle) = self.io_handle.take() {
            handle.abort();
            crate::verbose_print!("[godot-neovim] IO handler aborted");
        }

        // Clear the neovim instance without sending quit command
        // (IO is already aborted, command would timeout anyway)
        let neovim_arc = self.neovim.clone();
        self.runtime.block_on(async {
            let mut nvim_lock = neovim_arc.lock().await;
            nvim_lock.take();
        });
        crate::verbose_print!("[godot-neovim] Neovim stopped");
    }

    /// Get current mode from Neovim directly with timeout
    /// Returns (mode, blocking) tuple
    #[allow(dead_code)]
    pub fn get_mode(&self) -> (String, bool) {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            // Use timeout to avoid blocking on operator-pending commands like 'g'
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        neovim.get_mode().await.ok()
                    } else {
                        None
                    }
                })
                .await;

            match result {
                Ok(Some(mode_info)) => {
                    let mut mode = "n".to_string();
                    let mut blocking = false;
                    for (key, value) in mode_info {
                        if let rmpv::Value::String(k) = &key {
                            match k.as_str() {
                                Some("mode") => {
                                    if let rmpv::Value::String(v) = value {
                                        mode = v.as_str().unwrap_or("n").to_string();
                                    }
                                }
                                Some("blocking") => {
                                    if let rmpv::Value::Boolean(b) = value {
                                        blocking = b;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    crate::verbose_print!(
                        "[godot-neovim] get_mode: mode={}, blocking={} (from Neovim)",
                        mode,
                        blocking
                    );
                    (mode, blocking)
                }
                Ok(None) => {
                    crate::verbose_print!("[godot-neovim] get_mode: None response");
                    ("n".to_string(), false)
                }
                Err(_) => {
                    // Timeout or error - assume not blocking to allow normal operation
                    crate::verbose_print!("[godot-neovim] get_mode: timeout/error");
                    ("n".to_string(), false)
                }
            }
        })
    }

    /// Send keys to Neovim with timeout
    pub fn input(&self, keys: &str) -> Result<(), String> {
        let neovim_arc = self.neovim.clone();
        let keys = keys.to_string();

        self.runtime.block_on(async {
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        // nvim_input returns bytes written, but we only care about success
                        neovim
                            .input(&keys)
                            .await
                            .map(|_| ())
                            .map_err(|e| format!("Failed to send input: {}", e))
                    } else {
                        Err("Neovim not connected".to_string())
                    }
                })
                .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err("Timeout sending input".to_string()),
            }
        })
    }

    /// Send keys asynchronously (keys are processed after RPC returns)
    /// Uses external Lua function
    #[allow(dead_code)]
    pub fn send_keys_async(&self, keys: &str) -> Result<(), String> {
        use rmpv::Value;
        let neovim_arc = self.neovim.clone();
        let keys = keys.to_string();

        self.runtime.block_on(async {
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        neovim
                            .exec_lua(
                                "return _G.godot_neovim.send_keys(...)",
                                vec![Value::from(keys)],
                            )
                            .await
                            .map_err(|e| format!("Failed to send keys: {}", e))?;
                        Ok(())
                    } else {
                        Err("Neovim not connected".to_string())
                    }
                })
                .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err("Timeout sending keys".to_string()),
            }
        })
    }

    /// Get current mode and cursor position via Lua
    /// Returns (mode, cursor_line, cursor_col) where line is 1-indexed
    #[allow(dead_code)]
    pub fn get_state_lua(&self) -> Result<(String, i64, i64), String> {
        use rmpv::Value;
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        let result = neovim
                            .exec_lua("return _G.godot_neovim.get_state()", vec![])
                            .await
                            .map_err(|e| format!("Failed to get state: {}", e))?;

                        // Parse result { mode, line, col }
                        if let Value::Map(map) = result {
                            let mut mode = "n".to_string();
                            let mut line = 1i64;
                            let mut col = 0i64;

                            for (k, v) in map {
                                if let Value::String(key) = k {
                                    match key.as_str() {
                                        Some("mode") => {
                                            if let Value::String(m) = v {
                                                mode = m.as_str().unwrap_or("n").to_string();
                                            }
                                        }
                                        Some("line") => line = v.as_i64().unwrap_or(1),
                                        Some("col") => col = v.as_i64().unwrap_or(0),
                                        _ => {}
                                    }
                                }
                            }
                            Ok((mode, line, col))
                        } else {
                            Err("Invalid response from Lua".to_string())
                        }
                    } else {
                        Err("Neovim not connected".to_string())
                    }
                })
                .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err("Timeout getting state".to_string()),
            }
        })
    }

    /// Send keys to Neovim without blocking for response
    /// State updates will come via redraw events
    #[allow(dead_code)]
    pub fn input_async(&self, keys: &str) -> Result<(), String> {
        let neovim_arc = self.neovim.clone();
        let keys = keys.to_string();

        // Spawn the input task without waiting
        self.runtime.spawn(async move {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                let _ = neovim.input(&keys).await;
            }
        });

        Ok(())
    }

    /// Send keys via unbounded channel (never blocks, never drops keys)
    /// Keys are processed in order by a dedicated task
    /// Returns true if key was queued, false if channel is not available
    pub fn send_key_via_channel(&self, keys: &str) -> bool {
        if let Some(ref tx) = self.key_input_tx {
            // send() on unbounded channel never blocks and only fails if receiver is dropped
            tx.send(keys.to_string()).is_ok()
        } else {
            false
        }
    }

    /// Send a serial command (keyboard input, cursor movement)
    /// Serial commands are processed in order but don't block waiting for response
    #[allow(dead_code)]
    pub fn send_serial(&self, cmd: super::SerialCommand) {
        use super::SerialCommand;

        let neovim_arc = self.neovim.clone();

        match cmd {
            SerialCommand::Input(keys) => {
                // Fire and forget - don't wait for response
                self.runtime.spawn(async move {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        let _ = neovim.input(&keys).await;
                    }
                });
            }
            SerialCommand::SetCursor { line, col } => {
                // Cursor setting also fire and forget
                self.runtime.spawn(async move {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        if let Ok(window) = neovim.get_current_win().await {
                            let _ = window.set_cursor((line, col)).await;
                        }
                    }
                });
            }
        }
    }

    /// Execute Neovim command
    #[allow(dead_code)]
    pub fn command(&self, cmd: &str) -> Result<(), String> {
        let neovim_arc = self.neovim.clone();
        let cmd = cmd.to_string();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                neovim
                    .command(&cmd)
                    .await
                    .map_err(|e| format!("Failed to execute command: {}", e))?;
                Ok(())
            } else {
                Err("Neovim not connected".to_string())
            }
        })
    }

    /// Get buffer content
    #[allow(dead_code)]
    pub fn get_buffer_lines(&self, start: i64, end: i64) -> Result<Vec<String>, String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                let buffer = neovim
                    .get_current_buf()
                    .await
                    .map_err(|e| format!("Failed to get buffer: {}", e))?;
                let lines = buffer
                    .get_lines(start, end, false)
                    .await
                    .map_err(|e| format!("Failed to get lines: {}", e))?;
                Ok(lines)
            } else {
                Err("Neovim not connected".to_string())
            }
        })
    }

    /// Get buffer line count
    #[allow(dead_code)]
    pub fn get_line_count(&self) -> Result<i64, String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                let buffer = neovim
                    .get_current_buf()
                    .await
                    .map_err(|e| format!("Failed to get buffer: {}", e))?;
                let count = buffer
                    .line_count()
                    .await
                    .map_err(|e| format!("Failed to get line count: {}", e))?;
                Ok(count)
            } else {
                Err("Neovim not connected".to_string())
            }
        })
    }

    /// Update buffer content (preserves undo history for 'u' command)
    /// Uses Lua function to properly manage undo history
    pub fn buffer_update(&self, lines: Vec<String>) -> Result<i64, String> {
        use rmpv::Value;
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                // Convert lines to Lua array
                let lines_value: Vec<Value> = lines.into_iter().map(Value::from).collect();
                let args = vec![Value::from(0i64), Value::Array(lines_value)];

                let result = neovim
                    .exec_lua("return _G.godot_neovim.buffer_update(...)", args)
                    .await
                    .map_err(|e| format!("Failed to update buffer: {}", e))?;

                // Return changedtick
                result
                    .as_i64()
                    .ok_or_else(|| "Invalid changedtick returned".to_string())
            } else {
                Err("Neovim not connected".to_string())
            }
        })
    }

    /// Switch to buffer by path, creating and initializing if needed
    /// Returns (bufnr, tick, is_new, cursor) where cursor is (line, col) 1-indexed
    pub fn switch_to_buffer(
        &self,
        path: &str,
        lines: Option<Vec<String>>,
    ) -> Result<SwitchBufferResult, String> {
        use rmpv::Value;
        let neovim_arc = self.neovim.clone();
        let path = path.to_string();

        self.runtime.block_on(async {
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(RPC_EXTENDED_TIMEOUT_MS),
                async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        // Prepare arguments
                        let lines_value = match lines {
                            Some(l) => Value::Array(l.into_iter().map(Value::from).collect()),
                            None => Value::Nil,
                        };
                        let args = vec![Value::from(path), lines_value];

                        let result = neovim
                            .exec_lua("return _G.godot_neovim.switch_to_buffer(...)", args)
                            .await
                            .map_err(|e| format!("Failed to switch buffer: {}", e))?;

                        // Parse result table { bufnr, tick, is_new, attached, cursor }
                        Self::parse_switch_buffer_result(result)
                    } else {
                        Err("Neovim not connected".to_string())
                    }
                },
            )
            .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err("Timeout switching buffer".to_string()),
            }
        })
    }

    /// Parse the result from switch_to_buffer Lua function
    fn parse_switch_buffer_result(result: rmpv::Value) -> Result<SwitchBufferResult, String> {
        use rmpv::Value;

        if let Value::Map(map) = result {
            let mut bufnr: Option<i64> = None;
            let mut tick: Option<i64> = None;
            let mut is_new: Option<bool> = None;
            let mut attached: Option<bool> = None;
            let mut cursor: Option<(i64, i64)> = None;

            for (key, value) in map {
                if let Value::String(k) = key {
                    match k.as_str() {
                        Some("bufnr") => bufnr = value.as_i64(),
                        Some("tick") => tick = value.as_i64(),
                        Some("is_new") => is_new = value.as_bool(),
                        Some("attached") => attached = value.as_bool(),
                        Some("cursor") => {
                            // cursor is [row, col] array, 1-indexed
                            if let Value::Array(arr) = value {
                                if arr.len() >= 2 {
                                    let row = arr[0].as_i64().unwrap_or(1);
                                    let col = arr[1].as_i64().unwrap_or(0);
                                    cursor = Some((row, col));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            match (bufnr, tick, is_new, attached, cursor) {
                (Some(b), Some(t), Some(n), Some(a), Some(c)) => Ok(SwitchBufferResult {
                    bufnr: b,
                    tick: t,
                    is_new: n,
                    attached: a,
                    cursor: c,
                }),
                _ => Err("Invalid result from switch_to_buffer".to_string()),
            }
        } else {
            Err("Expected table result from switch_to_buffer".to_string())
        }
    }

    /// Get cursor position (1-indexed line, 0-indexed column) with timeout
    pub fn get_cursor(&self) -> Result<(i64, i64), String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            // Use timeout to avoid blocking on operator-pending commands
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        let window = neovim.get_current_win().await.ok()?;
                        window.get_cursor().await.ok()
                    } else {
                        None
                    }
                })
                .await;

            match result {
                Ok(Some(pos)) => Ok(pos),
                Ok(None) => Err("Failed to get cursor".to_string()),
                Err(_) => Err("Timeout getting cursor".to_string()),
            }
        })
    }

    /// Set cursor position with timeout
    pub fn set_cursor(&self, line: i64, col: i64) -> Result<(), String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            // Use timeout to avoid blocking
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        let window = neovim.get_current_win().await.ok()?;
                        window.set_cursor((line, col)).await.ok()?;
                        Some(())
                    } else {
                        None
                    }
                })
                .await;

            match result {
                Ok(Some(())) => Ok(()),
                Ok(None) => Err("Failed to set cursor".to_string()),
                Err(_) => Err("Timeout setting cursor".to_string()),
            }
        })
    }

    /// Get visual selection range
    /// Returns ((start_line, start_col), (end_line, end_col)) - 0-indexed
    /// Returns None if not in visual mode or failed to get selection
    pub fn get_visual_selection(&self) -> Option<((i64, i64), (i64, i64))> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    let neovim = nvim_lock.as_ref()?;

                    // Get visual start position using getpos("v")
                    let visual_start = neovim
                        .call_function("getpos", vec![rmpv::Value::from("v")])
                        .await
                        .ok()?;

                    // Get current cursor position using getpos(".")
                    let cursor_pos = neovim
                        .call_function("getpos", vec![rmpv::Value::from(".")])
                        .await
                        .ok()?;

                    // Parse positions: [bufnum, lnum, col, off] (1-indexed)
                    let parse_pos = |val: rmpv::Value| -> Option<(i64, i64)> {
                        let arr = val.as_array()?;
                        let line = arr.get(1)?.as_i64()? - 1; // Convert to 0-indexed
                        let col = arr.get(2)?.as_i64()? - 1; // Convert to 0-indexed
                        Some((line, col))
                    };

                    let start = parse_pos(visual_start)?;
                    let end = parse_pos(cursor_pos)?;

                    Some((start, end))
                })
                .await;

            match result {
                Ok(Some(selection)) => Some(selection),
                _ => None,
            }
        })
    }

    /// Check if buffer is modified in Neovim
    #[allow(dead_code)]
    pub fn is_buffer_modified(&self) -> bool {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        // Get &modified option
                        let modified = neovim
                            .call_function(
                                "getbufvar",
                                vec![
                                    rmpv::Value::from(0), // current buffer
                                    rmpv::Value::from("&modified"),
                                ],
                            )
                            .await
                            .ok()?;
                        // Returns 1 if modified, 0 if not
                        Some(modified.as_i64().unwrap_or(0) != 0)
                    } else {
                        None
                    }
                })
                .await;

            match result {
                Ok(Some(modified)) => modified,
                _ => false,
            }
        })
    }

    /// Execute Lua code in Neovim
    #[allow(dead_code)]
    pub fn execute_lua(&self, code: &str) -> Result<rmpv::Value, String> {
        let neovim_arc = self.neovim.clone();
        let code = code.to_string();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                neovim
                    .exec_lua(&code, vec![])
                    .await
                    .map_err(|e| format!("Failed to execute Lua: {}", e))
            } else {
                Err("Neovim not connected".to_string())
            }
        })
    }

    /// Attach to buffer for change notifications
    /// Returns true if successfully attached
    #[allow(dead_code)]
    pub fn buf_attach(&self, buf_id: i64) -> Result<bool, String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        // Get buffer by ID
                        let buffers = neovim
                            .list_bufs()
                            .await
                            .map_err(|e| format!("Failed to list buffers: {}", e))?;

                        // Find buffer with matching ID
                        for buf in buffers {
                            // nvim-rs Buffer doesn't expose ID directly, use get_number
                            if let Ok(num) = buf.get_number().await {
                                if num == buf_id {
                                    // Attach to buffer with send_buffer=false (we only want notifications)
                                    let attached = buf
                                        .attach(false, vec![])
                                        .await
                                        .map_err(|e| format!("Failed to attach: {}", e))?;
                                    return Ok(attached);
                                }
                            }
                        }
                        Err("Buffer not found".to_string())
                    } else {
                        Err("Neovim not connected".to_string())
                    }
                })
                .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err("Timeout attaching to buffer".to_string()),
            }
        })
    }

    /// Attach to current buffer for change notifications
    pub fn buf_attach_current(&self) -> Result<bool, String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        let buf = neovim
                            .get_current_buf()
                            .await
                            .map_err(|e| format!("Failed to get buffer: {}", e))?;

                        // Attach to buffer with send_buffer=false (we only want notifications)
                        let attached = buf
                            .attach(false, vec![])
                            .await
                            .map_err(|e| format!("Failed to attach: {}", e))?;
                        Ok(attached)
                    } else {
                        Err("Neovim not connected".to_string())
                    }
                })
                .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err("Timeout attaching to buffer".to_string()),
            }
        })
    }

    /// Get buffer events queue
    pub fn get_buf_events(
        &self,
    ) -> std::sync::Arc<tokio::sync::Mutex<std::collections::VecDeque<super::BufEvent>>> {
        self.handler.get_buf_events()
    }

    /// Check if there are pending buffer events
    pub fn has_buf_events(&self) -> bool {
        self.handler
            .get_buf_events_flag()
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Clear the buffer events flag
    pub fn clear_buf_events_flag(&self) {
        self.handler
            .get_buf_events_flag()
            .store(false, std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for NeovimClient {
    fn default() -> Self {
        Self::new().expect("Failed to create NeovimClient")
    }
}

impl Drop for NeovimClient {
    fn drop(&mut self) {
        self.stop();
        // Force non-blocking runtime shutdown to avoid hanging
        // Note: This consumes self.runtime, but we're in drop so that's fine
        crate::verbose_print!("[godot-neovim] Shutting down runtime");
    }
}

/// Create Neovim command with platform-specific settings
fn create_nvim_command(nvim_path: &str, clean: bool) -> Command {
    // -n: No swap file (prevents E325 ATTENTION errors in headless mode)
    let mut args = vec!["--embed", "--headless", "-n"];
    if clean {
        args.push("--clean");
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut std_cmd = std::process::Command::new(nvim_path);
        std_cmd
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);
        Command::from(std_cmd)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = Command::new(nvim_path);
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }
}

/// Get Neovim version from API info
async fn get_neovim_version(neovim: &Neovim<Writer>) -> Option<NeovimVersion> {
    // Call nvim_get_api_info to get version information
    let api_info = neovim.get_api_info().await.ok()?;

    // API info is [channel_id, {version: {...}, functions: [...], ...}]
    let info_map = api_info.get(1)?;
    let info_map = info_map.as_map()?;

    // Find version in the map
    for (key, value) in info_map {
        if key.as_str() == Some("version") {
            let version_map = value.as_map()?;
            let mut major = 0u64;
            let mut minor = 0u64;
            let mut patch = 0u64;

            for (vkey, vval) in version_map {
                match vkey.as_str() {
                    Some("major") => major = vval.as_u64().unwrap_or(0),
                    Some("minor") => minor = vval.as_u64().unwrap_or(0),
                    Some("patch") => patch = vval.as_u64().unwrap_or(0),
                    _ => {}
                }
            }

            return Some(NeovimVersion {
                major,
                minor,
                patch,
            });
        }
    }

    None
}
