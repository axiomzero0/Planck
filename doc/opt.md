# Optimizer

## Abstract

The optimizer runs the versioned pass pipeline over verified IR, with a verifier gate after every pass. Its inputs are programs that pass `verify_program`; its outputs are programs that still pass it, plus per-pass change counts. A pass that produces invalid IR surfaces as findings and halts compilation with an internal-compiler-error report, never as miscompiled output.

## Formal Definitions

```rust
pub const PIPELINE_NAME: &str = "planck-opt-0.2.0";
pub const PIPELINE: &[(&str, PassFn)] = &[
    ("ssa_construct", ssa::run),
    ("sccp", sccp::run),
    ("lcssa", out_ssa::lcssa),
    ("phi_cleanup", out_ssa::phi_cleanup),
    ("ssa_destroy", out_ssa::ssa_destroy),
    ("copy_prop", out_ssa::copy_prop),
    ("const_fold", const_fold::run),
    ("gvn_cse", gvn::run),
    ("licm", licm::run),
    ("peephole", peephole::run),
    ("dce", dce::run),
];
pub struct OptResult { pub total_changes: usize, pub per_pass: Vec<(&'static str, usize)>, pub verify_errors: Vec<VerifyError> }
```

## Algorithm

The SSA stage (PLK-003, design passes 21/22/35/38/40):

1. `ssa_construct`: deletes unreachable blocks, then promotes scalar frame slots that are provably private (never address-taken, never field-accessed, never an aggregate memcpy destination, never the hidden sret slot) into single-assignment vregs. Phi placement uses the pruned iterated dominance frontier of each slot's definition blocks; renaming walks the dominator tree iteratively and commits each slot all-or-nothing (a use without a reaching definition or an unresolvable phi input reverts that slot to memory). Promoted parameters are defined by `Arg` instructions at the entry-block top, mirroring the System V ABI classification shared with the prologue spills. Stores become stack pushes and are deleted; loads become stack reads, deleted, with their destinations rewritten through an acyclic replacement map that is chased to its end.
2. `sccp`: sparse conditional constant propagation over a three-level lattice (unknown, constant, overdefined). Branch edges whose condition folds to a constant are not marked executable, so whole regions go dead; phis meet only along executable edges; would-trap arithmetic (checked overflow, division by zero) is overdefined so its trap stays observable. Post-processing rewrites constants, prunes never-firing `trap_if` guards, folds constant branches, and deletes non-executable blocks.
3. `lcssa`: for each natural loop (innermost first), values defined inside and used outside get exit phis. Placement starts at the exit blocks that dominate an outflow use and follows the iterated dominance frontier through placed blocks; a phi is inserted only when every predecessor edge carries a dominance-valid value, and a value is skipped otherwise.
4. `phi_cleanup`: removes phis whose non-self inputs all agree, except at loop exits where the replacement is defined inside the loop (which would undo LCSSA).
5. `ssa_destroy`: splits critical edges, lowers each phi into copies at the end of its predecessors (conflicting sources on one edge are versioned through fresh temporaries first, which resolves swaps and longer cycles), then deletes the phi instructions. Former phi destinations become multi-def vregs, which the verifier validates by reaching definitions.
6. `copy_prop`: folds uses of single-def copies to their sources (chains chased, multi-def destinations left intact).

The classic stage then runs over the phi-free result:

7. `const_fold`: folds arithmetic, bitwise, comparison, and cast operations over block-local constant environments (copies of known constants fold too); folds `BrCond` on constants; removes unreachable blocks. Checked operations whose results would overflow are not folded, so their traps stay observable; division by a zero constant is never folded.
8. `gvn_cse`: dominator-scoped value numbering over trap-free pure instructions with a single definition (multi-def vregs are excluded from CSE); duplicates are replaced by their dominating definitions in one rewrite sweep.
9. `licm`: discovers natural loops via back edges, computes the reverse-reachable body, and hoists trap-free pure instructions whose operands have no definitions inside the loop to the unique preheader.
10. `peephole`: algebraic identities (x+0, x-0, x-x, x*0, x*1, x/1, x%1, shift-by-zero, boolean identities) and unchecked multiply-by-power-of-two to shift, with replacement chains resolved to fixpoint.
11. `dce`: removes pure definitions whose results are unused, to fixpoint. Loads and copies are removable because the borrow checker guarantees reference validity; stores, calls, and traps are never removed.

Each pass is O(N) to O(slots * N); the pipeline runs them in fixed order with verification O(N + E) between passes.

## Invariants

1. Pipeline order is fixed and versioned; changing it is a semantic event requiring golden updates (standard 38.20).
2. Verification runs after every pass and before returning; `verify_errors` non-empty means the pipeline aborted.
3. Hoisting never moves a potentially trapping operation.
4. Checked arithmetic traps are preserved by folding: an overflow that would trap at runtime is not folded away.
5. SCCP never invents values: folding reuses the const-folder's arithmetic, and the only blocks it removes are ones no executable path reaches.
6. Copies that define a multi-def (former phi) destination are never propagated or CSE'd away.
7. Codegen receives phi-free IR; a phi reaching the emitter is an internal-invariant violation and aborts loudly.

## Integration Points

- Depends on: `planck_ir` (IR, verifier, dominators), `planck_support`.
- Depended on by: `planckc` (drives the pipeline between sema and codegen).

Signature: `pub fn optimize(program: &mut Program) -> OptResult`.

## Known Limitations

- No interprocedural passes; no inlining (PLK-014).
- Constants do not propagate into slots that stayed in memory (aggregates, address-taken locals); SROA is PLK-011.
- No loop unrolling, unswitching, or versioning (PLK-013).
- Full Sea-of-Nodes floating (pure nodes detached from blocks) is not implemented; the SSA stage operates on the structured CFG (PLK-002/PLK-003 remaining scope).
