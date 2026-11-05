// CEP:FILE: crates/planck_lexer/src/cursor.rs
// CEP:WHAT: Single-pass scanning engine that converts Planck source text into a token stream.
// CEP:WHY: All lexical ambiguity resolution (dot ranges versus field access, path separators versus colon, attribute bodies, escapes) is centralized here so the parser never re-examines raw text.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Emits Diag errors with codes PLK-L0001 through PLK-L0006 for invalid characters, unterminated strings, block comments, character literals, invalid escapes, and malformed numbers; scanning continues after each error.
// CEP:ASSUMES: Input is valid UTF-8. Numeric literals are decimal only; hexadecimal and digit separators are not supported in this milestone (ticket PLK-013). Block comments do not nest (ticket PLK-014).
// CEP:COST: O(N) over N bytes; each byte is visited a bounded number of times (at most twice: once to test, once to consume); allocation occurs only for identifier and literal payloads.
// CEP:EVIDENCE: cargo test -p planck_lexer.
// CEP:SECURITY: Input is untrusted; every access is via bounds-checked byte reads and checked UTF-8 slicing, so malformed input degrades to diagnostics, never memory unsafety.
// CEP:PORTABILITY: Pure core; no platform dependence.

use crate::token::{Lit, TokKind, Token};
use planck_support::{Diag, Span};

const ERR_UNKNOWN_CHAR: &str = "PLK-L0001";
const ERR_UNTERMINATED_STRING: &str = "PLK-L0002";
const ERR_UNTERMINATED_BLOCK_COMMENT: &str = "PLK-L0003";
const ERR_UNTERMINATED_CHAR: &str = "PLK-L0004";
const ERR_INVALID_ESCAPE: &str = "PLK-L0005";
const ERR_MALFORMED_NUMBER: &str = "PLK-L0006";

