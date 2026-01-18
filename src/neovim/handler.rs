use super::events::RedrawEvent;
use crate::sync::BufLinesEvent;
use nvim_rs::Handler;
use rmpv::Value;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state between handler and plugin
#[derive(Debug, Default)]
pub struct NeovimState {
    /// Current mode (n, i, v, etc.)
    pub mode: String,
    /// Cursor position (line, col) - 0-indexed (from grid_cursor_goto, screen position)
    pub cursor: (i64, i64),
    /// Grid ID for cursor
    pub cursor_grid: i64,
    /// Actual cursor position (line, col) - line is 0-indexed, col is byte position
    /// This comes from CursorMoved autocmd and is the true buffer position
    pub actual_cursor: Option<(i64, i64)>,
}

/// Buffer events from nvim_buf_attach
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum BufEvent {
    /// Buffer lines changed
    Lines(BufLinesEvent),
    /// Only changedtick updated (no content change)
    ChangedTick { buf: i64, tick: i64 },
    /// Buffer detached
    Detach { buf: i64 },
    /// Buffer modified flag changed (from BufModifiedSet autocmd)
    ModifiedChanged { buf: i64, modified: bool },
}

/// Handler for Neovim RPC notifications and requests
#[derive(Clone)]
pub struct NeovimHandler {
    /// Shared state updated by redraw events
    state: Arc<Mutex<NeovimState>>,
    /// Flag indicating new updates are available
    has_updates: Arc<AtomicBool>,
    /// Buffer events queue (from nvim_buf_attach)
    buf_events: Arc<Mutex<VecDeque<BufEvent>>>,
    /// Flag indicating new buffer events are available
    has_buf_events: Arc<AtomicBool>,
}

