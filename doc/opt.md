# Optimizer

## Abstract

The optimizer runs the versioned pass pipeline over verified IR, with a verifier gate after every pass. Its inputs are programs that pass `verify_program`; its outputs are programs that still pass it, plus per-pass change counts. A pass that produces invalid IR surfaces as findings and halts compilation with an internal-compiler-error report, never as miscompiled output.

## Formal Definitions

```rust
pub const PIPELINE_NAME: &str = "planck-opt-0.1.0";
pub const PIPELINE: &[(&str, PassFn)] = &[
    ("const_fold", const_fold::run),
    ("gvn_cse", gvn::run),
    ("licm", licm::run),
    ("peephole", peephole::run),
    ("dce", dce::run),
];
pub struct OptResult { pub total_changes: usize, pub per_pass: Vec<(&'static str, usize)>, pub verify_errors: Vec<VerifyError> }
```

## Algorithm

1. `const_fold`: folds arithmetic, bitwise, comparison, and cast operations over block-local constant environments; folds `BrCond` on constants; removes unreachable blocks. Checked operations whose results would overflow are not folded, so their traps stay observable; division by a zero constant is never folded.
2. `gvn_cse`: dominator-scoped value numbering over trap-free pure instructions; duplicates are replaced by their dominating definitions in one rewrite sweep.
3. `licm`: discovers natural loops via back edges, computes the reverse-reachable body, and hoists trap-free pure instructions whose operands are loop-invariant to the unique preheader.
4. `peephole`: algebraic identities (x+0, x-0, x-x, x*0, x*1, x/1, x%1, shift-by-zero, boolean identities) and unchecked multiply-by-power-of-two to shift, with replacement chains resolved to fixpoint.
5. `dce`: removes pure definitions whose results are unused, to fixpoint. Loads are removable because the borrow checker guarantees reference validity; stores, calls, and traps are never removed.

Each pass is O(N) to O(N log N); the pipeline runs them in fixed order with verification O(N + E) between passes.

## Invariants

1. Pipeline order is fixed and versioned; changing it is a semantic event requiring golden updates (standard 38.20).
2. Verification runs after every pass and before returning; `verify_errors` non-empty means the pipeline aborted.
3. Hoisting never moves a potentially trapping operation.
4. Checked arithmetic traps are preserved by folding: an overflow that would trap at runtime is not folded away.

## Integration Points

- Depends on: `planck_ir` (IR, verifier, dominators), `planck_support`.
- Depended on by: `planckc` (drives the pipeline between sema and codegen).

Signature: `pub fn optimize(program: &mut Program) -> OptResult`.

## Known Limitations

- No interprocedural passes; no inlining (PLK-014).
- Constant environments are block-local: constants from dominating blocks do not propagate across block boundaries.
- No loop unrolling, unswitching, or versioning (PLK-013).
