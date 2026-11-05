// CEP:FILE: crates/planck_lexer/src/lib.rs
// CEP:WHAT: Planck surface tokenizer producing a token stream with byte-accurate spans and structured errors.
// CEP:WHY: The parser needs one pass, deterministic tokens with positions; lexical decisions (comments, literals, attributes) live here and nowhere else.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: LexError records are emitted for unterminated literals, invalid escapes, malformed numeric literals, and unknown characters; the lexer recovers at the next character and never panics.
// CEP:ASSUMES: Input is valid UTF-8. Block comments do not nest (documented deviation ticket PLK-014). Numeric literals are decimal digits with optional single fractional/exponent part.
// CEP:COST: O(N) single scan for N bytes, O(1) amortized per token; allocations only for identifier/literal payload strings.
// CEP:EVIDENCE: cargo test -p planck_lexer (unit + golden token streams).
// CEP:SECURITY: Source text is untrusted; scanning is bounds-checked and recursion-free, so hostile input cannot exhaust the stack.
// CEP:PORTABILITY: Pure core; no platform dependence.

#![forbid(unsafe_code)]

mod cursor;
mod token;

pub use cursor::Lexer;
pub use token::{Lit, TokKind, Token};

use planck_support::Diag;

/// Result of lexing: tokens always accompany any errors so the parser can continue.
pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub errors: Vec<Diag>,
}

/// Tokenizes `source` from the file named `file_name` (used only for diagnostics elsewhere).
// CEP:WHAT: Handles the lex step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn lex(source: &str) -> LexOutput {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.run();
    let errors = lexer.take_errors();
    LexOutput { tokens, errors }
}
