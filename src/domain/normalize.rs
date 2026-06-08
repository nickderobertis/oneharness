//! Best-effort extraction of a harness's final assistant text from its raw
//! stdout. Pure: no I/O. The execution envelope (exit code, stdout, stderr,
//! duration) is always guaranteed; `text` is a convenience, and its method is
//! recorded so a consumer can tell extraction apart from raw passthrough.

use crate::domain::report::OutputFormat;
use serde_json::Value;

/// A successfully extracted final message and the method used to find it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub text: String,
    pub source: String,
}

/// Object keys, in priority order, that commonly hold a harness's final text.
const TEXT_KEYS: &[&str] = &["result", "text", "message", "output", "content", "response"];

/// Extract the final text from `stdout` according to the harness's format.
/// Returns `None` when nothing usable can be found (the caller then leaves
/// `text` null and consumers fall back to the raw `stdout`).
pub fn extract(stdout: &str, fmt: OutputFormat) -> Option<Extracted> {
    match fmt {
        OutputFormat::Text => extract_text(stdout),
        OutputFormat::Json => extract_json(stdout),
        OutputFormat::StreamJson => extract_stream_json(stdout),
    }
}

fn extract_text(stdout: &str) -> Option<Extracted> {
    let t = stdout.trim();
    (!t.is_empty()).then(|| Extracted {
        text: t.to_string(),
        source: "raw".to_string(),
    })
}

fn extract_json(stdout: &str) -> Option<Extracted> {
    let value: Value = serde_json::from_str(stdout.trim()).ok()?;
    json_text(&value).map(|(text, key)| Extracted {
        text,
        source: format!("json:{key}"),
    })
}

/// Scan line-delimited JSON events and return the last usable text, preferring
/// a terminal `result` event (the shape harnesses use for their final answer).
fn extract_stream_json(stdout: &str) -> Option<Extracted> {
    let mut last: Option<(String, &'static str)> = None;
    let mut last_result: Option<String> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some((text, key)) = json_text(&value) {
            if key == "result" {
                last_result = Some(text.clone());
            }
            last = Some((text, key));
        }
    }
    if let Some(text) = last_result {
        return Some(Extracted {
            text,
            source: "stream-json:result".to_string(),
        });
    }
    last.map(|(text, key)| Extracted {
        text,
        source: format!("stream-json:{key}"),
    })
}

/// Pull a non-empty string out of a JSON value: a bare string, or the first
/// matching key of an object. Returns the text and the key that matched.
fn json_text(value: &Value) -> Option<(String, &'static str)> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some((s.clone(), "string")),
        Value::Object(map) => {
            for key in TEXT_KEYS {
                if let Some(Value::String(s)) = map.get(*key) {
                    if !s.trim().is_empty() {
                        return Some((s.clone(), key));
                    }
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_format_trims_raw_stdout() {
        let got = extract("  hi there\n\n", OutputFormat::Text).unwrap();
        assert_eq!(got.text, "hi there");
        assert_eq!(got.source, "raw");
    }

    #[test]
    fn empty_text_yields_none() {
        assert!(extract("   \n", OutputFormat::Text).is_none());
    }

    #[test]
    fn json_prefers_result_field() {
        let raw = r#"{"type":"result","result":"the answer","is_error":false}"#;
        let got = extract(raw, OutputFormat::Json).unwrap();
        assert_eq!(got.text, "the answer");
        assert_eq!(got.source, "json:result");
    }

    #[test]
    fn json_falls_back_to_other_known_keys() {
        let got = extract(r#"{"message":"hello"}"#, OutputFormat::Json).unwrap();
        assert_eq!(got.text, "hello");
        assert_eq!(got.source, "json:message");
    }

    #[test]
    fn bare_json_string_is_extracted() {
        let got = extract(r#""just a string""#, OutputFormat::Json).unwrap();
        assert_eq!(got.text, "just a string");
        assert_eq!(got.source, "json:string");
    }

    #[test]
    fn unparseable_json_yields_none() {
        assert!(extract("not json at all", OutputFormat::Json).is_none());
    }

    #[test]
    fn stream_json_takes_terminal_result_event() {
        let raw = concat!(
            "{\"type\":\"system\"}\n",
            "{\"type\":\"assistant\",\"text\":\"thinking\"}\n",
            "{\"type\":\"result\",\"result\":\"final\"}\n",
        );
        let got = extract(raw, OutputFormat::StreamJson).unwrap();
        assert_eq!(got.text, "final");
        assert_eq!(got.source, "stream-json:result");
    }

    #[test]
    fn stream_json_falls_back_to_last_text_without_result() {
        let raw = "{\"type\":\"assistant\",\"text\":\"first\"}\n{\"type\":\"assistant\",\"text\":\"second\"}\n";
        let got = extract(raw, OutputFormat::StreamJson).unwrap();
        assert_eq!(got.text, "second");
        assert_eq!(got.source, "stream-json:text");
    }
}
