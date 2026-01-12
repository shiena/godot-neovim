use nvim_rs::Handler;
use rmpv::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state between handler and plugin
#[derive(Debug, Default)]
pub struct NeovimState {
    /// Current mode (n, i, v, etc.)
    pub mode: String,
    /// Cursor position (line, col) - 0-indexed
    pub cursor: (i64, i64),
    /// Grid ID for cursor
    pub cursor_grid: i64,
}

/// Handler for Neovim RPC notifications and requests
#[derive(Clone)]
pub struct NeovimHandler {
    /// Shared state updated by redraw events
    state: Arc<Mutex<NeovimState>>,
    /// Flag indicating new updates are available
    has_updates: Arc<AtomicBool>,
}

impl NeovimHandler {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(NeovimState {
                mode: "n".to_string(),
                cursor: (0, 0),
                cursor_grid: 1,
            })),
            has_updates: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a clone of the state Arc for sharing with plugin
    pub fn get_state(&self) -> Arc<Mutex<NeovimState>> {
        self.state.clone()
    }

    /// Get a clone of the has_updates flag for sharing with plugin
    pub fn get_updates_flag(&self) -> Arc<AtomicBool> {
        self.has_updates.clone()
    }

    /// Check and clear the updates flag
    pub fn take_updates(&self) -> bool {
        self.has_updates.swap(false, Ordering::SeqCst)
    }

    async fn handle_redraw(&self, args: Vec<Value>) {
        let mut state = self.state.lock().await;
        let mut updated = false;

        for arg in args {
            if let Value::Array(events) = arg {
                for event in events {
                    if let Value::Array(event_data) = event {
                        if let Some(Value::String(event_name)) = event_data.first() {
                            match event_name.as_str() {
                                Some("mode_change") => {
                                    if self.handle_mode_change(&mut state, &event_data) {
                                        updated = true;
                                    }
                                }
                                Some("grid_cursor_goto") => {
                                    if self.handle_cursor_goto(&mut state, &event_data) {
                                        updated = true;
                                    }
                                }
                                Some("flush") => {
                                    // Flush signals end of redraw batch
                                    if updated {
                                        self.has_updates.store(true, Ordering::SeqCst);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    fn handle_mode_change(&self, state: &mut NeovimState, event_data: &[Value]) -> bool {
        // mode_change event: ["mode_change", [mode_name, mode_idx], ...]
        // Can have multiple mode changes in one event
        for i in 1..event_data.len() {
            if let Some(Value::Array(mode_info)) = event_data.get(i) {
                if let Some(Value::String(mode_name)) = mode_info.first() {
                    if let Some(mode_str) = mode_name.as_str() {
                        state.mode = mode_str.to_string();
                        return true;
                    }
                }
            }
        }
        false
    }

    fn handle_cursor_goto(&self, state: &mut NeovimState, event_data: &[Value]) -> bool {
        // grid_cursor_goto event: ["grid_cursor_goto", [grid, row, col], ...]
        for i in 1..event_data.len() {
            if let Some(Value::Array(cursor_info)) = event_data.get(i) {
                if cursor_info.len() >= 3 {
                    if let (Some(grid), Some(row), Some(col)) = (
                        cursor_info.first().and_then(|v| v.as_i64()),
                        cursor_info.get(1).and_then(|v| v.as_i64()),
                        cursor_info.get(2).and_then(|v| v.as_i64()),
                    ) {
                        state.cursor_grid = grid;
                        state.cursor = (row, col);
                        return true;
                    }
                }
            }
        }
        false
    }
}

impl Default for NeovimHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Handler for NeovimHandler {
    type Writer = nvim_rs::compat::tokio::Compat<tokio::process::ChildStdin>;

    async fn handle_notify(
        &self,
        name: String,
        args: Vec<Value>,
        _neovim: nvim_rs::Neovim<Self::Writer>,
    ) {
        // Note: Cannot use godot_print! here - this runs on tokio worker thread
        if name.as_str() == "redraw" {
            self.handle_redraw(args).await;
        }
    }

    async fn handle_request(
        &self,
        _name: String,
        _args: Vec<Value>,
        _neovim: nvim_rs::Neovim<Self::Writer>,
    ) -> Result<Value, Value> {
        // Note: Cannot use godot_print! here - this runs on tokio worker thread
        Ok(Value::Nil)
    }
}
