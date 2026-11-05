// CEP:FILE: crates/planck_opt/tests/pipeline.rs
// CEP:WHAT: Pipeline regression gate: runs each optimization pass in order and requires verification to stay clean after every pass.
// CEP:WHY: Standard 38.18 requires verification after each pass; this test pins that contract for the whole pipeline on a representative control-flow program.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: Panics naming the first pass whose output fails verification.
// CEP:ASSUMES: The fixture program exercises branches, a loop, checked arithmetic, and division.
// CEP:COST: One full pipeline run per test execution.
// CEP:EVIDENCE: this test.
use planck_ir::verify_program;
use planck_opt::{const_fold, dce, gvn, licm, peephole};

fn source() -> &'static str {
    "fn main() -> i32 { let mut i: i32 = 0; let mut acc: i32 = 0; while i < 5 { if i % 2 == 0 { acc = acc + i; } else { acc = acc + 1; } i = i + 1; } return acc; }"
}

// CEP:WHAT: Verifies that bisect passes behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn bisect_passes() {
    let (module, pd) = planck_parser::parse_module(source());
    assert!(pd.is_empty());
    let (mut program, sd) = planck_sema::analyze(&module, true);
    assert!(sd.is_empty(), "{:?}", sd);
    type PassEntry = (&'static str, fn(&mut planck_ir::Program) -> usize);
    let passes: Vec<PassEntry> = vec![
        ("const_fold", const_fold::run),
        ("gvn", gvn::run),
        ("licm", licm::run),
        ("peephole", peephole::run),
        ("dce", dce::run),
    ];
    for (name, pass) in passes {
        pass(&mut program);
        let findings = verify_program(&program);
        assert!(
            findings.is_empty(),
            "pass {} broke verification: {:?}",
            name,
            findings.first()
        );
    }
}
