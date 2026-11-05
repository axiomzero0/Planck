// CEP:FILE: crates/planck_opt/src/lib.rs
// CEP:WHAT: The verified optimization pass pipeline over the Planck IR.
// CEP:WHY: Standard 38.20 requires an explicit, versioned pass pipeline with verification between passes; this crate is that pipeline for milestone 0.1.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Every pass maintains verifier-checked invariants; verify runs after each pass and before returning, so a pass bug surfaces as VerifyError findings rather than miscompilation. Panics are impossible on verifier-valid input.
// CEP:ASSUMES: Input programs pass verify_program. The pipeline order is fixed and versioned as planck-opt-0.1.0; changing it is a semantic event per standard 38.20. Aggregate memory operations (StoreStructFromAddr, StoreIndirect on aggregates) are always treated as effects.
// CEP:COST: Each pass is O(N) or O(N log N) over IR nodes; the full pipeline runs in one sweep per pass with verification O(N + E) between passes.
// CEP:EVIDENCE: cargo test -p planck_opt; golden before/after dumps in crates/planckc/tests.
// CEP:SECURITY: Optimization never increases attack surface: all transformations are semantics-preserving on verified input, and verification gates every mutation.
// CEP:PORTABILITY: Passes are target-independent; machine-specific strength reduction lives in planck_codegen.

#![forbid(unsafe_code)]

pub mod const_fold;
pub mod dce;
pub mod gvn;
pub mod licm;
pub mod peephole;

use planck_ir::{verify_program, Program, VerifyError};

/// The versioned pass pipeline. Order changes are a semantic event (standard 38.20).
pub const PIPELINE_NAME: &str = "planck-opt-0.1.0";
pub const PIPELINE_PASSES: &[&str] = &["const_fold", "gvn_cse", "licm", "peephole", "dce"];

/// A single optimization pass: mutates the program in place, returns the number of changes.
pub type PassFn = fn(&mut Program) -> usize;

/// The ordered pipeline with verification gates.
pub const PIPELINE: &[(&str, PassFn)] = &[
    ("const_fold", const_fold::run),
    ("gvn_cse", gvn::run),
    ("licm", licm::run),
    ("peephole", peephole::run),
    ("dce", dce::run),
];

pub struct OptResult {
    pub total_changes: usize,
    pub per_pass: Vec<(&'static str, usize)>,
    /// Verification findings after the final pass; empty means the pipeline preserved validity.
    pub verify_errors: Vec<VerifyError>,
}

/// Runs the full pipeline with a verifier gate after each pass.
// CEP:WHAT: Handles the optimize step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn optimize(program: &mut Program) -> OptResult {
    let mut per_pass = Vec::new();
    let mut total = 0usize;
    for (name, pass) in PIPELINE {
        let changes = pass(program);
        let findings = verify_program(program);
        if !findings.is_empty() {
            // A pass produced invalid IR: this is a compiler bug surfaced by the gate.
            // Return immediately with the findings so the driver can report an ICE.
            return OptResult {
                total_changes: total,
                per_pass,
                verify_errors: findings,
            };
        }
        per_pass.push((*name, changes));
        total += changes;
    }
    OptResult {
        total_changes: total,
        per_pass,
        verify_errors: Vec::new(),
    }
}
