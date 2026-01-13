use lsp_types::{
    GotoDefinitionParams, GotoDefinitionResponse, InitializeParams, InitializeResult,
    Location, Position, TextDocumentIdentifier, TextDocumentPositionParams, Url,
    DidOpenTextDocumentParams, TextDocumentItem,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: i64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcNotification {
    jsonrpc: &'static str,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<i64>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

pub struct GodotLspClient {
    stream: Mutex<Option<TcpStream>>,
    request_id: AtomicI64,
    initialized: std::sync::atomic::AtomicBool,
}

impl GodotLspClient {
    pub fn new() -> Self {
        Self {
            stream: Mutex::new(None),
            request_id: AtomicI64::new(1),
            initialized: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn connect(&self, port: u16) -> Result<(), String> {
        let addr = format!("127.0.0.1:{}", port);
        let stream = TcpStream::connect(&addr)
            .map_err(|e| format!("Failed to connect to LSP server at {}: {}", addr, e))?;

        // Set read timeout - longer for initialization
        stream.set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| format!("Failed to set timeout: {}", e))?;

        *self.stream.lock().unwrap() = Some(stream);
        Ok(())
    }

    /// Disconnect from LSP server
    pub fn disconnect(&self) {
        if let Ok(mut guard) = self.stream.lock() {
            if let Some(stream) = guard.take() {
                // Shutdown the stream to unblock any pending reads
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
        self.initialized.store(false, Ordering::SeqCst);
        crate::verbose_print!("[godot-neovim] LSP disconnected");
    }

    pub fn initialize(&self, root_uri: &str) -> Result<(), String> {
        if self.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Small delay to let the server prepare after connection
        std::thread::sleep(Duration::from_millis(100));

        let params = InitializeParams {
            root_uri: Some(Url::parse(root_uri).map_err(|e| e.to_string())?),
            capabilities: lsp_types::ClientCapabilities::default(),
            ..Default::default()
        };

        crate::verbose_print!("[godot-neovim] LSP: Sending initialize request...");

        let _result: InitializeResult = self
            .send_request("initialize", Some(serde_json::to_value(params).unwrap()))?;

        crate::verbose_print!("[godot-neovim] LSP: Initialize response received");

        // Send initialized notification
        self.send_notification("initialized", Some(json!({})))?;

        self.initialized.store(true, Ordering::SeqCst);
        crate::verbose_print!("[godot-neovim] LSP: Initialization complete");
        Ok(())
    }

    pub fn did_open(&self, uri: &str, text: &str) -> Result<(), String> {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Url::parse(uri).map_err(|e| e.to_string())?,
                language_id: "gdscript".to_string(),
                version: 1,
                text: text.to_string(),
            },
        };

        self.send_notification(
            "textDocument/didOpen",
            Some(serde_json::to_value(params).unwrap()),
        )
    }

    pub fn goto_definition(
        &self,
        uri: &str,
        line: u32,
        col: u32,
    ) -> Result<Option<Location>, String> {
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).map_err(|e| e.to_string())?,
                },
                position: Position {
                    line,
                    character: col,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result: Option<GotoDefinitionResponse> = self
            .send_request(
                "textDocument/definition",
                Some(serde_json::to_value(params).unwrap()),
            )?;

        match result {
            Some(GotoDefinitionResponse::Scalar(loc)) => Ok(Some(loc)),
            Some(GotoDefinitionResponse::Array(locs)) if !locs.is_empty() => {
                Ok(Some(locs[0].clone()))
            }
            Some(GotoDefinitionResponse::Link(links)) if !links.is_empty() => {
                let link = &links[0];
                let loc = Location {
                    uri: link.target_uri.clone(),
                    range: link.target_selection_range,
                };
                Ok(Some(loc))
            }
            _ => Ok(None),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.stream.lock().unwrap().is_some()
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }

    fn send_request<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<T, String> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        crate::verbose_print!("[godot-neovim] LSP request: {} (id={})", method, id);

        let mut guard = self.stream.lock().unwrap();
        let stream = guard
            .as_mut()
            .ok_or_else(|| "Not connected to LSP server".to_string())?;

        // Write request
        stream
            .write_all(message.as_bytes())
            .map_err(|e| format!("Failed to send request: {}", e))?;
        stream.flush().map_err(|e| format!("Failed to flush: {}", e))?;

        crate::verbose_print!("[godot-neovim] LSP: Request sent, waiting for response...");

        // Read response
        let response = Self::read_response(stream)?;

        crate::verbose_print!("[godot-neovim] LSP: Response received for id={:?}", response.id);

        if let Some(id_resp) = response.id {
            if id_resp != id {
                return Err(format!(
                    "Response ID mismatch: expected {}, got {}",
                    id, id_resp
                ));
            }
        }

        if let Some(error) = response.error {
            return Err(format!("LSP error {}: {}", error.code, error.message));
        }

        match response.result {
            Some(value) => {
                serde_json::from_value(value).map_err(|e| format!("Failed to parse result: {}", e))
            }
            None => {
                // For some requests, null result is valid
                serde_json::from_value(Value::Null)
                    .map_err(|e| format!("Failed to parse null result: {}", e))
            }
        }
    }

    fn send_notification(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
        };

        let body = serde_json::to_string(&notification).map_err(|e| e.to_string())?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        let mut guard = self.stream.lock().unwrap();
        let stream = guard
            .as_mut()
            .ok_or_else(|| "Not connected to LSP server".to_string())?;

        stream
            .write_all(message.as_bytes())
            .map_err(|e| format!("Failed to send notification: {}", e))?;
        stream.flush().map_err(|e| format!("Failed to flush: {}", e))?;

        Ok(())
    }

    fn read_message(reader: &mut BufReader<&mut TcpStream>) -> Result<String, String> {
        // Read headers
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|e| format!("Failed to read header: {}", e))?;

            let line = line.trim();
            if line.is_empty() {
                break;
            }

            if let Some(len_str) = line.strip_prefix("Content-Length: ") {
                content_length = Some(
                    len_str
                        .parse()
                        .map_err(|e| format!("Invalid Content-Length: {}", e))?,
                );
            }
        }

        let content_length =
            content_length.ok_or_else(|| "Missing Content-Length header".to_string())?;

        // Read body
        let mut body = vec![0u8; content_length];
        reader
            .read_exact(&mut body)
            .map_err(|e| format!("Failed to read body: {}", e))?;

        String::from_utf8(body).map_err(|e| format!("Invalid UTF-8 in response: {}", e))
    }

    fn read_response(stream: &mut TcpStream) -> Result<JsonRpcResponse, String> {
        let mut reader = BufReader::new(stream);

        // Loop to skip notifications (messages without id)
        loop {
            let body_str = Self::read_message(&mut reader)?;

            // Try to parse as response
            let value: Value = serde_json::from_str(&body_str)
                .map_err(|e| format!("Failed to parse JSON: {}", e))?;

            // Check if this is a response (has id) or a notification (no id)
            if value.get("id").is_some() {
                // This is a response
                return serde_json::from_value(value)
                    .map_err(|e| format!("Failed to parse response: {}", e));
            }

            // This is a notification, skip it and continue reading
            // Optionally log it for debugging
        }
    }
}

impl Default for GodotLspClient {
    fn default() -> Self {
        Self::new()
    }
}
