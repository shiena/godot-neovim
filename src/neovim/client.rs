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
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Minimum required Neovim version (major, minor, patch)
const NEOVIM_REQUIRED_VERSION: (u64, u64, u64) = (0, 9, 0);

/// Default timeout for RPC commands (milliseconds)
const RPC_TIMEOUT_MS: u64 = 50;

/// Extended timeout for operations that may trigger dialogs (e.g., swap file)
const RPC_EXTENDED_TIMEOUT_MS: u64 = 500;

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
        })
    }

    /// Check if there are pending updates from redraw events
    #[allow(dead_code)]
    pub fn has_pending_updates(&self) -> bool {
        self.has_updates.load(Ordering::SeqCst)
    }

    /// Take pending updates (clears the flag) and return current state
    pub fn take_state(&self) -> Option<(String, (i64, i64))> {
        if !self.has_updates.swap(false, Ordering::SeqCst) {
            return None;
        }

        // Try to get state without blocking
        self.runtime.block_on(async {
            let state = self.state.lock().await;
            Some((state.mode.clone(), state.cursor))
        })
    }

    /// Get current state (always returns, doesn't check updates flag)
    #[allow(dead_code)]
    pub fn get_state(&self) -> (String, (i64, i64)) {
        self.runtime.block_on(async {
            let state = self.state.lock().await;
            (state.mode.clone(), state.cursor)
        })
    }

    /// Poll the runtime to process pending async events (like redraw notifications)
    /// This must be called regularly (e.g., every frame) to receive events
    pub fn poll(&self) {
        self.runtime.block_on(async {
            // Give the runtime a chance to process IO events
            // A short sleep allows tokio to poll IO
            tokio::time::sleep(std::time::Duration::from_micros(100)).await;
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
    pub fn start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let handler = self.handler.clone();
        let neovim_arc = self.neovim.clone();
        let nvim_path = self.nvim_path.clone();
        let clean = self.clean;

        crate::verbose_print!(
            "[godot-neovim] Starting Neovim: {} (clean={})",
            nvim_path,
            clean
        );

        let io_handle = self.runtime.block_on(async {
            let mut cmd = create_nvim_command(&nvim_path, clean);

            let (neovim, io_handler, _child) = create::new_child_cmd(&mut cmd, handler).await?;

            // Attach UI to receive redraw events
            let mut ui_opts = UiAttachOptions::new();
            ui_opts.set_rgb(true);
            ui_opts.set_linegrid_external(true);
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

            let mut nvim_lock = neovim_arc.lock().await;
            *nvim_lock = Some(neovim);

            crate::verbose_print!("[godot-neovim] Neovim started successfully");
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(io_handler)
        })?;

        self.io_handle = Some(io_handle);

        crate::verbose_print!(
            "[godot-neovim] IO handler spawned, has_updates={}",
            self.has_updates.load(std::sync::atomic::Ordering::SeqCst)
        );

        Ok(())
    }

    /// Stop Neovim process
    pub fn stop(&mut self) {
        // Abort the IO handler first to prevent blocking on read
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
                    (mode, blocking)
                }
                _ => {
                    // Timeout or error - return current cached mode
                    ("n".to_string(), true) // blocking=true indicates pending
                }
            }
        })
    }

    /// Send keys to Neovim
    pub fn input(&self, keys: &str) -> Result<(), String> {
        let neovim_arc = self.neovim.clone();
        let keys = keys.to_string();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                neovim
                    .input(&keys)
                    .await
                    .map_err(|e| format!("Failed to send input: {}", e))?;
                Ok(())
            } else {
                Err("Neovim not connected".to_string())
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

    /// Set buffer content
    pub fn set_buffer_lines(&self, start: i64, end: i64, lines: Vec<String>) -> Result<(), String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                let buffer = neovim
                    .get_current_buf()
                    .await
                    .map_err(|e| format!("Failed to get buffer: {}", e))?;
                buffer
                    .set_lines(start, end, false, lines)
                    .await
                    .map_err(|e| format!("Failed to set lines: {}", e))?;
                Ok(())
            } else {
                Err("Neovim not connected".to_string())
            }
        })
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

    /// Set buffer as not modified in Neovim
    pub fn set_buffer_not_modified(&self) {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let _ = tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                let nvim_lock = neovim_arc.lock().await;
                if let Some(neovim) = nvim_lock.as_ref() {
                    let _ = neovim.command("set nomodified").await;
                }
            })
            .await;
        });
    }

    /// Set buffer name (for LSP compatibility)
    /// Uses timeout to prevent blocking on E325 swap file dialogs
    pub fn set_buffer_name(&self, name: &str) -> Result<(), String> {
        let neovim_arc = self.neovim.clone();
        let name = name.to_string();

        self.runtime.block_on(async {
            // Use timeout to prevent blocking on E325 ATTENTION dialogs
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(RPC_EXTENDED_TIMEOUT_MS),
                async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        // Suppress E325 ATTENTION errors before setting buffer name
                        let _ = neovim.command("set shortmess+=A").await;

                        let buffer = neovim
                            .get_current_buf()
                            .await
                            .map_err(|e| format!("Failed to get buffer: {}", e))?;

                        let set_result = buffer.set_name(&name).await;

                        if set_result.is_err() {
                            // E325 error may leave Neovim in confirmation state
                            // Send Enter to clear any pending prompts
                            let _ = neovim.input("<CR>").await;
                            let _ = neovim.input("<Esc>").await;
                        }

                        set_result.map_err(|e| format!("Failed to set buffer name: {}", e))?;
                        Ok(())
                    } else {
                        Err("Neovim not connected".to_string())
                    }
                },
            )
            .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err("Timeout setting buffer name (possible swap file issue)".to_string()),
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
