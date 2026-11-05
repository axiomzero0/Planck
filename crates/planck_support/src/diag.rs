// CEP:FILE: crates/planck_support/src/diag.rs
// CEP:WHAT: Structured diagnostics: error and warning records with stable codes, spans, and rendered output.
// CEP:WHY: The driver needs deterministic, sortable, machine-checkable failure reports instead of ad hoc printed strings.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: None; DiagSink accumulates diagnostics and returns them in insertion order sorted deterministically by render_key.
// CEP:ASSUMES: Diagnostic codes are unique per compiler stage and never reused with different meanings.
// CEP:COST: O(1) amortized push; O(D log D) sort at render time for D diagnostics.
// CEP:EVIDENCE: unit tests in this file; end-to-end diagnostic goldens in crates/planckc/tests.
// CEP:SECURITY: Source text may be untrusted; rendering escapes nothing because output is plain text to stderr.

use crate::sourcemap::SourceMap;
use crate::span::Span;
use std::fmt;

/// Severity of a diagnostic. Ordered so that Error sorts as most severe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagKind {
    Error,
    Warning,
}

/// One diagnostic record. `code` is stable, e.g. `PLK-E0101`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    pub kind: DiagKind,
    pub code: String,
    pub message: String,
    pub span: Span,
}

impl Diag {
    pub fn error(code: &str, span: Span, message: String) -> Self {
        Diag {
            kind: DiagKind::Error,
            code: code.to_string(),
            message,
            span,
        }
    }

    pub fn warning(code: &str, span: Span, message: String) -> Self {
        Diag {
            kind: DiagKind::Warning,
            code: code.to_string(),
            message,
            span,
        }
    }

    /// Sort key: severity first, then code, then span start, then message.
    pub fn render_key(&self) -> (u8, String, u32, String) {
        (
            match self.kind {
                DiagKind::Error => 0,
                DiagKind::Warning => 1,
            },
            self.code.clone(),
            self.span.start() as u32,
            self.message.clone(),
        )
    }

    /// Renders `file:line:col: error[CODE]: message` plus a caret line under the source text.
    // CEP:WHAT: Handles the render step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn render(&self, sm: &SourceMap) -> String {
        let lc = sm.line_col(self.span.start());
        let severity = match self.kind {
            DiagKind::Error => "error",
            DiagKind::Warning => "warning",
        };
        let mut out = format!(
            "{}:{}:{}: {}[{}]: {}\n",
            sm.file_name(),
            lc.line,
            lc.col,
            severity,
            self.code,
            self.message
        );
        let line = sm.line_text(self.span.start());
        if !line.is_empty() {
            out.push_str("  | ");
            out.push_str(line);
            out.push('\n');
            let pad = " ".repeat(lc.col.saturating_sub(1) as usize);
            let width = self.span.len().max(1).min(line.len().max(1));
            out.push_str(&format!("  | {}{}\n", pad, "^".repeat(width)));
        }
        out
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}[{}]: {}",
            match self.kind {
                DiagKind::Error => "error",
                DiagKind::Warning => "warning",
            },
            self.code,
            self.message
        )
    }
}

/// Accumulates diagnostics across pipeline stages with deterministic ordering.
#[derive(Default, Debug)]
pub struct DiagSink {
    diags: Vec<Diag>,
}

impl DiagSink {
    pub fn new() -> Self {
        DiagSink { diags: Vec::new() }
    }

    pub fn push(&mut self, diag: Diag) {
        self.diags.push(diag);
    }

    pub fn is_empty(&self) -> bool {
        self.diags.is_empty()
    }

    pub fn has_errors(&self) -> bool {
        self.diags.iter().any(|d| d.kind == DiagKind::Error)
    }

    pub fn error_count(&self) -> usize {
        self.diags
            .iter()
            .filter(|d| d.kind == DiagKind::Error)
            .count()
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Diag> {
        self.diags.iter().filter(|d| d.kind == DiagKind::Warning)
    }

    /// Diagnostics sorted by the stable render key.
    // CEP:WHAT: Handles the sorted step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn sorted(&self) -> Vec<&Diag> {
        let mut refs: Vec<&Diag> = self.diags.iter().collect();
        refs.sort_by_key(|d| d.render_key());
        refs
    }

    /// Renders all diagnostics against one source map, in stable order.
    // CEP:WHAT: Handles the render all step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn render_all(&self, sm: &SourceMap) -> String {
        let mut out = String::new();
        for diag in self.sorted() {
            out.push_str(&diag.render(sm));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::Span;

    // CEP:WHAT: Verifies that sorting is stable and severity first behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn sorting_is_stable_and_severity_first() {
        let mut sink = DiagSink::new();
        sink.push(Diag::warning("PLK-W0001", Span::new(0, 1), "w".into()));
        sink.push(Diag::error("PLK-E0002", Span::new(5, 6), "e2".into()));
        sink.push(Diag::error("PLK-E0001", Span::new(2, 3), "e1".into()));
        let sorted = sink.sorted();
        assert_eq!(sorted[0].code, "PLK-E0001");
        assert_eq!(sorted[1].code, "PLK-E0002");
        assert_eq!(sorted[2].code, "PLK-W0001");
        assert!(sink.has_errors());
        assert_eq!(sink.error_count(), 2);
    }
}
