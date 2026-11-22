//! Structured output: constrain a harness's final answer to a JSON Schema, then
//! validate it and drive a feedback-retry loop until it conforms. Pure: no I/O.
//!
//! Two delivery paths, chosen per harness (see [`NativeSchema`] in the
//! registry):
//!
//! - **Native** — a harness whose CLI takes the schema directly (Claude Code's
//!   `--json-schema`, which returns the conforming value in a `structured_output`
//!   field; codex's `--output-schema <file>`, whose final message is the value).
//!   The schema reaches the model through the flag; nothing is added to the
//!   prompt. Codex takes it only for a schema OpenAI strict mode accepts
//!   ([`Schema::is_openai_strict`]); any other schema stays prompt-based there.
//! - **Prompt-based** — every other harness. The schema is appended to the
//!   prompt as an instruction to emit only a conforming JSON value, and the
//!   value is recovered from the harness's final text.
//!
//! Either way oneharness *validates* the result itself with the [`jsonschema`]
//! crate, so a native harness that ignores its own flag is caught too. On a
//! validation failure the caller re-runs the harness with [`retry_instruction`],
//! which restates the schema, shows the prior answer, and lists the errors. Like
//! every other normalized signal, the structured value is **never fabricated**:
//! when no JSON can be extracted the result is "invalid", not a guess.

use serde_json::Value;

/// How a harness accepts a schema natively, when it does. Registry data on each
/// [`crate::domain::harness::HarnessSpec`]; `None` means prompt-based delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeSchema {
    /// Claude Code: `--json-schema <inline JSON>`. The conforming value is
    /// returned under `structured_output`: the result document's under
    /// `--output-format json` (only on an explicit non-stream format), else the
    /// `result` event's under stream-json, the run's format by default.
    ClaudeJsonSchema,
    /// Codex: `exec --output-schema <file>`. The conforming value is the final
    /// `agent_message` text. Codex sends the schema as a *strict* response
    /// format, which the API refuses for a schema strict mode does not accept,
    /// so only such a schema takes this path ([`NativeSchema::accepts`]).
    /// openai/codex#15451 (the flag reportedly ignored once a tool runs) did
    /// not reproduce on 0.159.2: real runs with and without `--image`, with no
    /// tool and after a shell tool ran, all ended on a conforming answer.
    CodexOutputSchema,
}

impl NativeSchema {
    /// Whether this native path takes `schema`; a schema it refuses stays on
    /// the prompt-based path, chosen before the run and never after a failure.
    pub fn accepts(self, schema: &Schema) -> bool {
        match self {
            NativeSchema::ClaudeJsonSchema => true,
            NativeSchema::CodexOutputSchema => schema.is_openai_strict(),
        }
    }

    /// Whether the harness reads the schema from a file rather than inline.
    pub fn takes_file(self) -> bool {
        matches!(self, NativeSchema::CodexOutputSchema)
    }
}

/// A compiled JSON Schema plus its canonical text, used to instruct a harness,
/// validate its answer, and build retry feedback.
pub struct Schema {
    raw: Value,
    /// Compact JSON of the schema, for prompt injection and native flags.
    text: String,
    validator: jsonschema::Validator,
}

impl Schema {
    /// Compile a schema from its JSON text. Errors when the text is not valid
    /// JSON or not a usable JSON Schema — a loud failure at the boundary, never
    /// a silently-ignored constraint.
    pub fn compile(text: &str) -> Result<Schema, String> {
        let raw: Value =
            serde_json::from_str(text).map_err(|e| format!("schema is not valid JSON: {e}"))?;
        let validator =
            jsonschema::validator_for(&raw).map_err(|e| format!("not a valid JSON Schema: {e}"))?;
        // Re-serialize to a compact, stable form so the schema embedded in a
        // prompt (or a native flag) is independent of the source file's
        // whitespace.
        let text = serde_json::to_string(&raw).unwrap_or_else(|_| text.to_string());
        Ok(Schema {
            raw,
            text,
            validator,
        })
    }

