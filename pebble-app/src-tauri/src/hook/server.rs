use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use parking_lot::Mutex;
use std::collections::HashMap;

pub const HOOK_PORT: u16 = 9876;

pub fn start_hook_server<F>(instances: Arc<Mutex<HashMap<String, crate::types::Instance>>>, handler: F)
where
    F: FnMut(&crate::types::IncomingHookPayload) + Send + 'static,
{
    let handler = Arc::new(Mutex::new(handler));
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", HOOK_PORT)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind hook server: {}", e);
                return;
            }
        };
        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                let inst = instances.clone();
                let h = handler.clone();
                std::thread::spawn(move || {
                    handle_http_request(stream, inst, h);
                });
            }
        }
    });
}

fn handle_http_request<F>(
    mut stream: TcpStream,
    instances: Arc<Mutex<HashMap<String, crate::types::Instance>>>,
    handler: Arc<Mutex<F>>,
)
where
    F: FnMut(&crate::types::IncomingHookPayload),
{
    let mut buf = [0u8; 65536];
    let mut n = 0usize;
    loop {
        match stream.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(bytes_read) => {
                n += bytes_read;
                if n == buf.len() {
                    let _ = stream.write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\n\r\n");
                    return;
                }
            }
            Err(_) => break,
        }
    }
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");

    if first_line.starts_with("GET /instances") {
        let map = instances.lock();
        let mut list: Vec<crate::types::Instance> = map.values().cloned().collect();
        drop(map);
        list.sort_by(|a, b| a.working_directory.cmp(&b.working_directory));
        let body = serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string());
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
    } else if first_line.starts_with("POST /hook") {
        if let Some(body_start) = req.find("\r\n\r\n") {
            let body = &req[body_start + 4..];
            if let Ok(payload) = serde_json::from_str::<crate::types::IncomingHookPayload>(body) {
                handler.lock()(&payload);
            }
        }
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
    } else {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
    }
}
