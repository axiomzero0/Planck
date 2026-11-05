// CEP:FILE: crates/planck_support/src/span.rs
// CEP:WHAT: Byte-offset span and line-column position types shared by all pipeline stages.
// CEP:WHY: A single span vocabulary lets the parser, analyzers, and the driver point at one exact source region without converting representations.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: None; constructors are total and clamping is explicit in Span::new.
// CEP:ASSUMES: Offsets are byte offsets into a single UTF-8 source buffer.
// CEP:COST: All operations are O(1) with no allocation.
// CEP:EVIDENCE: unit tests in this file.
// CEP:SECURITY: none; spans carry no untrusted payload.

use core::fmt;

/// Half-open byte range `[start, end)` inside one source file. `start <= end` always holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    start: u32,
    end: u32,
}

impl Span {
    /// Creates a span, clamping `end` to `start` if the caller passes an inverted range.
    // CEP:WHAT: Handles the new step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn new(start: usize, end: usize) -> Self {
        let start = u32::try_from(start).unwrap_or(u32::MAX);
        let end = u32::try_from(end).unwrap_or(u32::MAX);
        Span {
            start,
            end: end.max(start),
        }
    }

    pub fn start(self) -> usize {
        self.start as usize
    }

    pub fn end(self) -> usize {
        self.end as usize
    }

    pub fn len(self) -> usize {
        (self.end - self.start) as usize
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Smallest span covering both inputs.
    pub fn join(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// A zero-width span at the start of `self`, used when a token is missing.
    pub fn to_point(self) -> Span {
        Span {
            start: self.start,
            end: self.start,
        }
    }

    /// The zero span at offset 0, used when no better location exists.
    pub fn new_zero() -> Span {
        Span { start: 0, end: 0 }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// One-based line and column pair. Column counts bytes from line start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for LineCol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // CEP:WHAT: Verifies that join covers both spans behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn join_covers_both_spans() {
        let a = Span::new(2, 5);
        let b = Span::new(7, 9);
        assert_eq!(a.join(b), Span::new(2, 9));
    }

    // CEP:WHAT: Verifies that new clamps inverted range behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn new_clamps_inverted_range() {
        let s = Span::new(9, 4);
        assert_eq!(s.start(), 9);
        assert_eq!(s.end(), 9);
        assert!(s.is_empty());
    }
}