    /// The parsed schema value.
    pub fn as_value(&self) -> &Value {
        &self.raw
    }

    /// The canonical compact schema text (for prompts / native flags).
    pub fn as_text(&self) -> &str {
        &self.text
    }

    /// Whether OpenAI's strict structured-output mode accepts this schema, by
    /// a conservative whitelist of what the real API was seen to accept: the
    /// root is `type: object`; every node states a `type` (or is a `$ref` or
    /// an `anyOf`); every object node has `additionalProperties: false` and
    /// requires every property; and no node uses a keyword outside
    /// [`STRICT_KEYWORDS`]. Anything else — `allOf`, `oneOf`, `not`, `if`,
    /// `format`, an untyped `enum`/`const` — answers `false`.
    pub fn is_openai_strict(&self) -> bool {
        self.raw.get("type").and_then(Value::as_str) == Some("object") && strict_node(&self.raw)
    }

    /// Validate an instance: `Ok(())` when it conforms, else the human-readable
    /// validation errors in document order.
    pub fn validate(&self, instance: &Value) -> Result<(), Vec<String>> {
        let errors: Vec<String> = self
            .validator
            .iter_errors(instance)
            .map(|e| e.to_string())
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// The keywords a schema node may use and still be delivered to codex
/// natively. Seen accepted by the API under strict mode (codex 0.159.2):
/// `anyOf`, `$ref`/`$defs`, a nullable `type` array, `patternProperties`,
/// `pattern`, `minLength`/`maxLength`, the numeric bounds, `multipleOf`,
/// `minItems`/`maxItems`, a typed `enum`, and the annotations.
pub const STRICT_KEYWORDS: &[&str] = &[
    "$schema",
    "$ref",
    "$defs",
    "definitions",
    "title",
    "description",
    "type",
    "properties",
    "required",
    "additionalProperties",
    "patternProperties",
    "items",
    "anyOf",
    "enum",
    "const",
    "pattern",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "minItems",
    "maxItems",
];

fn strict_node(node: &Value) -> bool {
    let Some(map) = node.as_object() else {
        return false;
    };
    if !map.keys().all(|k| STRICT_KEYWORDS.contains(&k.as_str())) {
        return false;
    }
    let types: Vec<&str> = match map.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(ts)) => match ts.iter().map(Value::as_str).collect::<Option<Vec<_>>>() {
            Some(ts) => ts,
            None => return false,
        },
        Some(_) => return false,
        None if map.contains_key("$ref") || map.contains_key("anyOf") => Vec::new(),
        None => return false,
    };
    let properties = map.get("properties").and_then(Value::as_object);
    if types.contains(&"object") || properties.is_some() {
        if map.get("additionalProperties") != Some(&Value::Bool(false)) {
            return false;
        }
        let required: Vec<&str> = map
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if !properties
            .into_iter()
            .flatten()
            .all(|(name, _)| required.contains(&name.as_str()))
        {
            return false;
        }
    }
    let children = properties
        .into_iter()
        .chain(map.get("patternProperties").and_then(Value::as_object))
        .chain(map.get("$defs").and_then(Value::as_object))
        .chain(map.get("definitions").and_then(Value::as_object))
        .flat_map(|m| m.values())
        .chain(map.get("items"))
        .chain(
            map.get("anyOf")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );
    children.into_iter().all(strict_node)
}

/// The result of checking one harness response against the schema: the value
/// that was extracted (when any) and the validation errors (empty == valid).
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub value: Option<Value>,
    pub errors: Vec<String>,
}

impl Check {
    /// A conforming response: a value was extracted and it had no errors.
    pub fn is_valid(&self) -> bool {
        self.value.is_some() && self.errors.is_empty()
    }
}

