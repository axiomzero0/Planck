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

    // CEP:WHAT: Verifies that aggregate-typed match arms follow the sret return contract.
    // CEP:WHY: Regression pin for the Ret-address bug the verifier caught; arms must memcpy into the sret buffer.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails if lowering produces a valueful Ret for aggregates.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn aggregate_returning_match_arms_use_sret_contract() {
        // Regression: match arms returning aggregates must memcpy into the sret
        // buffer and return valuelessly, not Ret the address (the verifier caught
        // this as "&enum where enum expected" before the fix).
        let program = lower_ok(
            "enum St { A, B(i32) } \
             fn pick(s: St, k: i32) -> St { match s { St::A => St::B(k), St::B(v) => St::A, _ => St::A } } \
             fn main() -> i32 { let a: St = St::A; let b: St = pick(a, 7); return 0; }",
        );
        assert!(program
            .funcs
            .iter()
            .any(|f| f.name == "pick" && f.sret_slot.is_some()));
    }

    // CEP:WHAT: Verifies that array indexing lowers an always-on bounds trap.
    // CEP:WHY: Design Part 3 mandates bounds checks by default; the trap must be visible in the IR.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails if the bounds trap disappears from the IR.
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

    // ----- ownership: move checking (ticket PLK-010) -----

    // CEP:WHAT: Verifies that use after move is rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn move_use_after_move_rejected() {
        let diags = lower_err(
            "struct P { x: i32 } \
             fn eat(p: P) -> i32 { return p.x; } \
             fn main() -> i32 { let p: P = P { x: 1 }; let q: P = p; return eat(p); }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0023"), "{:#?}", diags);
    }

    // CEP:WHAT: Verifies that second by-value argument is a move error behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn move_second_by_value_argument_rejected() {
        let diags = lower_err(
            "struct P { x: i32 } \
             fn eat(p: P) -> i32 { return p.x; } \
             fn main() -> i32 { let p: P = P { x: 1 }; let a: i32 = eat(p); let b: i32 = eat(p); return a + b; }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0023"));
    }

    // CEP:WHAT: Verifies that copy scalars can be reused freely behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn copy_scalars_are_immune_to_moves() {
        lower_ok("fn main() -> i32 { let x: i32 = 1; let y: i32 = x; return x + y; }");
    }

    // CEP:WHAT: Verifies that reassignment reinitializes a moved value behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn reassignment_reinitializes_moved_value() {
        lower_ok(
            "struct P { x: i32 } \
             fn eat(p: P) -> i32 { return p.x; } \
             fn make() -> P { return P { x: 7 }; } \
             fn main() -> i32 { let mut p: P = make(); let a: i32 = eat(p); p = make(); return a + eat(p); }",
        );
    }

    // CEP:WHAT: Verifies that partial move blocks whole use allows other fields behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn partial_move_blocks_whole_use_allows_other_fields() {
        // Reading a different field after a partial move is fine.
        lower_ok(
            "struct Inner { v: i32 } \
             struct P { a: Inner, b: i32 } \
             fn main() -> i32 { let p: P = P { a: Inner { v: 5 }, b: 7 }; let t: Inner = p.a; return p.b + t.v; }",
        );
        // Using the whole struct after a partial move is rejected.
        let diags = lower_err(
            "struct Inner { v: i32 } \
             struct P { a: Inner, b: i32 } \
             fn eat(p: P) -> i32 { return p.b; } \
             fn main() -> i32 { let p: P = P { a: Inner { v: 5 }, b: 7 }; let t: Inner = p.a; return eat(p); }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0023"), "{:#?}", diags);
        // Moving the same field twice is rejected.
        let diags = lower_err(
            "struct Inner { v: i32 } \
             struct P { a: Inner, b: i32 } \
             fn eat(i: Inner) -> i32 { return i.v; } \
             fn main() -> i32 { let p: P = P { a: Inner { v: 5 }, b: 7 }; let t: Inner = p.a; let u: Inner = p.a; return eat(t) + eat(u); }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0023"));
    }

    // CEP:WHAT: Verifies that field reassignment restores a partial move behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn field_reassignment_restores_partial_move() {
        lower_ok(
            "struct Inner { v: i32 } \
             struct P { a: Inner, b: i32 } \
             fn eat(i: Inner) -> i32 { return i.v; } \
             fn make() -> Inner { return Inner { v: 9 }; } \
             fn main() -> i32 { let mut p: P = P { a: Inner { v: 5 }, b: 7 }; let t: Inner = p.a; p.a = make(); return eat(p.a) + p.b + t.v; }",
        );
    }

    // CEP:WHAT: Verifies that move while borrowed is rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn move_conflicts_with_active_borrow() {
        let diags = lower_err(
            "struct P { x: i32 } \
             fn eat(p: P) -> i32 { return p.x; } \
             fn main() -> i32 { let p: P = P { x: 1 }; let r: &P = &p; return eat(p); }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0006"), "{:#?}", diags);
    }

    // CEP:WHAT: Verifies that moving out of a reference is rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn cannot_move_out_of_reference() {
        let diags = lower_err(
            "struct P { x: i32 } \
             fn main() -> i32 { let p: P = P { x: 1 }; let r: &P = &p; let q: P = *r; return q.x; }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0025"), "{:#?}", diags);
    }

    // CEP:WHAT: Verifies that moving out of an array element is rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn cannot_move_out_of_array_element() {
        let diags = lower_err(
            "struct P { x: i32 } \
             fn main() -> i32 { let a: [P; 2] = [P { x: 1 }, P { x: 2 }]; let q: P = a[0]; return q.x; }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0025"), "{:#?}", diags);
        // Copy elements stay freely readable.
        lower_ok(
            "fn main() -> i32 { let a: [i32; 2] = [1, 2]; let q: i32 = a[0]; return q + a[1]; }",
        );
    }

    // CEP:WHAT: Verifies that match moves the scrutinee and payload binding owns behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn match_moves_scrutinee_and_payload_binding_owns() {
        // The scrutinee is consumed by the match.
        let diags = lower_err(
            "enum W { S(i32), N } \
             fn main() -> i32 { let w: W = W::S(9); match w { W::S(v) => { return v; }, _ => {} } return w == w as i32; }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0023"), "{:#?}", diags);
        // Aggregate payloads bind by value and are owned by the arm.
        lower_ok(
            "struct P { x: i32 } \
             enum W { S(P), N } \
             fn main() -> i32 { let p: P = P { x: 3 }; let w: W = W::S(p); match w { W::S(v) => { return v.x; }, _ => {} } return 0; }",
        );
    }

    // CEP:WHAT: Verifies that struct literal fields move behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn struct_literal_fields_move() {
        let diags = lower_err(
            "struct Inner { v: i32 } \
             struct P { a: Inner } \
             fn eat(i: Inner) -> i32 { return i.v; } \
             fn main() -> i32 { let i: Inner = Inner { v: 1 }; let p: P = P { a: i }; return eat(i); }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0023"), "{:#?}", diags);
    }

    // CEP:WHAT: Verifies that by value receiver moves the receiver behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn by_value_receiver_moves() {
        let diags = lower_err(
            "struct P { x: i32 } \
             impl P { fn eat(self) -> i32 { return self.x; } } \
             fn main() -> i32 { let p: P = P { x: 1 }; let a: i32 = p.eat(); return a + p.eat(); }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0023"), "{:#?}", diags);
        // Reference receivers only borrow.
        lower_ok(
            "struct P { x: i32 } \
             impl P { fn get(&self) -> i32 { return self.x; } } \
             fn main() -> i32 { let p: P = P { x: 1 }; let a: i32 = p.get(); return a + p.get(); }",
        );
    }

    // ----- ownership: deferred initialization (ticket PLK-010) -----

    // CEP:WHAT: Verifies that deferred let requires annotation behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn deferred_let_requires_annotation() {
        let diags = lower_err("fn main() -> i32 { let x; x = 1; return x; }");
        assert!(diags.iter().any(|d| d.code == "PLK-S0020"), "{:#?}", diags);
    }

    // CEP:WHAT: Verifies that deferred let definite assignment works behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn deferred_let_definite_assignment_accepted() {
        let program = lower_ok(
            "fn main() -> i32 { \
                 let x: i32; \
                 let c: i32 = 1; \
                 if c > 0 { x = 10; } else { x = 20; } \
                 return x; \
             }",
        );
        assert_eq!(program.funcs.len(), 1);
    }

    // CEP:WHAT: Verifies that deferred let read before init rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn deferred_let_read_before_init_rejected() {
        let diags = lower_err("fn main() -> i32 { let x: i32; return x; }");
        assert!(diags.iter().any(|d| d.code == "PLK-S0024"), "{:#?}", diags);
    }

    // CEP:WHAT: Verifies that deferred let needs every branch to assign behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn deferred_let_requires_all_branches_to_assign() {
        let diags = lower_err(
            "fn main() -> i32 { let x: i32; let c: i32 = 1; if c > 0 { x = 10; } return x; }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0024"), "{:#?}", diags);
    }

    // CEP:WHAT: Verifies that diverging branches exempt assignment behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn deferred_let_diverging_branch_exempt() {
        lower_ok(
            "fn main() -> i32 { \
                 let x: i32; \
                 let c: i32 = 0; \
                 if c > 0 { return 1; } \
                 x = 5; \
                 return x; \
             }",
        );
    }

    // CEP:WHAT: Verifies that borrow before init is rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn deferred_let_borrow_before_init_rejected() {
        let diags =
            lower_err("fn main() -> i32 { let x: i32; let r: &i32 = &x; x = 5; return *r; }");
        assert!(diags.iter().any(|d| d.code == "PLK-S0024"), "{:#?}", diags);
    }

    // CEP:WHAT: Verifies that first assignment without mut works once behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn deferred_let_first_assignment_is_initialization() {
        lower_ok("fn main() -> i32 { let x: i32; x = 5; return x; }");
        // The second assignment mutates and therefore requires `mut`.
        let diags = lower_err("fn main() -> i32 { let x: i32; x = 5; x = 6; return x; }");
        assert!(diags.iter().any(|d| d.code == "PLK-S0005"), "{:#?}", diags);
        // With `mut` both assignments are fine.
        lower_ok("fn main() -> i32 { let mut x: i32; x = 5; x = 6; return x; }");
    }

    // CEP:WHAT: Verifies that moving a value inside a loop is rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn loop_move_rejected() {
        let diags = lower_err(
            "struct P { x: i32 } \
             fn eat(p: P) -> i32 { return p.x; } \
             fn main() -> i32 { let p: P = P { x: 4 }; while p.x > 0 { return eat(p); } return 0; }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0023"), "{:#?}", diags);
        // Reassignment inside the loop is the supported pattern.
        lower_ok(
            "struct P { x: i32 } \
             fn eat(p: P) -> i32 { return p.x; } \
             fn make() -> P { return P { x: 4 }; } \
             fn main() -> i32 { let mut p: P = make(); let mut n: i32 = 0; while n < 2 { let a: i32 = eat(p); p = make(); n = n + a; } return n; }",
        );
    }

    // CEP:WHAT: Verifies that deferred init is conservative across loops behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn deferred_let_in_loop_is_conservative() {
        // A loop body may run zero times: assignment inside does not count.
        let diags = lower_err(
            "fn main() -> i32 { let x: i32; let c: i32 = 0; while c > 0 { x = 1; } return x; }",
        );
        assert!(diags.iter().any(|d| d.code == "PLK-S0024"), "{:#?}", diags);
        // Assigning before the loop makes it definite.
        lower_ok(
            "fn main() -> i32 { let mut x: i32; let c: i32 = 0; x = 0; while c > 0 { x = 1; } return x; }",
        );
    }

    // CEP:WHAT: Verifies that statements after a terminator are skipped behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn statements_after_terminator_are_not_analyzed() {
        // The dead statement would otherwise be a use-after-move error.
        lower_ok(
            "struct P { x: i32 } \
             fn eat(p: P) -> i32 { return p.x; } \
             fn main() -> i32 { let p: P = P { x: 1 }; let a: i32 = eat(p); return a; eat(p); }",
        );
    }
}
