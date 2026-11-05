// CEP:FILE: tools/cep_lint/src/json.rs
// CEP:WHAT: Minimal deterministic JSON parser for lint configuration and self-test manifests.
// CEP:WHY: Chapter 50 requires the tool to be fully data-driven with zero external dependencies; a small, total parser with byte-accurate error positions is the cheapest way to satisfy CEP-LINT-BOOT-2 diagnostics.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: Parse errors return Err with file-relative line and column, reported as CEP-LINT-BOOT-2 by the caller; the parser never panics on any input.
// CEP:ASSUMES: Standard JSON grammar (RFC 8259) without trailing commas or comments; numbers are stored as f64 plus their raw text for exact integer recovery.
// CEP:COST: O(input length); recursion depth bounded by nesting depth of the document.
// CEP:EVIDENCE: unit tests in this file; exercised by every --self-test run.
// CEP:SECURITY: Untrusted configuration files cannot recurse unboundedly: nesting is bounded by input size and all slicing is bounds-checked.
// CEP:PORTABILITY: Pure core.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

/// A parsed JSON value. Objects use BTreeMap for deterministic iteration order.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// Numbers keep their raw text so integer semantics survive round-trips.
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Number(raw) => raw.parse::<i64>().ok(),
            _ => None,
        }
    }

    pub fn as_usize(&self) -> Option<usize> {
        match self {
            Json::Number(raw) => raw.parse::<usize>().ok(),
            _ => None,
        }
    }

    #[allow(dead_code)] // part of the complete Json accessor set
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, Json>> {
        match self {
            Json::Object(map) => Some(map),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        self.as_object().and_then(|map| map.get(key))
    }
}

/// Parse error with one-based line and column.
#[derive(Clone, Debug, PartialEq)]
pub struct JsonError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

// CEP:WHAT: Handles the parse step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn parse(text: &str) -> Result<Json, JsonError> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        pos: 0,
        depth: 0,
    };
    parser.skip_ws();
    let value = parser.value()?;
    parser.skip_ws();
    if parser.pos != parser.bytes.len() {
        return Err(parser.error("trailing characters after JSON value"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

const MAX_DEPTH: usize = 512;

impl<'a> Parser<'a> {
    fn error(&self, message: &str) -> JsonError {
        let (line, column) = self.line_col();
        JsonError {
            line,
            column,
            message: message.to_string(),
        }
    }

    // CEP:WHAT: Handles the line col step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn line_col(&self) -> (usize, usize) {
        let mut line = 1;
        let mut column = 1;
        for byte in &self.bytes[..self.pos.min(self.bytes.len())] {
            if *byte == b'\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        (line, column)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    // CEP:WHAT: Handles the bump step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek();
        if byte.is_some() {
            self.pos += 1;
        }
        byte
    }

    fn skip_ws(&mut self) {
        while matches!(
            self.peek(),
            Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
        ) {
            self.pos += 1;
        }
    }

    // CEP:WHAT: Handles the value step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn value(&mut self) -> Result<Json, JsonError> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error("JSON nesting too deep"));
        }
        match self.peek() {
            Some(b'{') => {
                self.depth += 1;
                let value = self.object();
                self.depth -= 1;
                value
            }
            Some(b'[') => {
                self.depth += 1;
                let value = self.array();
                self.depth -= 1;
                value
            }
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.error("expected a JSON value")),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.error("invalid literal"))
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        ) {
            self.bump();
        }
        let raw = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.error("invalid number bytes"))?;
        if raw.parse::<f64>().is_err() {
            return Err(self.error("malformed number"));
        }
        Ok(Json::Number(raw.to_string()))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        if self.bump() != Some(b'"') {
            return Err(self.error("expected string"));
        }
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.bump();
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.bump();
                    let escaped = self
                        .bump()
                        .ok_or_else(|| self.error("unterminated escape"))?;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self.hex4()?;
                            let ch = char::from_u32(hex as u32)
                                .ok_or_else(|| self.error("invalid unicode escape"))?;
                            out.push(ch);
                        }
                        _ => return Err(self.error("unknown escape")),
                    }
                }
                Some(byte) if byte < 0x20 => {
                    return Err(self.error("control character in string"));
                }
                Some(_) => {
                    // Copy one UTF-8 scalar.
                    let rest = &self.bytes[self.pos..];
                    let text =
                        std::str::from_utf8(rest).map_err(|_| self.error("invalid UTF-8"))?;
                    let ch = text
                        .chars()
                        .next()
                        .ok_or_else(|| self.error("empty string body"))?;
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    // CEP:WHAT: Handles the hex4 step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn hex4(&mut self) -> Result<u16, JsonError> {
        let mut value: u16 = 0;
        for _ in 0..4 {
            let byte = self
                .bump()
                .ok_or_else(|| self.error("truncated unicode escape"))?;
            let digit = (byte as char)
                .to_digit(16)
                .ok_or_else(|| self.error("bad hex digit"))?;
            value = value * 16 + digit as u16;
        }
        Ok(value)
    }

    // CEP:WHAT: Handles the array step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn array(&mut self) -> Result<Json, JsonError> {
        self.bump(); // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => return Ok(Json::Array(items)),
                _ => return Err(self.error("expected `,` or `]` in array")),
            }
        }
    }

    // CEP:WHAT: Handles the object step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn object(&mut self) -> Result<Json, JsonError> {
        self.bump(); // {
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Json::Object(map));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.bump() != Some(b':') {
                return Err(self.error("expected `:` after object key"));
            }
            self.skip_ws();
            let value = self.value()?;
            map.insert(key, value);
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => return Ok(Json::Object(map)),
                _ => return Err(self.error("expected `,` or `}` in object")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // CEP:WHAT: Verifies that parses nested document behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn parses_nested_document() {
        let text = r#"{"a": [1, 2, {"b": "x"}], "c": true, "d": null, "e": -3}"#;
        let value = parse(text).expect("parses");
        assert_eq!(value.get("c"), Some(&Json::Bool(true)));
        let arr = value.get("a").and_then(|v| v.as_array()).expect("array");
        assert_eq!(arr.len(), 3);
        assert_eq!(value.get("e").and_then(|v| v.as_i64()), Some(-3));
    }

    // CEP:WHAT: Verifies that error positions are accurate behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn error_positions_are_accurate() {
        let err = parse("{\n  \"a\": ??\n}").expect_err("fails");
        assert_eq!(err.line, 2);
    }

    // CEP:WHAT: Verifies that objects iterate in sorted order behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn objects_iterate_in_sorted_order() {
        let value = parse("{\"z\": 1, \"a\": 2}").expect("parses");
        let keys: Vec<&String> = value.as_object().expect("object").keys().collect();
        assert_eq!(keys, ["a", "z"]);
    }
}