/// Cursor-based lexer over the raw source bytes.
pub struct Lexer<'src> {
    bytes: &'src [u8],
    text: &'src str,
    pos: usize,
    tokens: Vec<Token>,
    errors: Vec<Diag>,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Lexer {
            bytes: source.as_bytes(),
            text: source,
            pos: 0,
            tokens: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Runs the full scan and returns the token list; the stream always ends with an Eof token.
    // CEP:WHAT: Handles the run step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn run(&mut self) -> Vec<Token> {
        loop {
            self.skip_trivia();
            if self.pos >= self.bytes.len() {
                let span = Span::new(self.pos, self.pos);
                self.tokens.push(Token::new(TokKind::Eof, span));
                return std::mem::take(&mut self.tokens);
            }
            self.next_token();
        }
    }

    pub fn take_errors(&mut self) -> Vec<Diag> {
        std::mem::take(&mut self.errors)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
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

    fn error(&mut self, code: &str, start: usize, message: String) {
        self.errors
            .push(Diag::error(code, Span::new(start, self.pos), message));
    }

    /// Skips whitespace and comments.
    // CEP:WHAT: Handles the skip trivia step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => {
                    self.pos += 1;
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    while let Some(byte) = self.peek() {
                        if byte == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    let start = self.pos;
                    self.pos += 2;
                    loop {
                        match self.peek() {
                            None => {
                                self.error(
                                    ERR_UNTERMINATED_BLOCK_COMMENT,
                                    start,
                                    "unterminated block comment".to_string(),
                                );
                                break;
                            }
                            Some(b'*') if self.peek_at(1) == Some(b'/') => {
                                self.pos += 2;
                                break;
                            }
                            Some(_) => self.pos += 1,
                        }
                    }
                }
                _ => break,
            }
        }
    }

    // CEP:WHAT: Handles the next token step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn next_token(&mut self) {
        let start = self.pos;
        let byte = match self.bump() {
            Some(byte) => byte,
            None => return,
        };
        let kind = match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_ident_or_keyword(start),
            b'0'..=b'9' => self.lex_number(start),
            b'"' => self.lex_string(start),
            b'\'' => self.lex_char(start),
            b'(' => TokKind::LParen,
            b')' => TokKind::RParen,
            b'{' => TokKind::LBrace,
            b'}' => TokKind::RBrace,
            b'[' => TokKind::LBracket,
            b']' => TokKind::RBracket,
            b',' => TokKind::Comma,
            b';' => TokKind::Semi,
            b'#' => self.lex_attribute(start),
            b':' => {
                if self.peek() == Some(b':') {
                    self.bump();
                    TokKind::PathSep
                } else {
                    TokKind::Colon
                }
            }
            b'-' => {
                if self.peek() == Some(b'>') {
                    self.bump();
                    TokKind::Arrow
                } else {
                    TokKind::Minus
                }
            }
            b'=' => {
                if self.peek() == Some(b'=') {
                    self.bump();
                    TokKind::EqEq
                } else if self.peek() == Some(b'>') {
                    self.bump();
                    TokKind::FatArrow
                } else {
                    TokKind::Eq
                }
            }
            b'!' => {
                if self.peek() == Some(b'=') {
                    self.bump();
                    TokKind::NotEq
                } else {
                    TokKind::Bang
                }
            }
            b'<' => {
                if self.peek() == Some(b'=') {
                    self.bump();
                    TokKind::Le
                } else if self.peek() == Some(b'<') {
                    self.bump();
                    TokKind::Shl
                } else {
                    TokKind::Lt
                }
            }
            b'>' => {
                if self.peek() == Some(b'=') {
                    self.bump();
                    TokKind::Ge
                } else if self.peek() == Some(b'>') {
                    self.bump();
                    TokKind::Shr
                } else {
                    TokKind::Gt
                }
            }
            b'&' => {
                if self.peek() == Some(b'&') {
                    self.bump();
                    TokKind::AndAnd
                } else {
                    TokKind::Amp
                }
            }
            b'|' => {
                if self.peek() == Some(b'|') {
                    self.bump();
                    TokKind::OrOr
                } else {
                    TokKind::Pipe
                }
            }
            b'.' => {
                if self.peek() == Some(b'.') {
                    self.bump();
                    if self.peek() == Some(b'=') {
                        self.bump();
                        TokKind::DotDotEq
                    } else {
                        TokKind::DotDot
                    }
                } else {
                    TokKind::Dot
                }
            }
            b'+' => TokKind::Plus,
            b'*' => TokKind::Star,
            b'/' => TokKind::Slash,
            b'%' => TokKind::Percent,
            b'^' => TokKind::Caret,
            other => {
                self.pos = start;
                let ch_len = utf8_char_len(self.bytes, self.pos);
                let shown = self
                    .text
                    .get(self.pos..self.pos + ch_len)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| char::from_u32(other as u32).unwrap_or('?').to_string());
                self.pos += ch_len;
                self.error(
                    ERR_UNKNOWN_CHAR,
                    start,
                    format!("unknown character `{}`", shown),
                );
                return;
            }
        };
        let span = Span::new(start, self.pos);
        self.tokens.push(Token::new(kind, span));
    }

    fn lex_ident_or_keyword(&mut self, start: usize) -> TokKind {
        while let Some(byte) = self.peek() {
            match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' => self.pos += 1,
                _ => break,
            }
        }
        let word = &self.text[start..self.pos];
        match word {
            "fn" => TokKind::Fn,
            "let" => TokKind::Let,
            "mut" => TokKind::Mut,
            "if" => TokKind::If,
            "else" => TokKind::Else,
            "while" => TokKind::While,
            "loop" => TokKind::Loop,
            "break" => TokKind::Break,
            "continue" => TokKind::Continue,
            "for" => TokKind::For,
            "in" => TokKind::In,
            "return" => TokKind::Return,
            "struct" => TokKind::Struct,
            "enum" => TokKind::Enum,
            "impl" => TokKind::Impl,
            "match" => TokKind::Match,
            "as" => TokKind::As,
            "true" => TokKind::Lit(Lit::Bool(true)),
            "false" => TokKind::Lit(Lit::Bool(false)),
            "self" => TokKind::SelfKw,
            _ => TokKind::Ident(word.to_string()),
        }
    }

    /// Lexes a number. `N.M` becomes a float only when `.` is followed by a digit,
    /// so range expressions like `0..10` lex as two integers.
    // CEP:WHAT: Handles the lex number step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lex_number(&mut self, start: usize) -> TokKind {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') && matches!(self.peek_at(1), Some(b'0'..=b'9')) {
            is_float = true;
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            let mut ahead = 1;
            if matches!(self.peek_at(1), Some(b'+') | Some(b'-')) {
                ahead = 2;
            }
            if matches!(self.peek_at(ahead), Some(b'0'..=b'9')) {
                is_float = true;
                self.pos += ahead;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
        }
        let text = &self.text[start..self.pos];
        if is_float {
            match text.parse::<f64>() {
                Ok(value) if value.is_finite() => TokKind::Lit(Lit::Float(value)),
                _ => {
                    self.error(
                        ERR_MALFORMED_NUMBER,
                        start,
                        format!("float literal `{}` is out of range", text),
                    );
                    TokKind::Lit(Lit::Float(0.0))
                }
            }
        } else {
            match text.parse::<u64>() {
                Ok(value) => TokKind::Lit(Lit::Int(value)),
                Err(_) => {
                    self.error(
                        ERR_MALFORMED_NUMBER,
                        start,
                        format!("integer literal `{}` exceeds the u64 limit", text),
                    );
                    TokKind::Lit(Lit::Int(0))
                }
            }
        }
    }

    // CEP:WHAT: Handles the lex string step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lex_string(&mut self, start: usize) -> TokKind {
        let mut value = String::new();
        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    self.error(
                        ERR_UNTERMINATED_STRING,
                        start,
                        "unterminated string literal".to_string(),
                    );
                    break;
                }
                Some(b'"') => {
                    self.bump();
                    break;
                }
                Some(b'\\') => {
                    let esc_start = self.pos;
                    self.bump();
                    match self.escape(esc_start) {
                        Ok(Some(ch)) => value.push(ch),
                        Ok(None) => {}
                        Err(()) => {}
                    }
                }
                Some(_) => {
                    let ch_len = utf8_char_len(self.bytes, self.pos);
                    if let Some(chunk) = self.text.get(self.pos..self.pos + ch_len) {
                        value.push_str(chunk);
                    }
                    self.pos += ch_len;
                }
            }
        }
        TokKind::Lit(Lit::Str(value))
    }

    // CEP:WHAT: Handles the lex char step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lex_char(&mut self, start: usize) -> TokKind {
        let mut value: Option<char> = None;
        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    self.error(
                        ERR_UNTERMINATED_CHAR,
                        start,
                        "unterminated character literal".to_string(),
                    );
                    value = None;
                    break;
                }
                Some(b'\'') => {
                    self.bump();
                    break;
                }
                Some(b'\\') => {
                    let esc_start = self.pos;
                    self.bump();
                    match self.escape(esc_start) {
                        Ok(Some(ch)) => value = Some(ch),
                        Ok(None) => {}
                        Err(()) => value = None,
                    }
                }
                Some(_) => {
                    let ch_len = utf8_char_len(self.bytes, self.pos);
                    let chunk = self.text.get(self.pos..self.pos + ch_len);
                    match chunk.and_then(|s| s.chars().next()) {
                        Some(ch) => {
                            if value.is_some() {
                                self.error(
                                    ERR_MALFORMED_NUMBER,
                                    start,
                                    "character literal holds more than one scalar".to_string(),
                                );
                            } else {
                                value = Some(ch);
                            }
                            self.pos += ch_len;
                        }
                        None => {
                            self.pos += 1;
                        }
                    }
                }
            }
        }
        let span = Span::new(start, self.pos);
        match value {
            Some(ch) => TokKind::Lit(Lit::Char(ch)),
            None => {
                self.errors.push(Diag::error(
                    ERR_INVALID_ESCAPE,
                    span,
                    "character literal could not be decoded".to_string(),
                ));
                TokKind::Lit(Lit::Char('\0'))
            }
        }
    }

    /// Decodes one escape sequence after the backslash. Ok(None) means "skip silently".
    // CEP:WHAT: Handles the escape step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn escape(&mut self, esc_start: usize) -> Result<Option<char>, ()> {
        let byte = match self.bump() {
            Some(byte) => byte,
            None => {
                self.error(
                    ERR_INVALID_ESCAPE,
                    esc_start,
                    "incomplete escape sequence".to_string(),
                );
                return Err(());
            }
        };
        let ch = match byte {
            b'n' => '\n',
            b't' => '\t',
            b'r' => '\r',
            b'0' => '\0',
            b'\\' => '\\',
            b'\'' => '\'',
            b'"' => '"',
            b'u' => return self.escape_unicode(esc_start),
            other => {
                let shown = char::from_u32(other as u32).unwrap_or('?');
                self.error(
                    ERR_INVALID_ESCAPE,
                    esc_start,
                    format!("unknown escape `\\{}`", shown),
                );
                return Err(());
            }
        };
        Ok(Some(ch))
    }

    /// Decodes `\u{HEX}` into a Unicode scalar value.
    // CEP:WHAT: Handles the escape unicode step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn escape_unicode(&mut self, esc_start: usize) -> Result<Option<char>, ()> {
        if self.peek() != Some(b'{') {
            self.error(
                ERR_INVALID_ESCAPE,
                esc_start,
                "expected `{` after `\\u`".to_string(),
            );
            return Err(());
        }
        self.bump();
        let mut hex = String::new();
        loop {
            match self.peek() {
                Some(b'}') => {
                    self.bump();
                    break;
                }
                Some(byte) if byte.is_ascii_hexdigit() => {
                    hex.push(byte as char);
                    self.bump();
                }
                _ => {
                    self.error(
                        ERR_INVALID_ESCAPE,
                        esc_start,
                        "unterminated unicode escape".to_string(),
                    );
                    return Err(());
                }
            }
        }
        if hex.is_empty() || hex.len() > 6 {
            self.error(
                ERR_INVALID_ESCAPE,
                esc_start,
                "unicode escape must hold 1 to 6 hex digits".to_string(),
            );
            return Err(());
        }
        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
            Some(ch) => Ok(Some(ch)),
            None => {
                self.error(
                    ERR_INVALID_ESCAPE,
                    esc_start,
                    format!("`\\u{{{}}}` is not a Unicode scalar value", hex),
                );
                Err(())
            }
        }
    }

    /// Lexes `#[...]` into one Attr token holding the inner text.
    // CEP:WHAT: Handles the lex attribute step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lex_attribute(&mut self, start: usize) -> TokKind {
        if self.peek() != Some(b'[') {
            self.error(
                ERR_UNKNOWN_CHAR,
                start,
                "expected `[` after `#`".to_string(),
            );
            return TokKind::Pound;
        }
        self.bump();
        let content_start = self.pos;
        loop {
            match self.peek() {
                Some(b']') => break,
                None | Some(b'\n') => {
                    self.error(
                        ERR_UNKNOWN_CHAR,
                        start,
                        "unterminated attribute".to_string(),
                    );
                    break;
                }
                Some(_) => self.pos += 1,
            }
        }
        let content = self.text[content_start..self.pos.min(self.text.len())]
            .trim()
            .to_string();
        if self.peek() == Some(b']') {
            self.bump();
        }
        TokKind::Attr(content)
    }
}

