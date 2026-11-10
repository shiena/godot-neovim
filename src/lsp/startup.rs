use super::client::GodotLspClient;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Connect and initialize off the editor thread, with cancellable retry waits.
pub struct LspStartup {
    result: mpsc::Receiver<Result<GodotLspClient, String>>,
    cancel: mpsc::Sender<()>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LspStartup {
    pub fn start(host: String, port: u16, root: String) -> Result<Self, String> {
        Self::with_timeout(host, port, root, Duration::from_secs(120))
    }

    fn with_timeout(
        host: String,
        port: u16,
        root: String,
        timeout: Duration,
    ) -> Result<Self, String> {
        let (tx, result) = mpsc::channel();
        let (cancel, cancelled_rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let worker = std::thread::Builder::new()
            .name("godot-neovim-lsp-startup".into())
            .spawn(move || {
                let deadline = Instant::now() + timeout;
                let addresses = match (host.as_str(), port).to_socket_addrs() {
                    Ok(addresses) => addresses.collect::<Vec<_>>(),
                    Err(error) => {
                        let _ = tx.send(Err(error.to_string()));
                        return;
                    }
                };
                let mut last_error = format!("No address for {host}:{port}");
                while !flag.load(Ordering::Acquire) && Instant::now() < deadline {
                    for address in &addresses {
                        if flag.load(Ordering::Acquire) {
                            return;
                        }
                        let attempt = (|| {
                            let stream =
                                TcpStream::connect_timeout(address, Duration::from_millis(250))
                                    .map_err(|e| e.to_string())?;
                            if flag.load(Ordering::Acquire) {
                                return Err("Cancelled".into());
                            }
                            let client = GodotLspClient::from_stream(stream)?;
                            client.initialize_cancellable(&root, flag.clone(), deadline)?;
                            Ok(client)
                        })();
                        match attempt {
                            Ok(client) => {
                                let _ = tx.send(Ok(client));
                                return;
                            }
                            Err(error) => last_error = error,
                        }
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if cancelled_rx
                        .recv_timeout(Duration::from_millis(500).min(remaining))
                        .is_ok()
                    {
                        return;
                    }
                }
                if !flag.load(Ordering::Acquire) {
                    let _ = tx.send(Err(format!(
                        "LSP startup timed out ({host}:{port}): {last_error}"
                    )));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            result,
            cancel,
            cancelled,
            worker: Some(worker),
        })
    }

    pub fn poll(&self) -> Option<Result<GodotLspClient, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("LSP startup worker stopped".into())),
        }
    }
}

impl Drop for LspStartup {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.cancel.send(());
        // Join before unloading the extension; short socket waits observe cancellation.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn respond_to_initialize(stream: TcpStream) {
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut length = 0;
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = value.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(request["method"], "initialize");
        let body =
            serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "result":{"capabilities":{}}})
                .to_string();
        // Split a response across socket timeouts to exercise partial-header retention.
        reader.get_mut().write_all(b"Content-Len").unwrap();
        std::thread::sleep(Duration::from_millis(250));
        write!(reader.get_mut(), "gth: {}\r\n\r\n{}", body.len(), body).unwrap();
        // Wait for the initialized notification before closing the socket.
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
    }

    #[test]
    fn retries_until_a_late_server_is_ready() {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let startup = LspStartup::with_timeout(
            "127.0.0.1".into(),
            port,
            "file:///test".into(),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(startup.poll().is_none());
        std::thread::sleep(Duration::from_millis(150));
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        let server =
            std::thread::spawn(move || respond_to_initialize(listener.accept().unwrap().0));
        let deadline = Instant::now() + Duration::from_secs(5);
        let client = loop {
            if let Some(result) = startup.poll() {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(client.is_connected() && client.is_initialized());
        server.join().unwrap();
    }

    #[test]
    fn cancellation_interrupts_initialization_and_joins_worker() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let startup = LspStartup::start(
            "127.0.0.1".into(),
            listener.local_addr().unwrap().port(),
            "file:///test".into(),
        )
        .unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut byte = [0];
        peer.read_exact(&mut byte).unwrap();
        let began = Instant::now();
        drop(startup);
        assert!(began.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn unavailable_server_times_out() {
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let startup = LspStartup::with_timeout(
            "127.0.0.1".into(),
            port,
            "file:///test".into(),
            Duration::from_millis(100),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(result) = startup.poll() {
                assert!(result.err().unwrap().contains("timed out"));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
