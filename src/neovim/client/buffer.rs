//! Buffer operations: buffer_update, switch_to_buffer, attach

use super::{
    ExternalSyncResult, ExternalSyncStatus, IndentOptions, NeovimClient, SwitchBufferResult,
    RPC_EXTENDED_TIMEOUT_MS, RPC_TIMEOUT_MS,
};
use rmpv::Value;

impl NeovimClient {
    /// Update buffer content (preserves undo history for 'u' command)
    /// Uses Lua function to properly manage undo history
    pub fn buffer_update(&self, lines: Vec<String>) -> Result<i64, String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let nvim = neovim_arc.lock().await.as_ref().cloned();
            if let Some(neovim) = nvim.as_ref() {
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

    /// Apply an edit Godot made on its own to the current buffer, replacing
    /// only the changed line range (see buffer.lua `sync_external_lines`).
    /// With `expected_tick`, nothing is changed if Neovim's changedtick
    /// differs (Neovim has edits that Godot has not applied yet).
    pub fn sync_external_lines(
        &self,
        lines: Vec<String>,
        expected_tick: Option<i64>,
        unmodified: bool,
    ) -> Result<ExternalSyncResult, String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(RPC_EXTENDED_TIMEOUT_MS),
                async {
                    let nvim = neovim_arc.lock().await.as_ref().cloned();
                    let Some(neovim) = nvim.as_ref() else {
                        return Err("Neovim not connected".to_string());
                    };
                    let mut opts = vec![(Value::from("unmodified"), Value::from(unmodified))];
                    if let Some(tick) = expected_tick {
                        opts.push((Value::from("expected_tick"), Value::from(tick)));
                    }
                    let lines_value = Value::Array(lines.into_iter().map(Value::from).collect());
                    let args = vec![Value::from(0i64), lines_value, Value::Map(opts)];
                    let result = neovim
                        .exec_lua("return _G.godot_neovim.sync_external_lines(...)", args)
                        .await
                        .map_err(|e| format!("Failed to sync external edit: {}", e))?;
                    Self::parse_external_sync_result(result)
                },
            )
            .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err("Timeout syncing external edit".to_string()),
            }
        })
    }

    fn parse_external_sync_result(result: Value) -> Result<ExternalSyncResult, String> {
        let Value::Map(map) = result else {
            return Err("Expected table result from sync_external_lines".to_string());
        };
        let mut status = None;
        let mut tick = None;
        let mut line_count = None;
        for (key, value) in map {
            match key.as_str() {
                Some("status") => {
                    status = match value.as_str() {
                        Some("unchanged") => Some(ExternalSyncStatus::Unchanged),
                        Some("updated") => Some(ExternalSyncStatus::Updated),
                        Some("stale") => Some(ExternalSyncStatus::Stale),
                        _ => None,
                    }
                }
                Some("tick") => tick = value.as_i64(),
                Some("line_count") => line_count = value.as_i64(),
                _ => {}
            }
        }
        match (status, tick, line_count) {
            (Some(status), Some(tick), Some(line_count)) => Ok(ExternalSyncResult {
                status,
                tick,
                line_count,
            }),
            _ => Err("Invalid result from sync_external_lines".to_string()),
        }
    }

    /// Switch to buffer by path, creating and initializing if needed.
    /// For an existing buffer, `lines` that differ from Neovim's content are
    /// applied as an external edit (`content_updated`); `unmodified` then
    /// clears Neovim's 'modified' flag because Godot has saved the file.
    pub fn switch_to_buffer(
        &self,
        path: &str,
        lines: Option<Vec<String>>,
        indent_opts: Option<IndentOptions>,
        unmodified: bool,
    ) -> Result<SwitchBufferResult, String> {
        let neovim_arc = self.neovim.clone();
        let path = path.to_string();

        self.runtime.block_on(async {
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(RPC_EXTENDED_TIMEOUT_MS),
                async {
                    let nvim = neovim_arc.lock().await.as_ref().cloned();
                    if let Some(neovim) = nvim.as_ref() {
                        // Prepare arguments
                        let lines_value = match lines {
                            Some(l) => Value::Array(l.into_iter().map(Value::from).collect()),
                            None => Value::Nil,
                        };

                        // Prepare arguments - only include indent_opts if provided
                        // (msgpack nil may not convert to Lua nil properly)
                        let args = match indent_opts {
                            Some(opts) => {
                                let map = vec![
                                    (Value::from("use_spaces"), Value::from(opts.use_spaces)),
                                    (
                                        Value::from("indent_size"),
                                        Value::from(opts.indent_size as i64),
                                    ),
                                ];
                                vec![
                                    Value::from(path),
                                    lines_value,
                                    Value::Map(map),
                                    Value::from(unmodified),
                                ]
                            }
                            // `false` stands in for "no indent options"
                            None => vec![
                                Value::from(path),
                                lines_value,
                                Value::from(false),
                                Value::from(unmodified),
                            ],
                        };

                        let result = neovim
                            .exec_lua("return _G.godot_neovim.switch_to_buffer(...)", args)
                            .await
                            .map_err(|e| format!("Failed to switch buffer: {}", e))?;

                        // Parse result table
                        // { bufnr, tick, is_new, content_updated, line_count, attached, cursor }
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

    /// Set indent options for the current buffer
    pub fn set_indent_options(&self, use_spaces: bool, indent_size: i32) -> Result<(), String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let nvim = neovim_arc.lock().await.as_ref().cloned();
            if let Some(neovim) = nvim.as_ref() {
                let args = vec![
                    Value::from(0i64), // current buffer
                    Value::from(use_spaces),
                    Value::from(indent_size as i64),
                ];

                neovim
                    .exec_lua("_G.godot_neovim.set_indent_options(...)", args)
                    .await
                    .map_err(|e| format!("Failed to set indent options: {}", e))?;

                Ok(())
            } else {
                Err("Neovim not connected".to_string())
            }
        })
    }

    /// Parse the result from switch_to_buffer Lua function
    fn parse_switch_buffer_result(result: rmpv::Value) -> Result<SwitchBufferResult, String> {
        if let Value::Map(map) = result {
            let mut bufnr: Option<i64> = None;
            let mut tick: Option<i64> = None;
            let mut is_new: Option<bool> = None;
            let mut content_updated = false;
            let mut line_count: Option<i64> = None;
            let mut attached: Option<bool> = None;
            let mut cursor: Option<(i64, i64)> = None;

            for (key, value) in map {
                if let Value::String(k) = key {
                    match k.as_str() {
                        Some("bufnr") => bufnr = value.as_i64(),
                        Some("tick") => tick = value.as_i64(),
                        Some("is_new") => is_new = value.as_bool(),
                        Some("content_updated") => {
                            content_updated = value.as_bool().unwrap_or(false)
                        }
                        Some("line_count") => line_count = value.as_i64(),
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
                    content_updated,
                    line_count,
                    attached: a,
                    cursor: c,
                }),
                _ => Err("Invalid result from switch_to_buffer".to_string()),
            }
        } else {
            Err("Expected table result from switch_to_buffer".to_string())
        }
    }

    /// Attach to current buffer for change notifications
    pub fn buf_attach_current(&self) -> Result<bool, String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim = neovim_arc.lock().await.as_ref().cloned();
                    if let Some(neovim) = nvim.as_ref() {
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