/// Extract and validate a harness response against `schema`. The single source
/// of truth shared by the retry loop (which decides whether to re-run) and the
/// report (which records validity), so the two can never disagree.
pub fn check(schema: &Schema, native: Option<NativeSchema>, text: &str, stdout: &str) -> Check {
    match extract_value(native, text, stdout) {
        Some(value) => match schema.validate(&value) {
            Ok(()) => Check {
                value: Some(value),
                errors: Vec::new(),
            },
            Err(errors) => Check {
                value: Some(value),
                errors,
            },
        },
        None => Check {
            value: None,
            errors: vec!["no JSON value could be extracted from the response".to_string()],
        },
    }
}

/// Recover the structured value from a harness response. A native harness
/// reports it in a known field; everyone else embeds it in the final text.
pub fn extract_value(native: Option<NativeSchema>, text: &str, stdout: &str) -> Option<Value> {
    if native == Some(NativeSchema::ClaudeJsonSchema) {
        // Claude Code's `--json-schema` returns the conforming value under
        // `structured_output`: the whole document's under `json`, the last
        // `result` event's under stream-json. Prefer it; fall back to parsing
        // the text if the field is absent (e.g. the flag was ignored).
        let document = serde_json::from_str::<Value>(stdout.trim()).ok();
        let result_event = || {
            stdout
                .lines()
                .rev()
                .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
                .find(|event| event.get("type").and_then(Value::as_str) == Some("result"))
        };
        if let Some(value) = document
            .or_else(result_event)
            .and_then(|doc| doc.get("structured_output").cloned())
            .filter(|value| !value.is_null())
        {
            return Some(value);
        }
    }
    extract_json(text)
}

/// Best-effort recovery of a single JSON value from a model's free-form answer:
/// the whole trimmed text when it parses, else a fenced ```` ```json ```` block,
/// else the first embedded JSON value found in surrounding prose. `None` when
/// none of those yield a value.
pub fn extract_json(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    if let Some(inner) = fenced_block(trimmed) {
        let body = inner.trim();
        if let Ok(value) = serde_json::from_str::<Value>(body) {
            return Some(value);
        }
        if let Some(value) = first_json_value(body) {
            return Some(value);
        }
    }
    first_json_value(trimmed)
}

/// The body of the first fenced code block, dropping an optional language tag on
/// the opening fence (```` ```json ````). `None` when there is no closed fence.
fn fenced_block(s: &str) -> Option<&str> {
    let start = s.find("```")?;
    let after = &s[start + 3..];
    // Skip an optional language tag up to (and including) the first newline.
    let body_start = after.find('\n').map(|i| i + 1)?;
    let body = &after[body_start..];
    let end = body.find("```")?;
    Some(&body[..end])
}

/// The first complete JSON value embedded in `s`, starting at the first `{`/`[`.
/// Uses serde's streaming parser, which stops at the end of the first value, so
/// trailing prose after a JSON object is ignored rather than failing the parse.
fn first_json_value(s: &str) -> Option<Value> {
    let start = s.find(['{', '['])?;
    serde_json::Deserializer::from_str(&s[start..])
        .into_iter::<Value>()
        .next()
        .and_then(Result::ok)
}

/// The instruction appended to a prompt for prompt-based delivery: emit a single
/// conforming JSON value and nothing else.
///
/// Deliberately **single-line** (no newlines). The command layer appends it to
/// the prompt, which becomes one argv element; Rust's std refuses to pass an
/// argument containing `\n`/`\r` to a `.bat`/`.cmd` shim ("batch file arguments
/// are invalid"), which is how every npm-installed harness lands on Windows. A
/// newline here would make prompt-based `--schema` unspawnable there. The schema
/// text is already compact JSON, so the whole instruction stays one line.
pub fn prompt_instruction(schema_text: &str) -> String {
    format!(
        "You must respond with a single JSON value that strictly conforms to the \
         following JSON Schema. Output ONLY that JSON value — no prose, no \
         explanation, and no Markdown code fences. JSON Schema: {schema_text}"
    )
}

