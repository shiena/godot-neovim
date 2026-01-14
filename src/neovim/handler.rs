use super::events::RedrawEvent;
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
    #[allow(dead_code)]
    pub fn take_updates(&self) -> bool {
        self.has_updates.swap(false, Ordering::SeqCst)
    }

    async fn handle_redraw(&self, args: Vec<Value>) {
        let mut state = self.state.lock().await;
        let mut updated = false;

        for arg in args {
            if let Value::Array(raw_events) = arg {
                for raw_event in raw_events {
                    if let Value::Array(event_data) = raw_event {
                        // Use typed event parsing
                        match RedrawEvent::parse(&event_data) {
                            Ok(events) => {
                                for event in events {
                                    match event {
                                        RedrawEvent::ModeChange { mode, .. } => {
                                            state.mode = mode;
                                            updated = true;
                                        }
                                        RedrawEvent::GridCursorGoto { grid, row, col } => {
                                            state.cursor_grid = grid as i64;
                                            state.cursor = (row as i64, col as i64);
                                            updated = true;
                                        }
                                        RedrawEvent::Flush => {
                                            // Flush signals end of redraw batch
                                            if updated {
                                                self.has_updates.store(true, Ordering::SeqCst);
                                            }
                                        }
                                        RedrawEvent::Unknown(_) => {
                                            // Ignore unknown events
                                        }
                                    }
                                }
                            }
                            Err(_) => {
                                // Silently ignore parse errors for now
                                // (Cannot log here - runs on tokio worker thread)
                            }
                        }
                    }
                }
            }
        }
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
