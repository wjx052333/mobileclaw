use std::collections::HashMap;
use std::path::Path;

/// Parse `export KEY = "value"` lines from a shell env file.
/// Strips surrounding quotes, leading/trailing whitespace, and trailing commas.
pub fn parse_env_file(src: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in src.lines() {
        let line = line.trim();
        let rest = if let Some(r) = line.strip_prefix("export ") { r } else { continue };
        let (key, val) = if let Some(pos) = rest.find('=') {
            (&rest[..pos], &rest[pos + 1..])
        } else {
            continue
        };
        let key = key.trim().to_string();
        let val = val.trim()
            .trim_end_matches(',')   // trailing comma (e.g. RECEIVER line)
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        if !key.is_empty() {
            map.insert(key, val);
        }
    }
    map
}

/// Load env from a .sh file path. Returns empty map if file can't be read.
pub fn load_env_file(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .map(|s| parse_env_file(&s))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_export_quoted_spaces() {
        let src = r#"export SMTP_SERVER = "smtp.163.com"
export SMTP_PORT = 25
export EMAIL_SENDER = "17611188358@163.com"
export EMAIL_PASSWORD = "ZXhXL5j2Z579xLMd"
export EMAIL_RECEIVER = "wjx052333@139.com","#;
        let map = parse_env_file(src);
        assert_eq!(map.get("SMTP_SERVER").map(|s| s.as_str()), Some("smtp.163.com"));
        assert_eq!(map.get("SMTP_PORT").map(|s| s.as_str()), Some("25"));
        assert_eq!(map.get("EMAIL_PASSWORD").map(|s| s.as_str()), Some("ZXhXL5j2Z579xLMd"));
        assert_eq!(map.get("EMAIL_RECEIVER").map(|s| s.as_str()), Some("wjx052333@139.com"));
    }

    #[test]
    fn test_skips_comments_and_blanks() {
        let src = "# comment\n\nexport FOO = \"bar\"\n# another";
        let map = parse_env_file(src);
        assert_eq!(map.len(), 1);
        assert_eq!(map["FOO"], "bar");
    }
}
