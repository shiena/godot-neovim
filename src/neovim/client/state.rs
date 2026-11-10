//! Shared state and viewport updates

use super::NeovimClient;
use std::sync::atomic::Ordering;

impl NeovimClient {
    /// Take pending updates (clears the flag) and return current state
    /// Keep buffer coordinates across redraws that only update the screen grid.
    pub fn take_state(&self) -> Option<(String, (i64, i64))> {
        if self.rpc_timed_out() {
            return None;
        }
        let state = self.state.try_lock().ok()?;
        if !self.has_updates.swap(false, Ordering::SeqCst) {
            return None;
        }
        let cursor = state.actual_cursor.unwrap_or(state.cursor);
        Some((state.mode.clone(), cursor))
    }

    pub fn take_viewport(&self) -> Option<(i64, i64, i64, i64)> {
        if self.rpc_timed_out() {
            return None;
        }
        let mut state = self.state.try_lock().ok()?;
        if !state.viewport_changed {
            return None;
        }
        state.viewport_changed = false;
        Some((
            state.viewport_topline,
            state.viewport_botline,
            state.viewport_curline,
            state.viewport_curcol,
        ))
    }

    pub fn force_viewport_changed(&self) {
        let state = self.state.clone();
        self.runtime.spawn(async move {
            state.lock().await.viewport_changed = true;
        });
    }

    /// Resize Neovim's UI to match Godot editor's visible area
    /// This is important for viewport commands (zz, zt, zb) to work correctly
    pub fn ui_try_resize(&self, width: i64, height: i64) {
        if self.rpc_timed_out.load(Ordering::Acquire) {
            return;
        }
        if self.commands_pending() {
            self.pending_resize.set(Some((width, height)));
            return;
        }
        self.pending_resize.set(None);
        let neovim_arc = self.neovim.clone();

        let result = self.wait_for_rpc("resizing UI", async {
            let nvim = neovim_arc.lock().await.as_ref().cloned();
            let Some(neovim) = nvim else {
                return Err(super::ClientError::NotConnected);
            };
            neovim
                .ui_try_resize(width, height)
                .await
                .map_err(|e| super::ClientError::rpc("resize UI", e))
        });
        if let Err(error) = result {
            crate::verbose_print!("[godot-neovim] Failed to resize UI: {}", error);
        }
    }

    /// Number of keys Neovim's main loop has confirmed processing since the
    /// last call (see keys_processed in mod.rs for what "confirmed" means).
    /// The baseline is stored on this client, so counters of different Neovim
    /// instances (script vs shader, or a restarted client) are never compared
    /// against each other. A non-zero delta proves this instance is alive even
    /// without any visible redraw — e.g. a no-op motion at the end of the file
    /// (#75) — and the caller can subtract it from its pending-key count.
    pub fn take_keys_processed_delta(&self) -> u64 {
        let current = self.keys_processed.load(Ordering::SeqCst);
        let seen = self.keys_processed_seen.swap(current, Ordering::SeqCst);
        current.saturating_sub(seen)
    }

    /// Take pending debug messages from Lua
    /// Returns empty Vec if no messages
    pub fn take_debug_messages(&self) -> Vec<String> {
        self.state
            .try_lock()
            .map(|mut state| std::mem::take(&mut state.debug_messages))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neovim::NeovimHandler;
    use std::sync::{atomic::AtomicU64, Arc};
    use tokio::sync::Mutex;

    fn client() -> NeovimClient {
        let handler = NeovimHandler::new();
        NeovimClient {
            commands: None,
            rpc_timed_out: std::sync::atomic::AtomicBool::new(false),
            pending_resize: std::cell::Cell::new(None),
            runtime: tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap(),
            child: None,
            neovim: Arc::new(Mutex::new(None)),
            state: handler.get_state(),
            has_updates: handler.get_updates_flag(),
            handler,
            nvim_path: String::new(),
            clean: true,
            io_handle: None,
            stderr_handle: None,
            key_input_tx: None,
            key_input_handle: None,
            keys_processed: Arc::new(AtomicU64::new(0)),
            keys_processed_seen: AtomicU64::new(0),
        }
    }

    #[test]
    fn grid_only_redraw_keeps_the_last_buffer_cursor() {
        let client = client();
        client.state.blocking_lock().actual_cursor = Some((1, 0));
        client.has_updates.store(true, Ordering::SeqCst);
        assert_eq!(client.take_state().unwrap().1, (1, 0));

        // A line-number gutter shifts the screen column, not the buffer column.
        client.state.blocking_lock().cursor = (1, 4);
        client.has_updates.store(true, Ordering::SeqCst);
        assert_eq!(client.take_state().unwrap().1, (1, 0));
    }

    #[test]
    fn contended_state_preserves_the_update_for_the_next_frame() {
        let client = client();
        let state = client.state.blocking_lock();
        client.has_updates.store(true, Ordering::SeqCst);
        assert!(client.take_state().is_none());
        drop(state);
        assert!(client.take_state().is_some());
        assert!(client.take_state().is_none());
    }
}
