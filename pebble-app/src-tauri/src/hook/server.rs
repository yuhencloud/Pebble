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

fn log_event(msg: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    eprintln!("[pebble-hook {}] {}", now, msg);
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

fn read_request(stream: &mut TcpStream) -> Option<String> {
    let mut buf = [0u8; 65536];
    let mut n = 0usize;
    loop {
        match stream.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(bytes_read) => {
                n += bytes_read;
                if n >= buf.len() {
                    return None;
                }
                // Check if we've received the full headers
                let req_str = String::from_utf8_lossy(&buf[..n]);
                if let Some(header_end) = req_str.find("\r\n\r\n") {
                    let headers = &req_str[..header_end];
                    let content_length = headers.lines()
                        .find(|line| line.to_lowercase().starts_with("content-length:"))
                        .and_then(|line| line.split(':').nth(1))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    let body_start = header_end + 4;
                    if n >= body_start + content_length {
                        return Some(req_str[..body_start + content_length].to_string());
                    }
                }
            }
            Err(_) => break,
        }
    }
    let req_str = String::from_utf8_lossy(&buf[..n]);
    if req_str.is_empty() { None } else { Some(req_str.to_string()) }
}

fn write_response(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
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
    let req = match read_request(&mut stream) {
        Some(r) => r,
        None => {
            let _ = stream.write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return;
        }
    };
    let first_line = req.lines().next().unwrap_or("");

    if first_line.starts_with("GET /instances") {
        let map = instances.lock();
        let mut list: Vec<crate::types::Instance> = map.values().cloned().collect();
        drop(map);
        list.sort_by(|a, b| a.working_directory.cmp(&b.working_directory));
        let body = serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string());
        write_response(&mut stream, "200 OK", &body);
    } else if first_line.starts_with("POST /hook") {
        if let Some(body_start) = req.find("\r\n\r\n") {
            let body = &req[body_start + 4..];
            if let Ok(payload) = serde_json::from_str::<crate::types::IncomingHookPayload>(body) {
                log_event(&format!("event={} tool={:?} mode={:?} tool_use_id={:?} ts={}",
                    payload.event,
                    payload.tool_name,
                    payload.permission_mode,
                    payload.tool_use_id,
                    payload.timestamp
                ));
                handler.lock()(&payload);
                let should_block = payload.event == "PermissionRequest"
                    || (payload.event == "PreToolUse"
                        && !matches!(
                            payload.permission_mode.as_deref(),
                            Some("bypassPermissions" | "dontAsk" | "auto" | "acceptEdits")
                        ));
                if should_block {
                    let key = payload
                        .tool_use_id
                        .clone()
                        .unwrap_or_else(|| payload.timestamp.to_string());
                    log_event(&format!("blocking for key={} event={}", key, payload.event));
                    if let Some(response_body) =
                        permission_store.wait_for(&key, Duration::from_secs(300))
                    {
                        log_event(&format!("responded key={} body={}", key, response_body));
                        write_response(&mut stream, "200 OK", &response_body);
                    } else {
                        log_event(&format!("timeout key={}", key));
                        write_response(&mut stream, "200 OK", "OK");
                    }
                } else {
                    write_response(&mut stream, "200 OK", "OK");
                }
            } else {
                log_event(&format!("failed to parse hook body: {}", body));
                write_response(&mut stream, "200 OK", "OK");
            }
        } else {
            write_response(&mut stream, "200 OK", "OK");
        }
    } else {
        write_response(&mut stream, "404 Not Found", "");
    }
}
