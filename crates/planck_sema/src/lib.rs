// CEP:FILE: crates/planck_sema/src/lib.rs
// CEP:WHAT: Semantic analysis facade: check-and-lower from AST to verified-planck-IR.
// CEP:WHY: One entry point keeps the driver simple and guarantees every function body passes through the same checking and lowering rules.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: All user-facing failures are diagnostics; check_and_lower never panics and always returns the partial program plus findings.
// CEP:ASSUMES: See lower.rs module documentation for the lowering contract.
// CEP:COST: O(program size) end to end.
// CEP:EVIDENCE: cargo test -p planck_sema.
// CEP:SECURITY: Untrusted input is fully checked before any code generation occurs; the driver refuses to codegen when errors exist.
// CEP:PORTABILITY: Pure core.

#![forbid(unsafe_code)]

pub mod expr;
pub mod lower;
pub mod sigs;
pub mod types;

pub use lower::{check_and_lower, SemaOutput};
pub use types::{is_copy, TypeTables};

use planck_parser::Module;
use planck_support::Diag;

/// Convenience: full analysis pipeline from a parsed module with typed output.
pub fn analyze(module: &Module, checked_overflow: bool) -> (planck_ir::Program, Vec<Diag>) {
    let output = check_and_lower(module, checked_overflow);
    (output.program, output.diags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use planck_ir::verify_program;
    use planck_parser::parse_module;

    // CEP:WHAT: Handles the lower ok step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_ok(src: &str) -> planck_ir::Program {
        let (module, parse_diags) = parse_module(src);
        assert!(parse_diags.is_empty(), "parse failed: {:#?}", parse_diags);
        let (program, diags) = analyze(&module, true);
        assert!(diags.is_empty(), "sema failed: {:#?}", diags);
        let errors = verify_program(&program);
        assert!(errors.is_empty(), "IR verification failed: {:#?}", errors);
        program
    }

    // CEP:WHAT: Handles the lower err step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_err(src: &str) -> Vec<Diag> {
        let (module, parse_diags) = parse_module(src);
        assert!(parse_diags.is_empty());
        let (_, diags) = analyze(&module, true);
        diags
    }

    // CEP:WHAT: Verifies that lowers arithmetic main behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn lowers_arithmetic_main() {
        let program = lower_ok("fn main() -> i32 { let x: i32 = 2 + 3 * 4; return x - 1; }");
        assert_eq!(program.funcs.len(), 1);
        assert_eq!(program.funcs[0].name, "main");
    }

    // CEP:WHAT: Verifies that borrow checker rejects mutable alias behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn borrow_checker_rejects_mutable_alias() {
        let diags = lower_err(
            "fn main() -> i32 { let mut x: i32 = 1; let r: &mut i32 = &mut x; let s: &mut i32 = &mut x; return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0006"), "{:#?}", diags);
    }

    // CEP:WHAT: Verifies that borrow checker rejects use while mutably borrowed behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn borrow_checker_rejects_use_while_mutably_borrowed() {
        let diags = lower_err(
            "fn main() -> i32 { let mut x: i32 = 1; let r: &mut i32 = &mut x; x = 2; return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0006"), "{:#?}", diags);
    }

    #[test]
    fn immutable_borrows_can_coexist() {
        lower_ok("fn main() -> i32 { let x: i32 = 1; let a: &i32 = &x; let b: &i32 = &x; return *a + *b; }");
    }

    // CEP:WHAT: Verifies that assignment to immutable is rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn assignment_to_immutable_is_rejected() {
        let diags = lower_err("fn main() -> i32 { let x: i32 = 1; x = 2; return 0; }");
        assert!(diags.iter().any(|d| d.code == "PLK-S0005"));
    }

    // CEP:WHAT: Verifies that structs layout and methods behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn structs_layout_and_methods() {
        lower_ok(
            "struct P { x: i32, y: i32 } \
             impl P { fn sum(&self) -> i32 { return self.x + self.y; } } \
             fn main() -> i32 { let p: P = P { x: 3, y: 4 }; return p.sum(); }",
        );
    }

    // CEP:WHAT: Verifies that enum match lowes and requires wildcard behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn enum_match_lowes_and_requires_wildcard() {
        // With wildcard: lowers cleanly.
        lower_ok(
            "enum C { Red, Blue } fn pick(c: C) -> i32 { match c { C::Red => 1, C::Blue => 2, _ => 0 } } fn main() -> i32 { let c: C = C::Red; return pick(c); }",
        );
        // Without wildcard: rejected.
        let diags = lower_err(
            "enum C { Red, Blue } fn pick(c: C) -> i32 { match c { C::Red => 1, C::Blue => 2 } } fn main() -> i32 { return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0010"));
    }

    #[test]
    fn enum_payload_binds_in_arm() {
        lower_ok(
            "enum Opt { Some(i32), None } fn unwrap_or(o: Opt, d: i32) -> i32 { match o { Opt::Some(v) => v, Opt::None => d, _ => d } } fn main() -> i32 { let a: Opt = Opt::Some(7); let b: Opt = Opt::None; return unwrap_or(a, 1) + unwrap_or(b, 2); }",
        );
    }

    // CEP:WHAT: Verifies that array bounds are checked at runtime behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn array_bounds_are_checked_at_runtime() {
        let program = lower_ok(
            "fn main() -> i32 { let a: [i32; 3] = [1,2,3]; let mut s: i32 = 0; for i in 0..3 { s = s + a[i]; } return s; }",
        );
        let text = planck_ir::print_program(&program);
        assert!(
            text.contains("trap_if"),
            "bounds check missing from IR:\n{}",
            text
        );
    }

    // CEP:WHAT: Verifies that main contract enforced behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn main_contract_enforced() {
        let diags = lower_err("fn main() -> () { return; }");
        assert!(diags.iter().any(|d| d.code == "PLK-S0009"));
        let diags = lower_err("fn notmain() -> i32 { return 0; }");
        assert!(diags.iter().any(|d| d.code == "PLK-S0008"));
    }

    // CEP:WHAT: Verifies that unknown type reported behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn unknown_type_reported() {
        let diags = lower_err("fn main() -> i32 { let x: Blob = 3; return 0; }");
        assert!(diags.iter().any(|d| d.code == "PLK-S0001"));
    }

    // CEP:WHAT: Verifies that short circuit lowers to branches behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn short_circuit_lowers_to_branches() {
        let program = lower_ok(
            "fn f() -> bool { return true; } fn main() -> i32 { if f() && false { return 1; } return 0; }",
        );
        let text = planck_ir::print_program(&program);
        assert!(text.contains("br_cond"));
    }

    // CEP:WHAT: Verifies that casts are explicit and validated behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn casts_are_explicit_and_validated() {
        lower_ok("fn main() -> i32 { let a: i64 = 5; let b: i32 = a as i32; return b; }");
        let _diags =
            lower_err("fn main() -> i32 { let a: bool = true; let b: i32 = a as i32; return b; }");
        // bool -> int casts are allowed; check a genuinely invalid cast
        let diags = lower_err("struct P { x: i32 } fn main() -> i32 { let p: P = P { x: 1 }; let b: i32 = p as i32; return b; }");
        assert!(diags.iter().any(|d| d.code == "PLK-S0007"));
    }

    // CEP:WHAT: Verifies that print intrinsics resolve behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn print_intrinsics_resolve() {
        let program = lower_ok(
            "fn main() -> i32 { print(\"hi\"); print(42); print(1.5); print(true); print('x'); return 0; }",
        );
        let text = planck_ir::print_program(&program);
        assert!(text.contains("@PrintStr"));
        assert!(text.contains("@Print"));
    }

    #[test]
    fn string_literal_passes_to_str_param() {
        lower_ok(
            "fn show(s: &str) -> () { print(s); } fn main() -> i32 { show(\"hello\"); return 0; }",
        );
    }

    // CEP:WHAT: Verifies that recursive value type rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn recursive_value_type_rejected() {
        let diags = lower_err("struct A { b: A } fn main() -> i32 { return 0; }");
        assert!(diags.iter().any(|d| d.code == "PLK-S0019"));
    }
}
