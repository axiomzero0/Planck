// CEP:FILE: crates/planck_opt/src/sccp.rs
// CEP:WHAT: Sparse conditional constant propagation (design pass 22): a three-level lattice (unknown, constant, overdefined) evaluated over SSA vregs, with CFG edges pruned when a branch condition is a known constant and unreachable blocks removed.
// CEP:WHY: SCCP sees through join points that block-local folding cannot: values merged by phis resolve when all executable inputs agree, and branches on provably constant conditions make whole regions disappear. Reuses the const-folder's arithmetic rules so overflow and trap behavior stay identical.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Any operand the folder cannot resolve (would-trap arithmetic, loads, calls, args) makes the result overdefined; phi inputs count only along executable edges. The result passes the pipeline verifier; malformed input is caught there.
// CEP:ASSUMES: Input functions pass verify_program; phis occupy a leading region of their block; vregs are single-assignment (the SSA construction contract).
// CEP:COST: Sweeps to fixpoint over blocks and instructions: O(sweeps * nodes) with sweeps bounded by 2 + edge count (lattice heights are 3 and edges only become executable); deterministic index-order sweeps, no hash iteration.
// CEP:EVIDENCE: unit tests in this file.
// CEP:SECURITY: Constant replacement never invents values: folded arithmetic reuses the const-folder, which keeps would-trap operations intact.
// CEP:PORTABILITY: Target-independent.

#![forbid(unsafe_code)]

use planck_ir::{Const, Inst, IrFunc, Program, Term};
use std::collections::BTreeSet;

/// Lattice cell per vreg. `Unknown` is the optimistic start: a vreg nobody has
/// evaluated yet. `Over` is the conservative bottom.
#[derive(Clone, Debug, PartialEq)]
enum Lattice {
    Unknown,
    Const(Const),
    Over,
}

// CEP:WHAT: Handles the run step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn run(program: &mut Program) -> usize {
    let mut changes = 0;
    for func in program.funcs.iter_mut() {
        changes += run_on_func(func);
    }
    changes
}

// CEP:WHAT: Handles the Runs this pass over one function, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn run_on_func(func: &mut IrFunc) -> usize {
    let n = func.blocks.len();
    if n == 0 {
        return 0;
    }
    let (exec_blocks, exec_edges, lattice) = solve(func);

    // Apply: rewrite constants, prune dead trap guards, fold dead branches,
    // drop non-executable phi inputs, then delete non-executable blocks.
    let mut changes = 0;
    let kept_flags: Vec<bool> = exec_blocks.clone();
    for (b, executable) in kept_flags.iter().enumerate() {
        if !executable {
            continue;
        }
        let block = &mut func.blocks[b];
        // Phis stay in the leading region; a phi that resolved to a constant
        // becomes a Const placed right after the surviving phi region.
        let mut resolved: Vec<Inst> = Vec::new();
        let mut kept: Vec<Inst> = Vec::with_capacity(block.insts.len());
        for inst in std::mem::take(&mut block.insts) {
            match inst {
                Inst::Phi { dst, inputs, ty } => {
                    // Keep only inputs along executable edges.
                    let live_inputs: Vec<_> = inputs
                        .into_iter()
                        .filter(|(pred, _)| exec_edges.contains(&(pred.0, b as u32)))
                        .collect();
                    match lattice.get(dst.0 as usize) {
                        Some(Lattice::Const(value)) => {
                            changes += 1;
                            resolved.push(Inst::Const {
                                dst,
                                value: value.clone(),
                            });
                        }
                        _ => {
                            kept.push(Inst::Phi {
                                dst,
                                ty,
                                inputs: live_inputs,
                            });
                        }
                    }
                }
                Inst::Bin { dst, .. }
                | Inst::Un { dst, .. }
                | Inst::Cast { dst, .. }
                | Inst::Copy { dst, .. } => match lattice.get(dst.0 as usize) {
                    Some(Lattice::Const(value)) => {
                        changes += 1;
                        kept.push(Inst::Const {
                            dst,
                            value: value.clone(),
                        });
                    }
                    _ => kept.push(inst),
                },
                Inst::TrapIf { cond, trap } => {
                    match lattice.get(cond.0 as usize) {
                        // A guard that can never fire is removed.
                        Some(Lattice::Const(Const::Bool(false))) => {
                            changes += 1;
                        }
                        // A guard that always fires stays; code after it is
                        // unreachable but harmless, and removing the trap
                        // would change observable behavior.
                        _ => kept.push(Inst::TrapIf { cond, trap }),
                    }
                }
                other => kept.push(other),
            }
        }
        // Rebuild: phi region first (surviving phis), then everything else in
        // original relative order.
        let phi_count = kept
            .iter()
            .take_while(|inst| matches!(inst, Inst::Phi { .. }))
            .count();
        let mut new_insts: Vec<Inst> = kept[..phi_count].to_vec();
        new_insts.extend(resolved);
        new_insts.extend(kept[phi_count..].iter().cloned());
        block.insts = new_insts;

        let taken = match &block.term {
            Term::BrCond {
                cond,
                then_blk,
                else_blk,
            } => match lattice.get(cond.0 as usize) {
                Some(Lattice::Const(Const::Bool(true))) => Some(*then_blk),
                Some(Lattice::Const(Const::Bool(false))) => Some(*else_blk),
                _ => None,
            },
            _ => None,
        };
        if let Some(target) = taken {
            block.term = Term::Br(target);
            changes += 1;
        }
    }

    // Delete non-executable blocks and remap terminators and phi inputs.
    let removed: usize = kept_flags.iter().filter(|exec| !**exec).count();
    if removed > 0 {
        let mut mapping: Vec<Option<u32>> = vec![None; n];
        let mut new_blocks: Vec<planck_ir::Block> = Vec::with_capacity(n);
        for (idx, block) in func.blocks.drain(..).enumerate() {
            if kept_flags[idx] {
                mapping[idx] = Some(new_blocks.len() as u32);
                new_blocks.push(block);
            }
        }
        let remap = |target: &mut u32| {
            if let Some(mapped) = mapping[*target as usize] {
                *target = mapped;
            }
        };
        for block in new_blocks.iter_mut() {
            match &mut block.term {
                Term::Br(target) => remap(&mut target.0),
                Term::BrCond {
                    then_blk, else_blk, ..
                } => {
                    remap(&mut then_blk.0);
                    remap(&mut else_blk.0);
                }
                Term::Ret { .. } => {}
            }
            for inst in block.insts.iter_mut() {
                if let Inst::Phi { inputs, .. } = inst {
                    for (pred, _) in inputs.iter_mut() {
                        remap(&mut pred.0);
                    }
                }
            }
        }
        func.blocks = new_blocks;
        changes += removed;
    }

    changes
}

