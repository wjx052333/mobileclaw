use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

/// Attempt to repair common LLM JSON mistakes before full parse failure.
///
/// Handles: missing comma between object fields — `"value" "key"` → `"value", "key"`.
/// This is safe because JSON string values always escape internal quotes as `\"`,
/// so an unescaped `"<whitespace>"` sequence must be a field boundary.
///
/// Returns the repaired string only if it differs from the input AND parses successfully.
fn try_repair_json(s: &str) -> Option<ToolCall> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // Matches: closing quote of a value, whitespace, opening quote of next key.
        // The pattern `"(\s+)"` matches both surrounding quotes, capturing the whitespace.
        // Replacement inserts the missing comma: `",$1"`.
        regex::Regex::new(r#""(\s+)""#).expect("static regex is valid")
    });
    let repaired = re.replace_all(s, "\",$1\"").into_owned();
    if repaired == s {
        return None; // no change — a different kind of error
    }
    serde_json::from_str::<ToolCall>(&repaired).ok()
}

/// Extract all `<tool_call>...</tool_call>` blocks from LLM output text
pub fn extract_tool_calls(text: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<tool_call>") {
        rest = &rest[start + "<tool_call>".len()..];
        if let Some(end) = rest.find("</tool_call>") {
            let json_str = rest[..end].trim();
            rest = &rest[end + "</tool_call>".len()..];
            if let Ok(call) = serde_json::from_str::<ToolCall>(json_str) {
                calls.push(call);
            } else if let Some(call) = try_repair_json(json_str) {
                tracing::debug!("repaired malformed tool_call JSON (missing comma): {}", json_str);
                calls.push(call);
            } else {
                tracing::warn!("skipping malformed tool_call JSON: {}", json_str);
            }
        } else {
            break; // unclosed tag, stop parsing
        }
    }
    calls
}

/// Serialize tool execution result to XML `<tool_result>` format
pub fn format_tool_result(name: &str, success: bool, output: &Value) -> String {
    let status = if success { "ok" } else { "error" };
    let body = serde_json::to_string(output).unwrap_or_else(|_| "{}".into());
    format!(r#"<tool_result name="{}" status="{}">{}</tool_result>"#, name, status, body)
}

/// Remove all tool_call blocks from text (extracts pure text output)
pub fn extract_text_without_tool_calls(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<tool_call>") {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(end) = rest.find("</tool_call>") {
            rest = &rest[end + "</tool_call>".len()..];
        } else {
            break;
        }
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_tool_call() {
        let text = r#"我来调用工具。
<tool_call>
{"name": "file_read", "args": {"path": "notes.txt"}}
</tool_call>
继续输出。"#;
        let calls = extract_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "file_read");
        assert_eq!(calls[0].args["path"], "notes.txt");
    }

    #[test]
    fn parse_multiple_tool_calls() {
        let text = r#"
<tool_call>{"name": "time", "args": {}}</tool_call>
<tool_call>{"name": "memory_search", "args": {"query": "rust"}}</tool_call>
"#;
        assert_eq!(extract_tool_calls(text).len(), 2);
    }

    #[test]
    fn no_tool_calls_returns_empty() {
        assert!(extract_tool_calls("hello world").is_empty());
    }

    #[test]
    fn malformed_json_is_skipped() {
        let text = r#"<tool_call>not json</tool_call>"#;
        assert!(extract_tool_calls(text).is_empty());
    }

    #[test]
    fn serialize_tool_result_ok() {
        let xml = format_tool_result("file_read", true, &serde_json::json!({"content": "hi"}));
        assert!(xml.contains(r#"name="file_read""#));
        assert!(xml.contains(r#"status="ok""#));
        assert!(xml.contains("content"));
    }

    #[test]
    fn serialize_tool_result_error() {
        let xml = format_tool_result("file_read", false, &serde_json::json!("file not found"));
        assert!(xml.contains(r#"status="error""#));
    }

    #[test]
    fn extract_text_strips_tool_calls() {
        let text = "Before.<tool_call>{\"name\":\"x\",\"args\":{}}</tool_call>After.";
        let clean = extract_text_without_tool_calls(text);
        assert_eq!(clean.trim(), "Before.After.");
    }

    #[test]
    fn repaired_missing_comma_between_name_and_args() {
        // LLM emits: {"name": "memory_search" "args": {"query": "foo"}}
        let text = r#"<tool_call>{"name": "memory_search" "args": {"query": "foo", "limit": 10}}</tool_call>"#;
        let calls = extract_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "memory_search");
        assert_eq!(calls[0].args["query"], "foo");
        assert_eq!(calls[0].args["limit"], 10);
    }

    #[test]
    fn repaired_multiline_whitespace_between_fields() {
        // Newline instead of comma
        let text = "<tool_call>{\"name\": \"file_read\"\n\"args\": {\"path\": \"x.txt\"}}</tool_call>";
        let calls = extract_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "file_read");
        assert_eq!(calls[0].args["path"], "x.txt");
    }

    #[test]
    fn unrepairable_json_still_skipped() {
        // Genuinely broken JSON that repair can't fix
        let text = r#"<tool_call>{"name": "foo" bad json here}</tool_call>"#;
        assert!(extract_tool_calls(text).is_empty());
    }

    #[test]
    fn valid_json_unaffected_by_repair_path() {
        // Already-valid JSON must not be double-modified
        let text = r#"<tool_call>{"name": "memory_search", "args": {"query": "test"}}</tool_call>"#;
        let calls = extract_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args["query"], "test");
    }
}
