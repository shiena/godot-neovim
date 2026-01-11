use crate::neovim::NeovimHandler;
use crate::settings;
use nvim_rs::create::tokio as create;
use nvim_rs::{Neovim, UiAttachOptions};
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::runtime::Runtime;
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
}

impl NeovimClient {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let runtime = Runtime::new()?;
        let nvim_path = settings::get_neovim_path();
        Ok(Self {
            runtime,
            neovim: Arc::new(Mutex::new(None)),
            handler: NeovimHandler::new(),
            nvim_path,
        })
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

        self.runtime.block_on(async {
            let mut cmd = create_nvim_command(&nvim_path);

            let (neovim, _io_handler, _child) = create::new_child_cmd(&mut cmd, handler).await?;

            // Attach UI to receive redraw events
            let mut ui_opts = UiAttachOptions::new();
            ui_opts.set_linegrid_external(true);
            neovim
                .ui_attach(80, 24, &ui_opts)
                .await
                .map_err(|e| format!("Failed to attach UI: {}", e))?;

            let mut nvim_lock = neovim_arc.lock().await;
            *nvim_lock = Some(neovim);

            godot::global::godot_print!("[godot-neovim] Neovim started successfully");
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        })?;

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

    /// Get current mode
    pub fn get_mode(&self) -> String {
        self.runtime.block_on(self.handler.get_mode())
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

    /// Get cursor position (1-indexed line, 0-indexed column)
    pub fn get_cursor(&self) -> Result<(i64, i64), String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let nvim_lock = neovim_arc.lock().await;
            if let Some(neovim) = nvim_lock.as_ref() {
                let window = neovim
                    .get_current_win()
                    .await
                    .map_err(|e| format!("Failed to get window: {}", e))?;
                let pos = window
                    .get_cursor()
                    .await
                    .map_err(|e| format!("Failed to get cursor: {}", e))?;
                Ok(pos)
            } else {
                Err("Neovim not connected".to_string())
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
