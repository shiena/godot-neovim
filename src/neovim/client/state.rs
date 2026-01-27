//! State management: poll, take_state, viewport

use super::NeovimClient;
use std::sync::atomic::Ordering;

impl NeovimClient {
    /// Take pending updates (clears the flag) and return current state
    /// Prefers actual_cursor (from CursorMoved autocmd) over grid cursor (from redraw)
    /// because actual_cursor is byte position, while grid cursor is screen position
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

    /// Take pending debug messages from Lua
    /// Returns empty Vec if no messages
    pub fn take_debug_messages(&self) -> Vec<String> {
        self.runtime.block_on(async {
            let mut state = self.state.lock().await;
            std::mem::take(&mut state.debug_messages)
        })
    }
}