/// The feedback prompt for a retry: restate the schema, show the prior
/// (non-conforming) answer and the exact validation errors, and ask again for a
/// conforming JSON value only. Single-line for the same cross-platform reason as
/// [`prompt_instruction`]; the prior answer (which may itself span lines) is
/// flattened to spaces so it never reintroduces a newline.
pub fn retry_instruction(schema_text: &str, previous: &str, errors: &[String]) -> String {
    let errors = if errors.is_empty() {
        "(no JSON value could be extracted from the response)".to_string()
    } else {
        errors.join("; ")
    };
    let previous = flatten_whitespace(previous);
    format!(
        "Your previous response did not conform to the required JSON Schema. \
         Previous response: {previous} -- Validation errors: {errors}. \
         Respond again with ONLY a single JSON value that strictly conforms to \
         this JSON Schema (no prose, no code fences): {schema_text}"
    )
}

/// Collapse every run of whitespace (including newlines) to a single space, so a
/// multi-line model answer can be embedded in a single-line retry prompt without
/// reintroducing a `\n` (see [`prompt_instruction`] for why that matters).
fn flatten_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PERSON: &str = r#"{"type":"object","properties":{"name":{"type":"string"},
        "age":{"type":"integer"}},"required":["name","age"],"additionalProperties":false}"#;

    fn person() -> Schema {
        Schema::compile(PERSON).expect("schema compiles")
    }

    #[test]
    fn compile_rejects_non_json_and_invalid_schema() {
        let err = Schema::compile("not json")
            .err()
            .expect("non-json rejected");
        assert!(err.contains("not valid JSON"), "{err}");
        // A schema whose `type` is a number is not a valid JSON Schema.
        let err = Schema::compile(r#"{"type": 5}"#)
            .err()
            .expect("invalid schema rejected");
        assert!(err.contains("not a valid JSON Schema"), "{err}");
    }

    #[test]
    fn validate_accepts_conforming_and_lists_errors_otherwise() {
        let s = person();
        assert!(s.validate(&json!({"name": "Ada", "age": 36})).is_ok());
        let errs = s
            .validate(&json!({"name": "Ada"}))
            .expect_err("missing required age");
        assert!(!errs.is_empty());
        // An extra property is rejected too (additionalProperties:false).
        assert!(s
            .validate(&json!({"name": "Ada", "age": 1, "x": true}))
            .is_err());
    }

    #[test]
    fn as_text_is_canonical_compact_json() {
        let s = Schema::compile(r#"{  "type" :  "string"  }"#).unwrap();
        assert_eq!(s.as_text(), r#"{"type":"string"}"#);
        assert_eq!(s.as_value(), &json!({"type": "string"}));
    }

    #[test]
    fn extract_json_parses_plain_document() {
        let v = extract_json(r#"  {"a":1}  "#).unwrap();
        assert_eq!(v, json!({"a": 1}));
    }

    #[test]
    fn extract_json_unwraps_fenced_block() {
        let text = "Here you go:\n```json\n{\"name\":\"Ada\",\"age\":36}\n```\nDone.";
        assert_eq!(extract_json(text).unwrap(), json!({"name":"Ada","age":36}));
        // A fence with no language tag works too.
        let text = "```\n[1, 2, 3]\n```";
        assert_eq!(extract_json(text).unwrap(), json!([1, 2, 3]));
    }

    #[test]
    fn extract_json_recovers_value_inside_a_noisy_fence() {
        // The fence is closed but its body has trailing prose, so the direct
        // parse of the body fails and the embedded-value scan inside the fence
        // recovers the object.
        let text = "```json\n{\"name\":\"Ada\",\"age\":36} (that's her)\n```";
        assert_eq!(extract_json(text).unwrap(), json!({"name":"Ada","age":36}));
    }

    #[test]
    fn extract_json_finds_object_embedded_in_prose() {
        let text = "Sure! {\"name\": \"Ada\", \"age\": 36} — hope that helps.";
        assert_eq!(extract_json(text).unwrap(), json!({"name":"Ada","age":36}));
    }

    #[test]
    fn extract_json_returns_none_when_absent() {
        assert!(extract_json("no json here at all").is_none());
        assert!(extract_json("   ").is_none());
        // An unterminated fence falls through to the embedded-value scan, which
        // still finds the object inside it.
        assert_eq!(extract_json("```json\n{\"a\":1}").unwrap(), json!({"a": 1}));
    }

    #[test]
    fn check_validates_prompt_based_text() {
        let s = person();
        let ok = check(&s, None, r#"{"name":"Ada","age":36}"#, "");
        assert!(ok.is_valid());
        assert_eq!(ok.value.unwrap(), json!({"name":"Ada","age":36}));

        let bad = check(&s, None, r#"{"name":"Ada"}"#, "");
        assert!(!bad.is_valid());
        assert!(bad.value.is_some());
        assert!(!bad.errors.is_empty());

        let none = check(&s, None, "I can't do that", "");
        assert!(!none.is_valid());
        assert!(none.value.is_none());
        assert_eq!(none.errors.len(), 1);
    }

    #[test]
    fn check_native_reads_structured_output_field() {
        let s = person();
        // Claude Code's result document: `result` is prose, the conforming value
        // is in `structured_output` — and that is what gets validated.
        let stdout = r#"{"type":"result","result":"Here is Ada.",
            "structured_output":{"name":"Ada","age":36}}"#;
        let c = check(
            &s,
            Some(NativeSchema::ClaudeJsonSchema),
            "Here is Ada.",
            stdout,
        );
        assert!(c.is_valid());
        assert_eq!(c.value.unwrap(), json!({"name":"Ada","age":36}));
    }

    #[test]
    fn native_falls_back_to_text_when_field_absent() {
        let s = person();
        // The flag was ignored (no structured_output) but the model still
        // emitted conforming JSON in its text — recovered and validated.
        let c = check(
            &s,
            Some(NativeSchema::ClaudeJsonSchema),
            r#"{"name":"Ada","age":36}"#,
            r#"{"type":"result","result":"{\"name\":\"Ada\",\"age\":36}"}"#,
        );
        assert!(c.is_valid());
    }

    #[test]
    fn instructions_carry_the_schema_and_errors() {
        let instr = prompt_instruction(r#"{"type":"string"}"#);
        assert!(instr.contains(r#"{"type":"string"}"#));
        assert!(instr.contains("ONLY"));

        let retry = retry_instruction(
            r#"{"type":"object"}"#,
            "oops not json",
            &["missing required property 'name'".to_string()],
        );
        assert!(retry.contains("oops not json"));
        assert!(retry.contains("missing required property 'name'"));
        assert!(retry.contains(r#"{"type":"object"}"#));

        // No extracted value: the retry still names the problem rather than
        // listing an empty error block.
        let retry = retry_instruction("{}", "prose", &[]);
        assert!(retry.contains("no JSON value could be extracted"));
    }

    #[test]
    fn instructions_are_single_line_for_cmd_shim_safety() {
        // The structured-output prompt additions must contain no newline, or the
        // prompt argument cannot be passed to a `.bat`/`.cmd` harness shim on
        // Windows ("batch file arguments are invalid"). A multi-line prior answer
        // must be flattened, not embedded verbatim.
        let instr = prompt_instruction("{\"type\":\"object\"}");
        assert!(!instr.contains('\n'), "prompt_instruction must be one line");
        let retry = retry_instruction(
            "{\"type\":\"object\"}",
            "line one\nline two\r\nline three",
            &["err a".to_string(), "err b".to_string()],
        );
        assert!(!retry.contains('\n'), "retry_instruction must be one line");
        assert!(!retry.contains('\r'), "retry_instruction must be one line");
        // The flattened prior answer keeps its words, just not its newlines.
        assert!(retry.contains("line one line two line three"), "{retry}");
        assert!(retry.contains("err a; err b"));
    }

    #[test]
    fn validator_is_shareable_across_threads() {
        // The retry loop calls `check` from the runner's worker threads, so the
        // compiled schema must be `Sync`. A failure here is a compile error.
        fn assert_sync<T: Sync>() {}
        assert_sync::<Schema>();
    }

    /// The cases are the schemas probed against the real API through codex
    /// 0.159.2 (`--output-schema`, strict mode): each `false` here is one the
    /// API refused with `invalid_json_schema`, or one that never answered.
    #[test]
    fn only_a_schema_strict_mode_accepts_is_openai_strict() {
        let strict = |v: Value| Schema::compile(&v.to_string()).unwrap().is_openai_strict();
        let object = |props: Value| {
            let required: Vec<String> = props.as_object().unwrap().keys().cloned().collect();
            json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
        };
        assert!(person().is_openai_strict());
        for accepted in [
            object(json!({"a": {"anyOf": [{"type": "string"}, {"type": "integer"}]}})),
            object(json!({"a": {"type": ["string", "null"]}})),
            object(json!({"a": {"type": "string", "minLength": 1, "pattern": "^a"}})),
            object(json!({"a": {"type": "string", "enum": ["x", "y"]}})),
            object(json!({"a": {"type": "array", "items": {"type": "string"}, "maxItems": 3}})),
            json!({"type":"object","properties":{"a":{"$ref":"#/$defs/x"}},"required":["a"],
                   "additionalProperties":false,"$defs":{"x":{"type":"string"}}}),
        ] {
            assert!(strict(accepted.clone()), "{accepted}");
        }
        for refused in [
            json!({"type":"object","properties":{"a":{"type":"string"}},"required":[],
                   "additionalProperties":false}),
            json!({"type":"object","properties":{"a":{"type":"string"}},"required":["a"]}),
            object(
                json!({"a": {"type": "object", "properties": {"b": {"type": "string"}},
                                 "required": ["b"]}}),
            ),
            object(json!({"a": {"allOf": [{"type": "string"}]}})),
            object(json!({"a": {"oneOf": [{"type": "string"}]}})),
            object(json!({"a": {"not": {"type": "string"}}})),
            object(json!({"a": {"type": "string", "format": "email"}})),
            object(json!({"a": {"const": "z"}})),
            json!({"type": "array", "items": {"type": "string"}}),
            json!({"properties":{"a":{"type":"string"}},"required":["a"],"additionalProperties":false}),
            json!({"type":"object","properties":{"a":{"type":"string"}},"required":["a"],
                   "additionalProperties":false,"if":{"properties":{"a":{"const":"x"}}}}),
        ] {
            assert!(!strict(refused.clone()), "{refused}");
        }
    }

    #[test]
    fn native_codex_takes_only_a_strict_schema_and_claude_any() {
        let loose =
            Schema::compile(r#"{"type":"object","properties":{"a":{"type":"string"}}}"#).unwrap();
        assert!(NativeSchema::ClaudeJsonSchema.accepts(&loose));
        assert!(!NativeSchema::CodexOutputSchema.accepts(&loose));
        assert!(NativeSchema::CodexOutputSchema.accepts(&person()));
        assert!(NativeSchema::CodexOutputSchema.takes_file());
        assert!(!NativeSchema::ClaudeJsonSchema.takes_file());
    }

    #[test]
    fn native_claude_reads_the_stream_json_result_event() {
        let stream = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","name":"StructuredOutput"}]}}"#,
            "\n",
            r#"{"type":"result","subtype":"success","result":"prose","structured_output":{"name":"Ada","age":36}}"#,
            "\n",
        );
        let c = check(
            &person(),
            Some(NativeSchema::ClaudeJsonSchema),
            "prose",
            stream,
        );
        assert!(c.is_valid(), "{c:?}");
        assert_eq!(c.value.unwrap()["age"], 36);
        let without = r#"{"type":"result","result":"{\"name\":\"Ada\",\"age\":1}"}"#;
        let c = check(
            &person(),
            Some(NativeSchema::ClaudeJsonSchema),
            r#"{"name":"Ada","age":1}"#,
            without,
        );
        assert_eq!(c.value.unwrap()["age"], 1, "falls back to the text");
    }
}