/// Returns the byte length of the UTF-8 character starting at `pos`, always at least 1.
fn utf8_char_len(bytes: &[u8], pos: usize) -> usize {
    match bytes.get(pos) {
        None => 1,
        Some(first) => {
            let len = if *first < 0x80 {
                1
            } else if *first < 0xE0 {
                2
            } else if *first < 0xF0 {
                3
            } else {
                4
            };
            len.min(bytes.len() - pos)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex;

    fn kinds(src: &str) -> Vec<TokKind> {
        lex(src).tokens.into_iter().map(|t| t.kind).collect()
    }

    // CEP:WHAT: Verifies that lexes arithmetic and ranges behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn lexes_arithmetic_and_ranges() {
        let ks = kinds("let a = 0..10; let b = 1.5e2;");
        assert_eq!(ks[3], TokKind::Lit(Lit::Int(0)));
        assert_eq!(ks[4], TokKind::DotDot);
        assert_eq!(ks[5], TokKind::Lit(Lit::Int(10)));
        assert!(ks.contains(&TokKind::Lit(Lit::Float(150.0))));
    }

    // CEP:WHAT: Verifies that lexes paths and fat arrows behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn lexes_paths_and_fat_arrows() {
        let ks = kinds("Color::Red => 1");
        assert!(ks.contains(&TokKind::PathSep));
        assert!(ks.contains(&TokKind::FatArrow));
    }

    // CEP:WHAT: Verifies that lexes attribute token behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn lexes_attribute_token() {
        let ks = kinds("#[repr(C)] struct P { x: i32 }");
        assert_eq!(ks[0], TokKind::Attr("repr(C)".to_string()));
    }

    // CEP:WHAT: Verifies that reports unknown character and recovers behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn reports_unknown_character_and_recovers() {
        let out = lex("let a = 3; @ let b = 4;");
        assert_eq!(out.errors.len(), 1);
        assert_eq!(out.errors[0].code, "PLK-L0001");
        assert!(out
            .tokens
            .iter()
            .any(|t| t.kind == TokKind::Lit(Lit::Int(4))));
    }

    // CEP:WHAT: Verifies that unterminated string is error behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn unterminated_string_is_error() {
        let out = lex("let s = \"abc");
        assert_eq!(out.errors.len(), 1);
        assert_eq!(out.errors[0].code, "PLK-L0002");
    }

    // CEP:WHAT: Verifies that escapes decode behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn escapes_decode() {
        let out = lex("\"a\\n\\u{41}\\t\"");
        match &out.tokens[0].kind {
            TokKind::Lit(Lit::Str(s)) => assert_eq!(s, "a\nA\t"),
            other => panic!("unexpected token: {:?}", other),
        }
        assert!(out.errors.is_empty());
    }

    // CEP:WHAT: Verifies that block comment requires close behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn block_comment_requires_close() {
        let out = lex("/* open");
        assert_eq!(out.errors[0].code, "PLK-L0003");
    }

    // CEP:WHAT: Verifies that integer dot without digit stays integer behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn integer_dot_without_digit_stays_integer() {
        let ks = kinds("for i in 0..=10 {}");
        assert!(ks.contains(&TokKind::DotDotEq));
    }
}