impl NeovimHandler {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(NeovimState {
                mode: "n".to_string(),
                cursor: (0, 0),
                cursor_grid: 1,
                actual_cursor: None,
            })),
            has_updates: Arc::new(AtomicBool::new(false)),
            buf_events: Arc::new(Mutex::new(VecDeque::new())),
            has_buf_events: Arc::new(AtomicBool::new(false)),
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

    /// Get a clone of the buffer events queue
    pub fn get_buf_events(&self) -> Arc<Mutex<VecDeque<BufEvent>>> {
        self.buf_events.clone()
    }

    /// Get a clone of the has_buf_events flag
    pub fn get_buf_events_flag(&self) -> Arc<AtomicBool> {
        self.has_buf_events.clone()
    }

    /// Check and clear the updates flag
    #[allow(dead_code)]
    pub fn take_updates(&self) -> bool {
        self.has_updates.swap(false, Ordering::SeqCst)
    }

    /// Parse nvim_buf_lines_event notification
    async fn handle_buf_lines_event(&self, args: Vec<Value>) {
        // args: [buf, changedtick, firstline, lastline, linedata, more]
        if args.len() < 6 {
            return;
        }

        let buf = match &args[0] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            Value::Ext(_, data) => {
                // Buffer type is ext(0, data)
                if !data.is_empty() {
                    data[0] as i64
                } else {
                    0
                }
            }
            _ => return,
        };

        let changedtick = match &args[1] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            _ => return,
        };

        let first_line = match &args[2] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            _ => return,
        };

        let last_line = match &args[3] {
            Value::Integer(i) => i.as_i64().unwrap_or(-1),
            _ => return,
        };

        let line_data: Vec<String> = match &args[4] {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| {
                    if let Value::String(s) = v {
                        s.as_str().map(String::from)
                    } else {
                        None
                    }
                })
                .collect(),
            _ => return,
        };

        let more = match &args[5] {
            Value::Boolean(b) => *b,
            _ => false,
        };

        let event = BufLinesEvent {
            buf,
            changedtick,
            first_line,
            last_line,
            line_data,
            more,
        };

        let mut events = self.buf_events.lock().await;
        events.push_back(BufEvent::Lines(event));
        self.has_buf_events.store(true, Ordering::SeqCst);
    }

    /// Parse godot_cursor_moved notification from Lua CursorMoved autocmd
    /// args: [line, col, mode] - line is 1-indexed, col is 0-indexed byte position
    async fn handle_godot_cursor_moved(&self, args: Vec<Value>) {
        if args.len() < 3 {
            return;
        }

        let line = match &args[0] {
            Value::Integer(i) => i.as_i64().unwrap_or(1),
            _ => return,
        };

        let col = match &args[1] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            _ => return,
        };

        let mode = match &args[2] {
            Value::String(s) => s.as_str().unwrap_or("n").to_string(),
            _ => "n".to_string(),
        };

        // Update state with actual cursor position (convert to 0-indexed line)
        let mut state = self.state.lock().await;
        state.actual_cursor = Some((line - 1, col));
        state.mode = mode;
        self.has_updates.store(true, Ordering::SeqCst);
    }

    /// Parse godot_buf_lines notification from Lua on_lines callback
    /// args: [buf, tick, first_line, last_line, new_lines]
    async fn handle_godot_buf_lines(&self, args: Vec<Value>) {
        if args.len() < 5 {
            return;
        }

        let buf = match &args[0] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            _ => return,
        };

        let changedtick = match &args[1] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            _ => return,
        };

        let first_line = match &args[2] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            _ => return,
        };

        let last_line = match &args[3] {
            Value::Integer(i) => i.as_i64().unwrap_or(-1),
            _ => return,
        };

        let line_data: Vec<String> = match &args[4] {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| {
                    if let Value::String(s) = v {
                        s.as_str().map(String::from)
                    } else {
                        None
                    }
                })
                .collect(),
            _ => return,
        };

        let event = BufLinesEvent {
            buf,
            changedtick,
            first_line,
            last_line,
            line_data,
            more: false,
        };

        let mut events = self.buf_events.lock().await;
        events.push_back(BufEvent::Lines(event));
        self.has_buf_events.store(true, Ordering::SeqCst);
    }

    /// Parse nvim_buf_changedtick_event notification
    async fn handle_buf_changedtick_event(&self, args: Vec<Value>) {
        // args: [buf, changedtick]
        if args.len() < 2 {
            return;
        }

        let buf = match &args[0] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            Value::Ext(_, data) => {
                if !data.is_empty() {
                    data[0] as i64
                } else {
                    0
                }
            }
            _ => return,
        };

        let tick = match &args[1] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            _ => return,
        };

        let mut events = self.buf_events.lock().await;
        events.push_back(BufEvent::ChangedTick { buf, tick });
        self.has_buf_events.store(true, Ordering::SeqCst);
    }

    /// Parse nvim_buf_detach_event notification
    async fn handle_buf_detach_event(&self, args: Vec<Value>) {
        // args: [buf]
        if args.is_empty() {
            return;
        }

        let buf = match &args[0] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            Value::Ext(_, data) => {
                if !data.is_empty() {
                    data[0] as i64
                } else {
                    0
                }
            }
            _ => return,
        };

        let mut events = self.buf_events.lock().await;
        events.push_back(BufEvent::Detach { buf });
        self.has_buf_events.store(true, Ordering::SeqCst);
    }

    /// Parse godot_modified_changed notification from Lua BufModifiedSet autocmd
    /// args: [buf, modified]
    async fn handle_godot_modified_changed(&self, args: Vec<Value>) {
        if args.len() < 2 {
            return;
        }

        let buf = match &args[0] {
            Value::Integer(i) => i.as_i64().unwrap_or(0),
            _ => return,
        };

        let modified = match &args[1] {
            Value::Boolean(b) => *b,
            _ => return,
        };

        let mut events = self.buf_events.lock().await;
        events.push_back(BufEvent::ModifiedChanged { buf, modified });
        self.has_buf_events.store(true, Ordering::SeqCst);
    }

    async fn handle_redraw(&self, args: Vec<Value>) {
        let mut state = self.state.lock().await;

        // Neovim redraw format: ["redraw", ["event_name", args...], ["event_name2", args...], ...]
        // Each arg is an event: ["event_name", [batch1_args], [batch2_args], ...]
        for arg in args {
            if let Value::Array(event_data) = arg {
                // event_data = ["event_name", args1, args2, ...]
                // Use typed event parsing
                if let Ok(events) = RedrawEvent::parse(&event_data) {
                    for event in events {
                        match event {
                            RedrawEvent::ModeChange { mode, .. } => {
                                state.mode = mode;
                                self.has_updates.store(true, Ordering::SeqCst);
                            }
                            RedrawEvent::GridCursorGoto { grid, row, col } => {
                                state.cursor_grid = grid as i64;
                                state.cursor = (row as i64, col as i64);
                                self.has_updates.store(true, Ordering::SeqCst);
                            }
                            RedrawEvent::Flush | RedrawEvent::Unknown(_) => {
                                // Flush: No longer needed since we set flag immediately
                                // Unknown: Silently ignore unhandled events
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
        match name.as_str() {
            "redraw" => self.handle_redraw(args).await,
            "nvim_buf_lines_event" => self.handle_buf_lines_event(args).await,
            "nvim_buf_changedtick_event" => self.handle_buf_changedtick_event(args).await,
            "nvim_buf_detach_event" => self.handle_buf_detach_event(args).await,
            "godot_buf_lines" => self.handle_godot_buf_lines(args).await,
            "godot_cursor_moved" => self.handle_godot_cursor_moved(args).await,
            "godot_modified_changed" => self.handle_godot_modified_changed(args).await,
            _ => {}
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