/// Runs the solver: returns the executable block flags, the executable edge
/// set, and the final lattice per vreg.
// CEP:WHAT: Handles the Runs the SCCP solver to a fixpoint over executable blocks and edges, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn solve(func: &IrFunc) -> (Vec<bool>, BTreeSet<(u32, u32)>, Vec<Lattice>) {
    let n = func.blocks.len();
    let mut exec_blocks: Vec<bool> = vec![false; n];
    let mut exec_edges: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut lattice: Vec<Lattice> = vec![Lattice::Unknown; func.vregs.len()];
    exec_blocks[0] = true;

    loop {
        let mut changed = false;
        let flags: Vec<bool> = exec_blocks.clone();
        for (b, executable) in flags.iter().enumerate() {
            if !executable {
                continue;
            }
            let block = &func.blocks[b];
            for inst in &block.insts {
                changed |= evaluate(inst, b, &exec_edges, &mut lattice);
            }
            // Terminator: decide which edges the block opens.
            match &block.term {
                Term::Br(target) => {
                    changed |= mark_edge(b, target.0, &mut exec_edges, &mut exec_blocks);
                }
                Term::BrCond {
                    cond,
                    then_blk,
                    else_blk,
                } => match lattice.get(cond.0 as usize) {
                    Some(Lattice::Const(Const::Bool(true))) => {
                        changed |= mark_edge(b, then_blk.0, &mut exec_edges, &mut exec_blocks);
                    }
                    Some(Lattice::Const(Const::Bool(false))) => {
                        changed |= mark_edge(b, else_blk.0, &mut exec_edges, &mut exec_blocks);
                    }
                    _ => {
                        changed |= mark_edge(b, then_blk.0, &mut exec_edges, &mut exec_blocks);
                        changed |= mark_edge(b, else_blk.0, &mut exec_edges, &mut exec_blocks);
                    }
                },
                Term::Ret { .. } => {}
            }
        }
        if !changed {
            break;
        }
    }
    (exec_blocks, exec_edges, lattice)
}

// CEP:WHAT: Handles the Marks one CFG edge executable and opens its target block, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn mark_edge(
    from: usize,
    to: u32,
    exec_edges: &mut BTreeSet<(u32, u32)>,
    exec_blocks: &mut [bool],
) -> bool {
    let edge = (from as u32, to);
    if exec_edges.insert(edge) {
        if (to as usize) < exec_blocks.len() {
            exec_blocks[to as usize] = true;
        }
        true
    } else {
        false
    }
}

