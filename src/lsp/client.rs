use lsp_types::{
    DidOpenTextDocumentParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    InitializeParams, InitializeResult, Location, Position, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, Uri, WorkspaceFolder,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

struct Connection {
    stream: LspStream,
    initialized: bool,
}

pub struct GodotLspClient {
    connection: Mutex<Option<Connection>>,
    request_id: AtomicI64,
}

impl GodotLspClient {
    pub fn new() -> Self {
        Self {
            connection: Mutex::new(None),
            request_id: AtomicI64::new(1),
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn connect(&self, port: u16) -> Result<(), String> {
        let addr = format!("127.0.0.1:{}", port);
        let stream = TcpStream::connect(&addr)
            .map_err(|e| format!("Failed to connect to LSP server at {}: {}", addr, e))?;

        Self::from_stream(stream).map(|client| {
            *self
                .connection
                .lock()
                .expect("LSP connection lock poisoned") = client.connection.into_inner().unwrap();
        })
    }

    pub(super) fn from_stream(stream: TcpStream) -> Result<Self, String> {
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            connection: Mutex::new(Some(Connection {
                stream: LspStream {
                    socket: stream,
                    startup: None,
                },
                initialized: false,
            })),
            request_id: AtomicI64::new(1),
        })
    }

    pub(super) fn initialize_cancellable(
        &self,
        root: &str,
        flag: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), String> {
        {
            let mut connection = self.connection.lock().map_err(|e| e.to_string())?;
            let stream = &mut connection.as_mut().ok_or("Not connected")?.stream;
            stream
                .socket
                .set_read_timeout(Some(Duration::from_millis(100)))
                .map_err(|e| e.to_string())?;
            stream
                .socket
                .set_write_timeout(Some(Duration::from_millis(100)))
                .map_err(|e| e.to_string())?;
            stream.startup = Some((flag, deadline));
        }
        self.initialize(root)?;
        let mut connection = self.connection.lock().map_err(|e| e.to_string())?;
        let stream = &mut connection.as_mut().ok_or("Not connected")?.stream;
        stream.startup = None;
        stream
            .socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        stream
            .socket
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Disconnect from LSP server
    pub fn disconnect(&self) {
        if let Ok(mut guard) = self.connection.lock() {
            if let Some(connection) = guard.take() {
                let _ = connection.stream.socket.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    pub fn initialize(&self, root_uri: &str) -> Result<(), String> {
        // Keep initialization tied to this connection even if reconnect is requested.
        let mut guard = self.connection.lock().map_err(|e| e.to_string())?;
        let connection = guard.as_mut().ok_or("Not connected to LSP server")?;
        if connection.initialized {
            return Ok(());
        }

        // Small delay to let the server prepare after connection
        std::thread::sleep(Duration::from_millis(100));

        let name = root_uri
            .split('/')
            .next_back()
            .unwrap_or("workspace")
            .to_string();
        let workspace_uri = root_uri.parse::<Uri>().map_err(|e| e.to_string())?;
        let params = InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: workspace_uri,
                name,
            }]),
            capabilities: lsp_types::ClientCapabilities::default(),
            ..Default::default()
        };

        let _result: InitializeResult = self.send_request_on(
            &mut connection.stream,
            "initialize",
            Some(serde_json::to_value(params).map_err(|e| format!("Failed to serialize: {}", e))?),
        )?;

        // Send initialized notification
        Self::send_notification_on(&mut connection.stream, "initialized", Some(json!({})))?;

        connection.initialized = true;
        Ok(())
    }

    pub fn did_open(&self, uri: &str, text: &str) -> Result<(), String> {
        let doc_uri = uri.parse::<Uri>().map_err(|e| e.to_string())?;
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri,
                language_id: "gdscript".to_string(),
                version: 1,
                text: text.to_string(),
            },
        };

        self.send_notification(
            "textDocument/didOpen",
            Some(serde_json::to_value(params).map_err(|e| format!("Failed to serialize: {}", e))?),
        )
    }

    pub fn goto_definition(
        &self,
        uri: &str,
        line: u32,
        col: u32,
    ) -> Result<Option<Location>, String> {
        let doc_uri = uri.parse::<Uri>().map_err(|e| e.to_string())?;
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: doc_uri },
                position: Position {
                    line,
                    character: col,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result: Option<GotoDefinitionResponse> = self.send_request(
            "textDocument/definition",
            Some(serde_json::to_value(params).map_err(|e| format!("Failed to serialize: {}", e))?),
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

    pub fn hover(&self, uri: &str, line: u32, col: u32) -> Result<Option<Hover>, String> {
        let doc_uri = uri.parse::<Uri>().map_err(|e| e.to_string())?;
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: doc_uri },
                position: Position {
                    line,
                    character: col,
                },
            },
            work_done_progress_params: Default::default(),
        };

        let result: Option<Hover> = self.send_request(
            "textDocument/hover",
            Some(serde_json::to_value(params).map_err(|e| format!("Failed to serialize: {}", e))?),
        )?;

        Ok(result)
    }

    pub fn is_connected(&self) -> bool {
        self.connection
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    pub fn is_initialized(&self) -> bool {
        self.connection
            .lock()
            .map(|guard| guard.as_ref().is_some_and(|c| c.initialized))
            .unwrap_or(false)
    }

    fn send_request<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<T, String> {
        let mut guard = self.connection.lock().map_err(|e| e.to_string())?;
        let connection = guard.as_mut().ok_or("Not connected to LSP server")?;
        self.send_request_on(&mut connection.stream, method, params)
    }

    fn send_request_on<T: for<'de> Deserialize<'de>>(
        &self,
        stream: &mut LspStream,
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

        // Write request
        stream
            .write_all(message.as_bytes())
            .map_err(|e| format!("Failed to send request: {}", e))?;
        stream
            .flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;

        // Read response
        let response = Self::read_response(stream)?;

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
        let mut guard = self.connection.lock().map_err(|e| e.to_string())?;
        let connection = guard.as_mut().ok_or("Not connected to LSP server")?;
        Self::send_notification_on(&mut connection.stream, method, params)
    }

    fn send_notification_on(
        stream: &mut LspStream,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), String> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
        };

        let body = serde_json::to_string(&notification).map_err(|e| e.to_string())?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        stream
            .write_all(message.as_bytes())
            .map_err(|e| format!("Failed to send notification: {}", e))?;
        stream
            .flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;

        Ok(())
    }

    fn read_message(reader: &mut BufReader<&mut LspStream>) -> Result<String, String> {
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

    fn read_response(stream: &mut LspStream) -> Result<JsonRpcResponse, String> {
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

// Preserve partial headers/bodies across short socket waits while allowing cancellation.
struct LspStream {
    socket: TcpStream,
    startup: Option<(Arc<AtomicBool>, Instant)>,
}
impl LspStream {
    fn check_cancelled(&self) -> std::io::Result<()> {
        if let Some((flag, deadline)) = &self.startup {
            if flag.load(Ordering::Acquire) || Instant::now() >= *deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "LSP startup cancelled or timed out",
                ));
            }
        }
        Ok(())
    }
    fn retry(&self, error: &std::io::Error) -> bool {
        self.startup.is_some()
            && matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::Interrupted
            )
    }
}
impl Read for LspStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        loop {
            self.check_cancelled()?;
            match self.socket.read(buffer) {
                Err(error) if self.retry(&error) => continue,
                result => return result,
            }
        }
    }
}
impl Write for LspStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        loop {
            self.check_cancelled()?;
            match self.socket.write(buffer) {
                Err(error) if self.retry(&error) => continue,
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush()
    }
}
