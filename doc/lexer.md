# Lexer

## Abstract

The lexer converts Planck source text into a token stream with byte-accurate spans. Its inputs are UTF-8 source bytes; its outputs are a dense token vector ending in an Eof token and a list of lexical diagnostics. Lexical decisions (comment forms, literal escapes, attribute bodies) are made here and nowhere else.

## Formal Definitions

```rust
pub struct Token { pub kind: TokKind, pub span: Span }
pub enum TokKind { Ident(String), Lit(Lit), Fn, Let, /* ... */ Attr(String), Eof }
pub enum Lit { Int(u64), Float(f64), Bool(bool), Char(char), Str(String) }
pub struct LexOutput { pub tokens: Vec<Token>, pub errors: Vec<Diag> }
```

A `Span` is a half-open byte range `[start, end)` with `start <= end`. Token payloads carry parsed values; downstream stages never re-parse literal text.

## Algorithm

1. Skip trivia: whitespace, `//` line comments, `/* */` block comments (non-nesting).
2. Dispatch on the first byte: identifiers/keywords share one scan with a table lookup; numbers scan digits with a digit-after-dot rule so `0..10` stays two integers; strings and characters decode escapes (`\n`, `\t`, `\r`, `\0`, `\\`, `\'`, `\"`, `\u{...}`); attributes `#[...]` scan to the matching `]`.
3. Multi-character operators resolve by two-byte lookahead (`::`, `->`, `=>`, `..=`, `<<`, `>>`, `&&`, `||`, `==`, `!=`, `<=`, `>=`).
4. Unknown characters emit `PLK-L0001` and scanning continues at the next UTF-8 boundary.

Time complexity O(N) for N bytes; each byte is visited a bounded number of times. Space is O(tokens).

## Invariants

1. The token stream is non-empty and its last token is `Eof`.
2. Every token's span lies within the source length.
3. Lexical errors never abort the scan; every error carries a span and the stream continues.
4. String and character payloads are fully decoded: no escapes survive into the AST.

## Integration Points

- Depends on: `planck_support` (Span, Diag).
- Depended on by: `planck_parser` (`parse_module` calls `planck_lexer::lex`).

Signature: `pub fn lex(source: &str) -> LexOutput`.

## Known Limitations

- Numeric literals are decimal only; hexadecimal and digit separators are not accepted (PLK-013).
- Block comments do not nest (PLK-014).
- Integer literals are limited to the u64 range; wider literals need design guidance.