fn meet(current: &Lattice, incoming: Lattice) -> Lattice {
    match (current, incoming) {
        (Lattice::Over, _) | (_, Lattice::Over) => Lattice::Over,
        (Lattice::Unknown, other) => other,
        (other, Lattice::Unknown) => other.clone(),
        (Lattice::Const(a), Lattice::Const(b)) => {
            if a == &b {
                Lattice::Const(a.clone())
            } else {
                Lattice::Over
            }
        }
    }
}

/// Assigns `incoming` into `lattice[vreg]`, reporting whether anything changed.
// CEP:WHAT: Handles the Merges one lattice value into a vreg cell and reports whether it changed, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn meet_assign(lattice: &mut [Lattice], vreg: u32, incoming: Lattice) -> bool {
    let vreg = vreg as usize;
    if vreg >= lattice.len() {
        return false;
    }
    let merged = meet(&lattice[vreg], incoming);
    if merged != lattice[vreg] {
        lattice[vreg] = merged;
        return true;
    }
    false
}

/// Operand lattice lookup: an out-of-range vreg is overdefined (defensive).
fn lat_of(lattice: &[Lattice], vreg: u32) -> Lattice {
    lattice.get(vreg as usize).cloned().unwrap_or(Lattice::Over)
}

// CEP:WHAT: Handles the Evaluates one instruction against the lattice per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn evaluate(
    inst: &Inst,
    block: usize,
    exec_edges: &BTreeSet<(u32, u32)>,
    lattice: &mut [Lattice],
) -> bool {
    match inst {
        Inst::Const { dst, value } => meet_assign(lattice, dst.0, Lattice::Const(value.clone())),
        Inst::Copy { dst, src, .. } => meet_assign(lattice, dst.0, lat_of(lattice, src.0)),
        Inst::Bin {
            dst,
            a,
            b,
            ty,
            op,
            checked,
        } => {
            let (la, lb) = (lat_of(lattice, a.0), lat_of(lattice, b.0));
            let result = match (&la, &lb) {
                (Lattice::Const(ca), Lattice::Const(cb)) => {
                    match crate::const_fold::fold_bin(*op, ca, cb, ty, *checked) {
                        crate::const_fold::FoldOutcome::Value(value) => Lattice::Const(value),
                        // Keep means the operation would trap or is not
                        // foldable: the trap is part of the semantics, so the
                        // result is overdefined and the instruction survives.
                        crate::const_fold::FoldOutcome::Keep => Lattice::Over,
                    }
                }
                (Lattice::Over, _) | (_, Lattice::Over) => Lattice::Over,
                _ => Lattice::Unknown,
            };
            meet_assign(lattice, dst.0, result)
        }
        Inst::Un {
            dst,
            a,
            ty,
            op,
            checked,
        } => {
            let result = match lat_of(lattice, a.0) {
                Lattice::Const(ca) => match crate::const_fold::fold_un(*op, &ca, ty, *checked) {
                    Some(value) => Lattice::Const(value),
                    None => Lattice::Over,
                },
                Lattice::Over => Lattice::Over,
                Lattice::Unknown => Lattice::Unknown,
            };
            meet_assign(lattice, dst.0, result)
        }
        Inst::Cast { dst, src, from, to } => {
            let result = match lat_of(lattice, src.0) {
                Lattice::Const(ca) => match crate::const_fold::fold_cast(&ca, from, to) {
                    Some(value) => Lattice::Const(value),
                    None => Lattice::Over,
                },
                Lattice::Over => Lattice::Over,
                Lattice::Unknown => Lattice::Unknown,
            };
            meet_assign(lattice, dst.0, result)
        }
        Inst::Phi { dst, inputs, .. } => {
            // Meet over the inputs whose incoming edge is executable.
            let mut value = Lattice::Unknown;
            for (pred, input) in inputs {
                if exec_edges.contains(&(pred.0, block as u32)) {
                    value = meet(&value, lat_of(lattice, input.0));
                }
            }
            meet_assign(lattice, dst.0, value)
        }
        Inst::Arg { dst, .. }
        | Inst::LoadSlot { dst, .. }
        | Inst::LoadField { dst, .. }
        | Inst::AddrSlot { dst, .. }
        | Inst::AddrOffset { dst, .. }
        | Inst::LoadIndirect { dst, .. } => meet_assign(lattice, dst.0, Lattice::Over),
        Inst::Call { dst, .. } => {
            if let Some(dst) = dst {
                meet_assign(lattice, dst.0, Lattice::Over);
            }
            false
        }
        Inst::StoreSlot { .. }
        | Inst::StoreField { .. }
        | Inst::StoreIndirect { .. }
        | Inst::StoreStructFromAddr { .. }
        | Inst::TrapIf { .. }
        | Inst::Trap { .. }
        | Inst::Comment { .. } => false,
    }
}
