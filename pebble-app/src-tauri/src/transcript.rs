use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

pub fn read_transcript_preview(path: &str, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }

    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let len = match file.seek(SeekFrom::End(0)) {
        Ok(l) => l as i64,
        Err(_) => return Vec::new(),
    };

    let seek_offset = -(65536.min(len));
    let _ = file.seek(SeekFrom::End(seek_offset));
    let mut discard = String::new();
    let mut reader = BufReader::new(file);
    let _ = reader.read_line(&mut discard);

    let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    let mut result = Vec::new();

    for line in lines.iter().rev() {
        if result.len() >= n {
            break;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            let t = json.get("type").and_then(|v| v.as_str());
            if let Some("user") = t {
                if let Some(content) = json.get("message").and_then(|m| m.get("content")) {
                    if let Some(txt) = extract_preview_text(content, "user") {
                        let trimmed = txt.trim();
                        if !trimmed.is_empty() {
                            result.push(format!(
                                "You: {}",
                                trimmed.chars().take(80).collect::<String>()
                            ));
                        }
                    }
                }
            } else if let Some("assistant") = t {
                if let Some(content) = json.get("message").and_then(|m| m.get("content")) {
                    if let Some(txt) = extract_preview_text(content, "assistant") {
                        let trimmed = txt.trim();
                        if !trimmed.is_empty() {
                            result.push(trimmed.chars().take(80).collect::<String>());
                        }
                    }
                }
            }
        }
    }

    result.reverse();
    result
}

pub fn read_session_start_from_transcript(path: &str) -> Option<u64> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    for line in reader.lines().filter_map(|l| l.ok()) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(ts_str) = json.get("timestamp").and_then(|v| v.as_str()) {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                    return Some(dt.timestamp() as u64);
                }
            }
        }
    }
    None
}

fn extract_preview_text(content: &serde_json::Value, role: &str) -> Option<String> {
    if let Some(s) = content.as_str() {
        if s.starts_with("<local-command-caveat>") || s.starts_with("<command-message>") {
            return None;
        }
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        for b in arr {
            if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                return Some(t.to_string());
            }
        }
        if role == "assistant" {
            for b in arr {
                if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("Tool");
                    return Some(format!("Using {}", name));
                }
            }
        }
        if role == "user" {
            for b in arr {
                if b.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    if let Some(c) = b.get("content").and_then(|c| c.as_str()) {
                        let preview = c.trim();
                        if preview.len() > 50 {
                            return Some(format!("Result: {}...", &preview[..50]));
                        }
                        return Some(format!("Result: {}", preview));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_preview_text_simple() {
        let val = serde_json::json!("hello world");
        assert_eq!(extract_preview_text(&val, "user"), Some("hello world".to_string()));
    }

    #[test]
    fn test_extract_preview_text_tool_use() {
        let val = serde_json::json!([{"type": "tool_use", "name": "Bash"}]);
        assert_eq!(extract_preview_text(&val, "assistant"), Some("Using Bash".to_string()));
    }
}
