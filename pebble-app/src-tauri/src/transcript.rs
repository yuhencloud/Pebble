use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};

pub fn read_transcript_preview(path: &str, n: usize) -> Vec<String> {
    let exchange = read_last_exchange(path);
    let mut result = Vec::new();
    if let Some(user) = exchange.0 {
        result.push(truncate_preview(&user, 80, "You: "));
    }
    if let Some(assistant) = exchange.1 {
        result.push(truncate_preview(&assistant, 80, ""));
    }
    if result.len() > n {
        result.truncate(n);
    }
    result
}

pub fn read_last_exchange(path: &str) -> (Option<String>, Option<String>) {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return (None, None),
    };

    let len = match file.seek(SeekFrom::End(0)) {
        Ok(l) => l as i64,
        Err(_) => return (None, None),
    };

    let seek_offset = -(65536.min(len));
    let _ = file.seek(SeekFrom::End(seek_offset));
    let mut discard = String::new();
    let mut reader = BufReader::new(file);
    let _ = reader.read_line(&mut discard);

    let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    let mut user_preview: Option<String> = None;
    let mut assistant_preview: Option<String> = None;

    for line in lines.iter().rev() {
        if user_preview.is_some() && assistant_preview.is_some() {
            break;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            let t = json.get("type").and_then(|v| v.as_str());
            if let Some("user") = t {
                if user_preview.is_none() {
                    if let Some(content) = json.get("message").and_then(|m| m.get("content")) {
                        if let Some(txt) = extract_clean_text(content, "user") {
                            let stripped = strip_markdown(&txt);
                            if !stripped.trim().is_empty() {
                                user_preview = Some(stripped);
                            }
                        }
                    }
                }
            } else if let Some("assistant") = t {
                if assistant_preview.is_none() {
                    if let Some(content) = json.get("message").and_then(|m| m.get("content")) {
                        if let Some(txt) = extract_clean_text(content, "assistant") {
                            let stripped = strip_markdown(&txt);
                            if !stripped.trim().is_empty() {
                                assistant_preview = Some(stripped);
                            }
                        }
                    }
                }
            }
        }
    }

    (user_preview, assistant_preview)
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

fn truncate_preview(text: &str, max_chars: usize, prefix: &str) -> String {
    let available = max_chars.saturating_sub(prefix.len());
    let truncated: String = text.chars().take(available).collect();
    if text.chars().count() > available {
        format!("{}{}...", prefix, truncated)
    } else {
        format!("{}{}", prefix, truncated)
    }
}

fn extract_clean_text(content: &serde_json::Value, role: &str) -> Option<String> {
    if let Some(s) = content.as_str() {
        if s.starts_with("<local-command-caveat>") || s.starts_with("<command-message>") {
            return None;
        }
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        let mut text_parts = Vec::new();
        for b in arr {
            if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                if !t.starts_with("<local-command-caveat>") && !t.starts_with("<command-message>") {
                    text_parts.push(t.to_string());
                }
            }
        }
        if !text_parts.is_empty() {
            return Some(text_parts.join("\n"));
        }
        if role == "assistant" {
            for b in arr {
                if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("Tool");
                    return Some(format!("Using {}", name));
                }
            }
        }
    }
    None
}

pub fn strip_markdown(text: &str) -> String {
    let mut result = text.to_string();
    // Code blocks -> "Code: lang" or stripped
    result = regex::Regex::new(r"```(\w+)?\n[\s\S]*?```")
        .unwrap()
        .replace_all(&result, |caps: &regex::Captures| {
            if let Some(lang) = caps.get(1) {
                format!("Code: {}", lang.as_str())
            } else {
                "Code".to_string()
            }
        })
        .to_string();
    // Inline code
    result = regex::Regex::new(r"`([^`]+)`").unwrap().replace_all(&result, "$1").to_string();
    // Bold / italic
    result = regex::Regex::new(r"\*\*([^*]+)\*\*").unwrap().replace_all(&result, "$1").to_string();
    result = regex::Regex::new(r"\*([^*]+)\*").unwrap().replace_all(&result, "$1").to_string();
    result = regex::Regex::new(r"__([^_]+)__").unwrap().replace_all(&result, "$1").to_string();
    result = regex::Regex::new(r"_([^_]+)_").unwrap().replace_all(&result, "$1").to_string();
    // Headers
    result = regex::Regex::new(r"(?m)^#{1,6}\s*").unwrap().replace_all(&result, "").to_string();
    // List markers
    result = regex::Regex::new(r"(?m)^\s*[-*+]\s+").unwrap().replace_all(&result, "").to_string();
    result = regex::Regex::new(r"(?m)^\s*\d+\.\s+").unwrap().replace_all(&result, "").to_string();
    // Links [text](url)
    result = regex::Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap().replace_all(&result, "$1").to_string();
    // Collapse multiple spaces
    result = regex::Regex::new(r"\s+").unwrap().replace_all(&result.trim(), " ").to_string();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_clean_text_simple() {
        let val = serde_json::json!("hello world");
        assert_eq!(extract_clean_text(&val, "user"), Some("hello world".to_string()));
    }

    #[test]
    fn test_extract_clean_text_tool_use() {
        let val = serde_json::json!([{"type": "tool_use", "name": "Bash"}]);
        assert_eq!(extract_clean_text(&val, "assistant"), Some("Using Bash".to_string()));
    }

    #[test]
    fn test_strip_markdown_headers_and_lists() {
        let text = "## Title\n- item 1\n* item 2\n1. item 3";
        let result = strip_markdown(text);
        assert_eq!(result, "Title item 1 item 2 item 3");
    }

    #[test]
    fn test_strip_markdown_code_and_links() {
        let text = "Check [docs](https://example.com) and run `cargo build` then:\n```rust\nfn main() {}\n```";
        let result = strip_markdown(text);
        assert!(result.contains("Check docs"));
        assert!(result.contains("cargo build"));
        assert!(result.contains("Code: rust"));
    }

    #[test]
    fn test_read_last_exchange_empty_file() {
        let (u, a) = read_last_exchange("/nonexistent/path.jsonl");
        assert!(u.is_none());
        assert!(a.is_none());
    }
}
