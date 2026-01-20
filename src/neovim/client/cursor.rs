//! Cursor and visual selection operations

use super::{NeovimClient, RPC_TIMEOUT_MS};
use rmpv::Value;

impl NeovimClient {
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

    /// Set visual selection atomically via Lua
    /// This ensures cursor movement and visual mode entry happen in the correct order
    /// @param from_line: Selection start line (1-indexed)
    /// @param from_col: Selection start column (0-indexed)
    /// @param to_line: Selection end line (1-indexed)
    /// @param to_col: Selection end column (0-indexed)
    pub fn set_visual_selection(
        &self,
        from_line: i64,
        from_col: i64,
        to_line: i64,
        to_col: i64,
    ) -> Result<String, String> {
        let neovim_arc = self.neovim.clone();

        self.runtime.block_on(async {
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(RPC_TIMEOUT_MS), async {
                    let nvim_lock = neovim_arc.lock().await;
                    if let Some(neovim) = nvim_lock.as_ref() {
                        let result = neovim
                            .exec_lua(
                                "return _G.godot_neovim.set_visual_selection(...)",
                                vec![
                                    Value::from(from_line),
                                    Value::from(from_col),
                                    Value::from(to_line),
                                    Value::from(to_col),
                                ],
                            )
                            .await
                            .map_err(|e| format!("Failed to set visual selection: {}", e))?;

                        // Parse result { mode }
                        if let Value::Map(map) = result {
                            for (k, v) in map {
                                if let Value::String(key) = k {
                                    if key.as_str() == Some("mode") {
                                        if let Value::String(m) = v {
                                            return Ok(m.as_str().unwrap_or("v").to_string());
                                        }
                                    }
                                }
                            }
                        }
                        Ok("v".to_string())
                    } else {
                        Err("Neovim not connected".to_string())
                    }
                })
                .await;

            match result {
                Ok(inner) => inner,
                Err(_) => Err("Timeout setting visual selection".to_string()),
            }
        })
    }
}
