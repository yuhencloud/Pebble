use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Condvar, Mutex as StdMutex};
use std::time::{Duration, Instant};

pub const HOOK_PORT: u16 = 9876;

#[derive(Clone)]
pub struct PermissionResponseStore {
    inner: Arc<(StdMutex<HashMap<String, String>>, Condvar)>,
}

impl PermissionResponseStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new((StdMutex::new(HashMap::new()), Condvar::new())),
        }
    }

    pub fn set(&self, key: String, value: String) {
        let (lock, cvar) = &*self.inner;
        lock.lock().unwrap().insert(key, value);
        cvar.notify_all();
    }

    pub fn wait_for(&self, key: &str, timeout: Duration) -> Option<String> {
        let (lock, cvar) = &*self.inner;
        let mut guard = lock.lock().unwrap();
        let start = Instant::now();
        loop {
            if let Some(val) = guard.get(key) {
                return Some(val.clone());
            }
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return None;
            }
            let remaining = timeout - elapsed;
            let (g, _) = cvar.wait_timeout(guard, remaining).ok()?;
            guard = g;
        }
    }
}

pub fn start_hook_server<F>(
    instances: Arc<Mutex<HashMap<String, crate::types::Instance>>>,
    permission_store: PermissionResponseStore,
    handler: F,
)
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
                let ps = permission_store.clone();
                let h = handler.clone();
                std::thread::spawn(move || {
                    handle_http_request(stream, inst, ps, h);
                });
            }
        }
    });
}

fn handle_http_request<F>(
    mut stream: TcpStream,
    instances: Arc<Mutex<HashMap<String, crate::types::Instance>>>,
    permission_store: PermissionResponseStore,
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
                if payload.event == "PermissionRequest" {
                    let key = payload
                        .tool_use_id
                        .clone()
                        .unwrap_or_else(|| payload.timestamp.to_string());
                    if let Some(response_body) =
                        permission_store.wait_for(&key, Duration::from_secs(300))
                    {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            response_body.len(),
                            response_body
                        );
                        let _ = stream.write_all(response.as_bytes());
                    } else {
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
                    }
                } else {
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
                }
            } else {
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
            }
        } else {
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
        }
    } else {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
    }
}
