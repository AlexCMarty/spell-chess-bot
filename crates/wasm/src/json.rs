//! A JSON writer sized to exactly what this crate emits.
//!
//! Everything crossing the boundary is either a fixed key, an algebraic square,
//! a small integer, or a word from a closed set -- none of which need escaping.
//! `escape` exists anyway so a future field carrying arbitrary text cannot
//! silently emit malformed JSON. Hand-rolling this instead of pulling in
//! serde_json keeps roughly 100KB out of the wasm binary.

/// Escapes the characters JSON forbids raw in a string. Control characters below
/// 0x20 become `\u00XX`.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// A quoted, escaped JSON string.
pub fn string(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

/// `{"a":1,"b":2}` from pre-rendered values. Values are inserted verbatim, so
/// callers pass `string(..)` for text and plain `format!` output for numbers.
pub fn object(fields: &[(&str, String)]) -> String {
    let body: Vec<String> = fields.iter().map(|(k, v)| format!("{}:{}", string(k), v)).collect();
    format!("{{{}}}", body.join(","))
}

/// `[a,b,c]` from pre-rendered elements.
pub fn array(items: &[String]) -> String {
    format!("[{}]", items.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes_backslashes_and_controls() {
        assert_eq!(escape(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(escape("a\nb"), "a\\nb");
        assert_eq!(escape("a\u{1}b"), "a\\u0001b");
    }

    #[test]
    fn builds_objects_and_arrays() {
        assert_eq!(object(&[("a", "1".to_string()), ("b", string("x"))]), r#"{"a":1,"b":"x"}"#);
        assert_eq!(array(&["1".to_string(), "2".to_string()]), "[1,2]");
        assert_eq!(object(&[]), "{}");
        assert_eq!(array(&[]), "[]");
    }
}
