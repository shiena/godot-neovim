use nvim_rs::Handler;
use rmpv::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Handler for Neovim RPC notifications and requests
#[derive(Clone)]
pub struct NeovimHandler {
    /// Current Neovim mode (n, i, v, etc.)
    pub mode: Arc<Mutex<String>>,
}

impl NeovimHandler {
    pub fn new() -> Self {
        Self {
            mode: Arc::new(Mutex::new("n".to_string())),
        }
    }

    pub async fn get_mode(&self) -> String {
        self.mode.lock().await.clone()
    }

    async fn handle_redraw(&self, args: Vec<Value>) {
        for arg in args {
            if let Value::Array(events) = arg {
                for event in events {
                    if let Value::Array(event_data) = event {
                        if let Some(Value::String(event_name)) = event_data.first() {
                            match event_name.as_str() {
                                Some("mode_change") => {
                                    self.handle_mode_change(&event_data).await;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    async fn handle_mode_change(&self, event_data: &[Value]) {
        // mode_change event: ["mode_change", [mode_name, mode_idx]]
        if let Some(Value::Array(mode_info)) = event_data.get(1) {
            if let Some(Value::String(mode_name)) = mode_info.first() {
                if let Some(mode_str) = mode_name.as_str() {
                    let mut mode = self.mode.lock().await;
                    *mode = mode_str.to_string();
                    godot::global::godot_print!("[godot-neovim] Mode changed: {}", mode_str);
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
        match name.as_str() {
            "redraw" => {
                self.handle_redraw(args).await;
            }
            _ => {
                godot::global::godot_print!("[godot-neovim] Unhandled notification: {}", name);
            }
        }
    }

    async fn handle_request(
        &self,
        name: String,
        _args: Vec<Value>,
        _neovim: nvim_rs::Neovim<Self::Writer>,
    ) -> Result<Value, Value> {
        godot::global::godot_print!("[godot-neovim] Received request: {}", name);
        Ok(Value::Nil)
    }
}
