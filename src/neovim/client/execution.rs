//! Command and Lua execution

use super::{NeovimClient, RPC_TIMEOUT_MS};

impl NeovimClient {
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
