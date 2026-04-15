use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

lazy_static::lazy_static! {
    static ref RE_CODE_BLOCK: regex::Regex = regex::Regex::new(r"```(\w+)?\n[\s\S]*?```").unwrap();
    static ref RE_INLINE_CODE: regex::Regex = regex::Regex::new(r"`([^`]+)`").unwrap();
    static ref RE_BOLD: regex::Regex = regex::Regex::new(r"\*\*([^*]+)\*\*").unwrap();
    static ref RE_ITALIC: regex::Regex = regex::Regex::new(r"\*([^*]+)\*").unwrap();
    static ref RE_UNDERLINE_DBL: regex::Regex = regex::Regex::new(r"__([^_]+)__").unwrap();
    static ref RE_UNDERLINE: regex::Regex = regex::Regex::new(r"_([^_]+)_").unwrap();
    static ref RE_HEADERS: regex::Regex = regex::Regex::new(r"(?m)^#{1,6}\s*").unwrap();
    static ref RE_LIST_BULLET: regex::Regex = regex::Regex::new(r"(?m)^\s*[-*+]\s+").unwrap();
    static ref RE_LIST_NUMBER: regex::Regex = regex::Regex::new(r"(?m)^\s*\d+\.\s+").unwrap();
    static ref RE_LINKS: regex::Regex = regex::Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap();
    static ref RE_SPACES: regex::Regex = regex::Regex::new(r"\s+").unwrap();
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

    let mut buf = Vec::new();
    let _ = reader.read_to_end(&mut buf);
    let lines: Vec<String> = buf
        .split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| String::from_utf8_lossy(l).into_owned())
        .collect();

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

    for line_result in reader.split(b'\n') {
        if let Ok(bytes) = line_result {
            let line = String::from_utf8_lossy(&bytes);
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(ts_str) = json.get("timestamp").and_then(|v| v.as_str()) {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                        return Some(dt.timestamp() as u64);
                    }
                }
            }
        }
    }
    None
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
    result = RE_CODE_BLOCK
        .replace_all(&result, |caps: &regex::Captures| {
            if let Some(lang) = caps.get(1) {
                format!("Code: {}", lang.as_str())
            } else {
                "Code".to_string()
            }
        })
        .to_string();
    // Inline code
    result = RE_INLINE_CODE.replace_all(&result, "$1").to_string();
    // Bold / italic
    result = RE_BOLD.replace_all(&result, "$1").to_string();
    result = RE_ITALIC.replace_all(&result, "$1").to_string();
    result = RE_UNDERLINE_DBL.replace_all(&result, "$1").to_string();
    result = RE_UNDERLINE.replace_all(&result, "$1").to_string();
    // Headers
    result = RE_HEADERS.replace_all(&result, "").to_string();
    // List markers
    result = RE_LIST_BULLET.replace_all(&result, "").to_string();
    result = RE_LIST_NUMBER.replace_all(&result, "").to_string();
    // Links [text](url)
    result = RE_LINKS.replace_all(&result, "$1").to_string();
    // Collapse multiple spaces
    result = RE_SPACES.replace_all(&result.trim(), " ").to_string();
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
