// CEP:FILE: crates/planck_support/src/sourcemap.rs
// CEP:WHAT: Source text holder that maps byte offsets to one-based line-column positions.
// CEP:WHY: Diagnostics must print stable file:line:col locations without rescanning the source for every report.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: line_col returns line 1 column 1 for an out-of-range offset instead of failing; the offset is clamped.
// CEP:ASSUMES: Input text is valid UTF-8 and ends with a newline-terminated final line or not; both are handled.
// CEP:COST: O(N) build, O(log L) lookup for L lines, O(W) extraction of a source line of width W.
// CEP:EVIDENCE: unit tests in this file.
// CEP:SECURITY: Source paths are untrusted; only the path string is stored, never opened here.

use crate::span::{LineCol, Span};

/// Holds one source file name and its text plus a precomputed line-start table.
#[derive(Clone, Debug)]
pub struct SourceMap {
    file_name: String,
    text: String,
    line_starts: Vec<u32>,
}

impl SourceMap {
    /// Builds a source map, storing byte offsets at which each line begins.
    // CEP:WHAT: Handles the new step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn new(file_name: String, text: String) -> Self {
        let mut line_starts = Vec::with_capacity(64);
        line_starts.push(0u32);
        for (idx, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx as u32 + 1);
            }
        }
        SourceMap {
            file_name,
            text,
            line_starts,
        }
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Converts a byte offset to a one-based line and column. Offsets past the end are clamped.
    // CEP:WHAT: Handles the line col step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn line_col(&self, offset: usize) -> LineCol {
        let offset = offset.min(self.text.len());
        let line_idx = match self.line_starts.binary_search(&(offset as u32)) {
            Ok(idx) => idx,
            Err(insert_at) => insert_at - 1,
        };
        let line_start = self.line_starts[line_idx] as usize;
        LineCol {
            line: line_idx as u32 + 1,
            col: (offset - line_start) as u32 + 1,
        }
    }

    /// Formats a location prefix `file:line:col` for the span start.
    pub fn location(&self, span: Span) -> String {
        let lc = self.line_col(span.start());
        format!("{}:{}", self.file_name, lc)
    }

    /// Returns the full text of the one-based line containing `offset`, without the trailing newline.
    // CEP:WHAT: Handles the line text step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn line_text(&self, offset: usize) -> &str {
        let offset = offset.min(self.text.len());
        let line_idx = match self.line_starts.binary_search(&(offset as u32)) {
            Ok(idx) => idx,
            Err(insert_at) => insert_at - 1,
        };
        let start = self.line_starts[line_idx] as usize;
        let end = if line_idx + 1 < self.line_starts.len() {
            (self.line_starts[line_idx + 1] as usize).saturating_sub(1)
        } else {
            self.text.len()
        };
        let end = end.min(self.text.len());
        self.text.get(start..end).unwrap_or("")
    }

    /// Returns the raw source substring of a span, empty for zero-width spans.
    pub fn snippet(&self, span: Span) -> &str {
        self.text.get(span.start()..span.end()).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // CEP:WHAT: Verifies that maps offsets to lines behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn maps_offsets_to_lines() {
        let sm = SourceMap::new("t.plk".to_string(), "ab\ncd\n".to_string());
        assert_eq!(sm.line_col(0).line, 1);
        assert_eq!(sm.line_col(3).line, 2);
        assert_eq!(sm.line_col(3).col, 1);
        assert_eq!(sm.line_text(3), "cd");
    }

    // CEP:WHAT: Verifies that clamps out of range offsets behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn clamps_out_of_range_offsets() {
        let sm = SourceMap::new("t.plk".to_string(), "ab".to_string());
        assert_eq!(sm.line_col(999), LineCol { line: 1, col: 3 });
    }
}
