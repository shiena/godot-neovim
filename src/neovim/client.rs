use crate::neovim::{NeovimHandler, NeovimState};
use crate::settings;
use nvim_rs::create::tokio as create;
use nvim_rs::{Neovim, UiAttachOptions};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::process::Command;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

type Writer = nvim_rs::compat::tokio::Compat<tokio::process::ChildStdin>;

/// Manages connection to Neovim process
pub struct NeovimClient {
    runtime: Runtime,
    neovim: Arc<Mutex<Option<Neovim<Writer>>>>,
    handler: NeovimHandler,
    nvim_path: String,
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
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()?;
        let nvim_path = settings::get_neovim_path();
        let handler = NeovimHandler::new();
        let state = handler.get_state();
        let has_updates = handler.get_updates_flag();
        Ok(Self {
            runtime,
            neovim: Arc::new(Mutex::new(None)),
            handler,
            nvim_path,
            state,
            has_updates,
            io_handle: None,
        })
    }

    /// Check if there are pending updates from redraw events
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
    pub fn get_state(&self) -> (String, (i64, i64)) {
        self.runtime.block_on(async {
            let state = self.state.lock().await;
            (state.mode.clone(), state.cursor)
        })
    }

    /// Poll the runtime to process pending async events (like redraw notifications)
    /// This must be called regularly (e.g., every frame) to receive events
    pub fn poll(&self) {
        // Check if io_handle is still running
        if let Some(ref handle) = self.io_handle {
            if handle.is_finished() {
                godot::global::godot_error!("[godot-neovim] IO handler has finished unexpectedly!");
            }
        }

        self.runtime.block_on(async {
            // Give the runtime a chance to process IO events
            // A short sleep allows tokio to poll IO
            tokio::time::sleep(std::time::Duration::from_micros(100)).await;
        });
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

        godot::global::godot_print!("[godot-neovim] Starting Neovim: {}", nvim_path);

        let io_handle = self.runtime.block_on(async {
            let mut cmd = create_nvim_command(&nvim_path);

            let (neovim, io_handler, _child) = create::new_child_cmd(&mut cmd, handler).await?;

            // Attach UI to receive redraw events
            let mut ui_opts = UiAttachOptions::new();
            ui_opts.set_rgb(true);
            ui_opts.set_linegrid_external(true);
            neovim
                .ui_attach(80, 24, &ui_opts)
                .await
                .map_err(|e| format!("Failed to attach UI: {}", e))?;

            godot::global::godot_print!("[godot-neovim] UI attached successfully");

            let mut nvim_lock = neovim_arc.lock().await;
            *nvim_lock = Some(neovim);

            godot::global::godot_print!("[godot-neovim] Neovim started successfully");
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(io_handler)
        })?;

        self.io_handle = Some(io_handle);

        godot::global::godot_print!("[godot-neovim] IO handler spawned, has_updates={}", self.has_updates.load(std::sync::atomic::Ordering::SeqCst));

        Ok(())
    }

    /// Stop Neovim process
    pub fn stop(&mut self) {
        let neovim_arc = self.neovim.clone();
        self.runtime.block_on(async {
            let mut nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.take() {
                let _ = neovim.command("qa!").await;
            }
        });
        godot::global::godot_print!("[godot-neovim] Neovim stopped");
    }

    /// Get current mode from Neovim directly with timeout
    /// Returns (mode, blocking) tuple
    pub fn get_mode(&self) -> (String, bool) {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            // Use timeout to avoid blocking on operator-pending commands like 'g'
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(50),
                async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        neovim.get_mode().await.ok()
                    } else {
                        None
                    }
                }
            ).await;

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

    /// Set buffer content
    pub fn set_buffer_lines(
        &self,
        start: i64,
        end: i64,
        lines: Vec<String>,
    ) -> Result<(), String> {
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
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(50),
                async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        let window = neovim.get_current_win().await.ok()?;
                        window.get_cursor().await.ok()
                    } else {
                        None
                    }
                }
            ).await;

            match result {
                Ok(Some(pos)) => Ok(pos),
                Ok(None) => Err("Failed to get cursor".to_string()),
                Err(_) => Err("Timeout getting cursor".to_string()),
            }
        })
    }

    /// Set cursor position
    #[allow(dead_code)]
    pub fn set_cursor(&self, line: i64, col: i64) -> Result<(), String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                let window = neovim
                    .get_current_win()
                    .await
                    .map_err(|e| format!("Failed to get window: {}", e))?;
                window
                    .set_cursor((line, col))
                    .await
                    .map_err(|e| format!("Failed to set cursor: {}", e))?;
                Ok(())
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
    }
}

/// Create Neovim command with platform-specific settings
fn create_nvim_command(nvim_path: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut std_cmd = std::process::Command::new(nvim_path);
        std_cmd
            .args(["--embed", "--headless"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);
        Command::from(std_cmd)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = Command::new(nvim_path);
        cmd.args(["--embed", "--headless"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }
}
