// CEP:FILE: crates/planck_support/src/lib.rs
// CEP:WHAT: Shared support types for the Planck compiler pipeline: byte spans, source maps, and structured diagnostics.
// CEP:WHY: Every pipeline stage reports positions and failures through one deterministic vocabulary so diagnostics are stable across stages and runs.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Constructors return values, never panic. Rendering requires a populated SourceMap; see Diag::render.
// CEP:ASSUMES: Source text is valid UTF-8; line indexing is defined on UTF-8 bytes with ASCII newline as line terminator.
// CEP:COST: O(N) source map build for N input bytes; O(log N) line lookup by binary search; no allocation in lookups.
// CEP:EVIDENCE: cargo test -p planck_support; golden diagnostics in crates/planckc/tests.
// CEP:SECURITY: Source text is untrusted input; spans are clamped to text length and never index out of bounds.
// CEP:PORTABILITY: Pure core; no platform dependence.

#![forbid(unsafe_code)]

pub mod diag;
pub mod sourcemap;
pub mod span;

pub use diag::{Diag, DiagKind, DiagSink};
pub use sourcemap::SourceMap;
pub use span::{LineCol, Span};
