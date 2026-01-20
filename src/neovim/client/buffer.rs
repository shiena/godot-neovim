//! Buffer operations: buffer_update, switch_to_buffer, attach

use super::{NeovimClient, SwitchBufferResult, RPC_EXTENDED_TIMEOUT_MS, RPC_TIMEOUT_MS};
use rmpv::Value;

impl NeovimClient {
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
    ) -> std::sync::Arc<tokio::sync::Mutex<std::collections::VecDeque<crate::neovim::BufEvent>>>
    {
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
