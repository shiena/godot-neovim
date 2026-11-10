// Run the protocol code without requiring an initialized Godot engine.
#[macro_export]
macro_rules! verbose_print {
    ($($arg:tt)*) => { let _ = format!($($arg)*); };
}

#[allow(dead_code)]
#[path = "../src/lsp/client.rs"]
mod lsp;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::time::Duration;

fn read_message(reader: &mut BufReader<std::net::TcpStream>) -> serde_json::Value {
    let mut length = None;
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let mut bytes = vec![0; length.unwrap()];
    reader.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn reconnect_requires_initialization_of_the_new_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream);
            let request = read_message(&mut reader);
            assert_eq!(request["method"], "initialize");
            let response = serde_json::json!({
                "jsonrpc": "2.0", "id": request["id"], "result": {"capabilities": {}}
            })
            .to_string();
            write!(
                reader.get_mut(),
                "Content-Length: {}\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
            assert_eq!(read_message(&mut reader)["method"], "initialized");
        }
    });
    let client = lsp::GodotLspClient::new();
    assert!(!client.is_connected());
    assert!(!client.is_initialized());
    for _ in 0..2 {
        client.connect(port).unwrap();
        assert!(client.is_connected());
        assert!(!client.is_initialized());
        client.initialize("file:///project").unwrap();
        assert!(client.is_initialized());
        // Repeated initialization must not send another request on this stream.
        client.initialize("file:///project").unwrap();
    }
    client.disconnect();
    assert!(!client.is_connected());
    assert!(!client.is_initialized());
    assert!(client.initialize("file:///project").is_err());
    server.join().unwrap();
}

#[test]
fn lsp_deadline_covers_trickled_notifications_and_discards_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let request = read_message(&mut reader);
        assert_eq!(request["method"], "textDocument/hover");
        // Frequent traffic defeats a per-read timeout, but not a request deadline.
        let body = r#"{"jsonrpc":"2.0","method":"test/progress","params":{}}"#;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        for _ in 0..30 {
            if reader.get_mut().write_all(message.as_bytes()).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let client = lsp::GodotLspClient::new();
    client.connect(port).unwrap();
    let began = std::time::Instant::now();
    assert!(client.hover("file:///project/test.gd", 0, 0).is_err());
    assert!(began.elapsed() < Duration::from_secs(2));
    assert!(!client.is_connected());
    server.join().unwrap();
}
