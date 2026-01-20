//! Key input: input, send_keys, channels

use super::{NeovimClient, RPC_TIMEOUT_MS};
use rmpv::Value;

impl NeovimClient {
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
    pub fn send_serial(&self, cmd: crate::neovim::SerialCommand) {
        use crate::neovim::SerialCommand;

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
}
