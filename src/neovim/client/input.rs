//! Key input: input, send_keys, channels

use super::{NeovimClient, RPC_TIMEOUT_MS};

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
}
