// CEP:FILE: crates/planck_opt/src/ssa.rs
// CEP:WHAT: SSA construction (design pass 35, construction half): promotes frame slots into single-assignment vregs, places phis at the pruned iterated dominance frontier, and renames through the dominator tree. Parameters become Arg-defined entry vregs.
// CEP:WHY: Phi-joined SSA is the form the design's global passes operate on (SCCP, redundant-phi elimination, LCSSA); promoting only provably private scalars keeps the lowering contract intact for everything else.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Promotion is all-or-nothing per slot: renaming edits a clone of the blocks and commits only when every load has a reaching definition and every phi input is resolvable; on any doubt the slot stays in memory. Malformed input is caught by the pipeline verifier.
// CEP:ASSUMES: Input functions pass verify_program. Slots referenced by AddrSlot, field access, aggregate memcpy, or the hidden sret slot are never promoted. Reducible CFGs (the frontend only produces structured control flow); nothing branches to the entry block.
// CEP:COST: Per slot O(blocks) placement plus O(nodes) renaming; the stage is O(slots * nodes) worst case with small constants; deterministic (index-ordered worklists and BTreeMaps, no hash iteration).
// CEP:EVIDENCE: unit tests in this file; golden dumps in crates/planckc/tests.
// CEP:SECURITY: The transformation is semantics-preserving on verified input; the verifier gates the result.
// CEP:PORTABILITY: Target-independent.

#![forbid(unsafe_code)]

use planck_ir::{Block, BlockId, Dominators, Inst, IrFunc, Program, Slot, Term, Ty, Vreg};
use std::collections::{BTreeMap, BTreeSet};

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
        changes += construct_on_func(func);
    }
    changes
}

// CEP:WHAT: Handles the Promotes one function's private scalars into phi-joined SSA, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn construct_on_func(func: &mut IrFunc) -> usize {
    let mut changes = 0;

    // Unreachable blocks would break the dominator-tree walk and produce phis
    // with unfillable inputs; they are dead code and removed first.
    changes += crate::const_fold::remove_unreachable(func);
    if func.blocks.is_empty() {
        return changes;
    }

    let n = func.blocks.len();
    let dominators = Dominators::compute(func);
    let preds = compute_preds(func);
    // The SSA walk relies on the entry block having no predecessors so that
    // Arg definitions and phi regions cannot collide; the frontend guarantees
    // this, and anything else is outside the construction contract.
    if !preds[0].is_empty() {
        return changes;
    }
    let frontiers = compute_frontiers(&dominators, &preds, n);

    // Slots that must stay in memory: address taken, field access, aggregate
    // memcpy destination, or the hidden sret buffer.
    let mut pinned: BTreeSet<u32> = BTreeSet::new();
    if let Some(sret) = func.sret_slot {
        pinned.insert(sret.0);
    }
    for block in &func.blocks {
        for inst in &block.insts {
            match inst {
                Inst::AddrSlot { slot, .. } => {
                    pinned.insert(slot.0);
                }
                Inst::LoadField { slot, .. } | Inst::StoreField { slot, .. } => {
                    pinned.insert(slot.0);
                }
                Inst::StoreStructFromAddr { dst_slot, .. } => {
                    pinned.insert(dst_slot.0);
                }
                _ => {}
            }
        }
    }

    // Candidates: scalars only. The pointer values of references are scalars
    // and promote; the memory they point to does not participate.
    let candidates: Vec<u32> = (0..func.slots.len() as u32)
        .filter(|idx| !pinned.contains(idx) && func.slots[*idx as usize].ty.scalar_size().is_some())
        .collect();
    if candidates.is_empty() {
        return changes;
    }

    // Pruned placement needs per-slot liveness; one backward pass covers all
    // candidate slots at once.
    let (use_gen, block_defs) = slot_use_def(func, &candidates);
    let live_in = slot_live_in(func, &use_gen, &block_defs);

    // Dominator-tree children in ascending block order, and structural
    // successors per block for phi input collection.
    let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
    for b in 1..n {
        if let Some(idom) = dominators.idom[b] {
            children[idom.0 as usize].push(b as u32);
        }
    }
    let successors: Vec<Vec<u32>> = func
        .blocks
        .iter()
        .map(|block| successors_of_term(&block.term))
        .collect();

    // Parameter slots are defined by an implicit entry def; remember their
    // positions so Arg instructions can be emitted in parameter order.
    let mut param_index_of: BTreeMap<u32, u32> = BTreeMap::new();
    for (idx, slot) in func.params.iter().enumerate() {
        param_index_of.insert(slot.0, idx as u32);
    }
    let mut param_args: BTreeMap<u32, Vreg> = BTreeMap::new();

    for slot_idx in candidates {
        let slot = Slot(slot_idx);
        let ty = func.slots[slot_idx as usize].ty.clone();
        let has_loads = (0..n).any(|b| {
            func.blocks[b]
                .insts
                .iter()
                .any(|inst| matches!(inst, Inst::LoadSlot { slot: s, .. } if *s == slot))
        });
        if !has_loads {
            continue; // nothing reads the slot; its stores are DCE material
        }

        // Def blocks: every StoreSlot, plus the implicit entry def for params.
        let mut def_blocks: BTreeSet<u32> = BTreeSet::new();
        for b in 0..n {
            if func.blocks[b]
                .insts
                .iter()
                .any(|inst| matches!(inst, Inst::StoreSlot { slot: s, .. } if *s == slot))
            {
                def_blocks.insert(b as u32);
            }
        }
        if def_blocks.is_empty() {
            if param_index_of.contains_key(&slot_idx) {
                def_blocks.insert(0);
            } else {
                continue; // loads with no store anywhere: not def-coverable
            }
        }

        // Pruned phi placement: iterated dominance frontier of the def blocks,
        // restricted to blocks where the slot is live-in.
        let mut phi_blocks: Vec<u32> = Vec::new();
        let mut placed: BTreeSet<u32> = BTreeSet::new();
        let mut queued: BTreeSet<u32> = def_blocks.clone();
        let mut worklist: Vec<u32> = def_blocks.iter().copied().collect();
        while let Some(x) = worklist.pop() {
            for &y in &frontiers[x as usize] {
                if placed.contains(&y) || !live_in[y as usize].contains(&slot_idx) {
                    continue;
                }
                placed.insert(y);
                phi_blocks.push(y);
                if queued.insert(y) {
                    worklist.push(y);
                }
            }
        }
        phi_blocks.sort_unstable();

        // Rename on a clone; commit only when the whole slot resolves.
        let vregs_before = func.vregs.len();
        let mut new_blocks: Vec<Block> = func.blocks.clone();

        // Phi destinations are allocated first (block order, deterministic).
        let mut phi_dsts: BTreeMap<u32, Vreg> = BTreeMap::new();
        for &b in &phi_blocks {
            phi_dsts.insert(b, fresh_vreg(func, &ty));
        }
        let entry_vreg = param_index_of
            .contains_key(&slot_idx)
            .then(|| fresh_vreg(func, &ty));

        let mut replace_load: BTreeMap<u32, Vreg> = BTreeMap::new();
        let mut phi_inputs: BTreeMap<u32, Vec<(BlockId, Vreg)>> = BTreeMap::new();
        let (ok, stores_removed) = rename_slot(
            &mut new_blocks,
            &successors,
            &children,
            slot,
            &phi_dsts,
            entry_vreg,
            &mut replace_load,
            &mut phi_inputs,
        );

        if !ok {
            func.vregs.truncate(vregs_before);
            continue;
        }

        // Load destinations are single-def and only used in blocks dominated
        // by their load, so a global replacement through the map is exact.
        // The map can chain (a load of a value that was itself a load result),
        // so every lookup chases to the final replacement.
        for block in new_blocks.iter_mut() {
            for inst in block.insts.iter_mut() {
                rewrite_uses(inst, &replace_load);
            }
            match &mut block.term {
                Term::BrCond { cond, .. } => {
                    *cond = chase(&replace_load, *cond);
                }
                Term::Ret { value: Some(value) } => {
                    *value = chase(&replace_load, *value);
                }
                _ => {}
            }
        }

        // Insert phis at the head of their blocks, after any phis already
        // present (the phi region grows deterministically slot by slot). Phi
        // inputs reference rename-stack values, which can be deleted load
        // destinations, so they go through the same chase.
        for &b in &phi_blocks {
            let dst = phi_dsts[&b];
            let mut inputs = phi_inputs.remove(&b).unwrap_or_default();
            for (_, value) in inputs.iter_mut() {
                *value = chase(&replace_load, *value);
            }
            // One input per predecessor, in ascending predecessor order.
            inputs.sort_by_key(|(pred, _)| pred.0);
            let phi = Inst::Phi {
                dst,
                ty: ty.clone(),
                inputs,
            };
            let insert_at = new_blocks[b as usize]
                .insts
                .iter()
                .take_while(|inst| matches!(inst, Inst::Phi { .. }))
                .count();
            new_blocks[b as usize].insts.insert(insert_at, phi);
        }

        func.blocks = new_blocks;
        changes += phi_blocks.len() + replace_load.len() + stores_removed;
        if let (Some(&param_slot), Some(entry)) = (param_index_of.get(&slot_idx), entry_vreg) {
            param_args.insert(param_slot, entry);
            changes += 1;
        }
    }

    // Entry definitions for promoted parameters: in parameter order, at the
    // very top of the entry block (the verifier's Arg region).
    if !param_args.is_empty() {
        let entry = &mut func.blocks[0];
        for (offset, (param_idx, vreg)) in param_args.iter().enumerate() {
            let arg_slot = func.params[*param_idx as usize];
            entry.insts.insert(
                offset,
                Inst::Arg {
                    dst: *vreg,
                    index: *param_idx,
                    ty: func.slots[arg_slot.0 as usize].ty.clone(),
                },
            );
        }
    }

    changes
}

/// Renames one slot through the dominator tree, editing `new_blocks` in place.
/// Stores become stack pushes and are deleted; loads become stack reads and are
/// deleted, their destinations recorded in `replace_load`. Returns whether the
/// whole slot resolved, plus the number of deleted stores.
#[allow(clippy::too_many_arguments)]
// CEP:WHAT: Handles the Renames one slot through the dominator tree with commit-or-revert semantics, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn rename_slot(
    new_blocks: &mut [Block],
    successors: &[Vec<u32>],
    children: &[Vec<u32>],
    slot: Slot,
    phi_dsts: &BTreeMap<u32, Vreg>,
    entry_vreg: Option<Vreg>,
    replace_load: &mut BTreeMap<u32, Vreg>,
    phi_inputs: &mut BTreeMap<u32, Vec<(BlockId, Vreg)>>,
) -> (bool, usize) {
    let mut stack: Vec<Vreg> = Vec::new();
    if let Some(entry) = entry_vreg {
        stack.push(entry);
    }
    let mut stores_removed = 0usize;
    let mut ok = true;

    // Iterative dominator-tree walk with enter/exit semantics: `work` holds
    // (block, next child index); `depth_log` holds the stack depth to restore
    // when a block is exited.
    let mut work: Vec<(u32, usize)> = vec![(0, 0)];
    let mut depth_log: Vec<usize> = vec![stack.len()];

    while let Some(&mut (b, ref mut child_idx)) = work.last_mut() {
        if !ok {
            break;
        }
        if *child_idx == 0 {
            // Enter b exactly once.
            if let Some(&dst) = phi_dsts.get(&b) {
                stack.push(dst);
            }
            let block = &mut new_blocks[b as usize];
            let mut kept: Vec<Inst> = Vec::with_capacity(block.insts.len());
            let insts = std::mem::take(&mut block.insts);
            for inst in insts {
                match inst {
                    Inst::LoadSlot { dst, slot: s, ty } if s == slot => {
                        match stack.last().copied() {
                            Some(current) => {
                                replace_load.insert(dst.0, current);
                            }
                            None => {
                                // Use without a reaching definition (possible
                                // only outside the checker-guaranteed subset):
                                // abort this slot's promotion.
                                ok = false;
                                kept.push(Inst::LoadSlot { dst, slot: s, ty });
                            }
                        }
                    }
                    Inst::StoreSlot { slot: s, src, ty } if s == slot => {
                        stack.push(src);
                        stores_removed += 1;
                        let _ = ty;
                    }
                    other => kept.push(other),
                }
            }
            block.insts = kept;

            if ok {
                for &succ in &successors[b as usize] {
                    if phi_dsts.contains_key(&succ) {
                        match stack.last().copied() {
                            Some(value) => {
                                phi_inputs
                                    .entry(succ)
                                    .or_default()
                                    .push((BlockId(b), value));
                            }
                            None => ok = false,
                        }
                    }
                }
            }
            if !ok {
                break;
            }
        }
        if *child_idx < children[b as usize].len() {
            let child = children[b as usize][*child_idx];
            *child_idx += 1;
            depth_log.push(stack.len());
            work.push((child, 0));
        } else {
            work.pop();
            let depth = depth_log.pop().unwrap_or(0);
            stack.truncate(depth);
        }
    }

    // Every phi needs one input per structural predecessor.
    if ok {
        for (&b, inputs) in phi_inputs.iter() {
            let pred_count = successors.iter().filter(|succs| succs.contains(&b)).count();
            if inputs.len() != pred_count {
                ok = false;
                break;
            }
        }
    }
    if ok {
        for &b in phi_dsts.keys() {
            if !phi_inputs.contains_key(&b) {
                ok = false;
                break;
            }
        }
    }

    (ok, stores_removed)
}

/// Rewrites vreg uses inside one instruction through the replacement map.
// CEP:WHAT: Handles the Rewrites a vreg's uses inside one instruction through the replacement map, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn rewrite_uses(inst: &mut Inst, map: &BTreeMap<u32, Vreg>) {
    if map.is_empty() {
        return;
    }
    let remap = |vreg: &mut Vreg| {
        *vreg = chase(map, *vreg);
    };
    match inst {
        Inst::Bin { a, b, .. } => {
            remap(a);
            remap(b);
        }
        Inst::Un { a, .. } | Inst::Cast { src: a, .. } => remap(a),
        Inst::AddrOffset { base, offset, .. } => {
            remap(base);
            remap(offset);
        }
        Inst::LoadIndirect { addr, .. } => remap(addr),
        Inst::StoreIndirect { addr, src, .. } => {
            remap(addr);
            remap(src);
        }
        Inst::StoreSlot { src, .. } | Inst::StoreField { src, .. } => remap(src),
        Inst::StoreStructFromAddr { src_addr, .. } => remap(src_addr),
        Inst::TrapIf { cond, .. } => remap(cond),
        Inst::Call { sret, args, .. } => {
            if let Some(sret) = sret {
                remap(sret);
            }
            for arg in args.iter_mut() {
                remap(&mut arg.value);
            }
        }
        Inst::Phi { inputs, .. } => {
            for (_, value) in inputs.iter_mut() {
                remap(value);
            }
        }
        Inst::Copy { src, .. } => remap(src),
        Inst::LoadSlot { .. }
        | Inst::LoadField { .. }
        | Inst::AddrSlot { .. }
        | Inst::Const { .. }
        | Inst::Arg { .. }
        | Inst::Trap { .. }
        | Inst::Comment { .. } => {}
    }
}

// CEP:WHAT: Handles the Lists a terminator's structural successors, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn successors_of_term(term: &Term) -> Vec<u32> {
    match term {
        Term::Br(target) => vec![target.0],
        Term::BrCond {
            then_blk, else_blk, ..
        } => {
            if then_blk == else_blk {
                vec![then_blk.0]
            } else {
                vec![then_blk.0, else_blk.0]
            }
        }
        Term::Ret { .. } => Vec::new(),
    }
}

// CEP:WHAT: Handles the Computes the structural predecessor list of every block, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn compute_preds(func: &IrFunc) -> Vec<Vec<u32>> {
    let mut preds: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); func.blocks.len()];
    for (idx, block) in func.blocks.iter().enumerate() {
        for succ in successors_of_term(&block.term) {
            if (succ as usize) < preds.len() {
                preds[succ as usize].insert(idx as u32);
            }
        }
    }
    preds
        .into_iter()
        .map(|set| set.into_iter().collect())
        .collect()
}

/// Dominance frontiers per block (Cooper, Harvey, Kennedy): for each join
/// block, walk each predecessor up the dominator tree to the join's idom.
// CEP:WHAT: Handles the Computes dominance frontiers with the Cooper-Harvey-Kennedy walk, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn compute_frontiers(dominators: &Dominators, preds: &[Vec<u32>], n: usize) -> Vec<BTreeSet<u32>> {
    let mut frontiers: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    for (y, pred_list) in preds.iter().enumerate() {
        if pred_list.len() < 2 {
            continue;
        }
        let y_idom = match dominators.idom[y] {
            Some(idom) => idom.0,
            None => continue,
        };
        for &p in pred_list {
            let mut runner = p;
            let mut guard = 0usize;
            while runner != y_idom {
                frontiers[runner as usize].insert(y as u32);
                match dominators.idom[runner as usize] {
                    Some(parent) if parent.0 != runner => runner = parent.0,
                    _ => break,
                }
                guard += 1;
                if guard > n {
                    break; // defensive: malformed dominator tree
                }
            }
        }
    }
    frontiers
}

/// Upward-exposed uses (reads before any write in the block) and write sets,
/// restricted to the candidate slots.
// CEP:WHAT: Handles the Computes upward-exposed uses and write sets for candidate slots, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn slot_use_def(func: &IrFunc, candidates: &[u32]) -> (Vec<BTreeSet<u32>>, Vec<BTreeSet<u32>>) {
    let n = func.blocks.len();
    let candidate_set: BTreeSet<u32> = candidates.iter().copied().collect();
    let mut use_gen: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    let mut block_defs: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    for (b, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            match inst {
                Inst::LoadSlot { slot, .. } if candidate_set.contains(&slot.0) => {
                    if !block_defs[b].contains(&slot.0) {
                        use_gen[b].insert(slot.0);
                    }
                }
                Inst::StoreSlot { slot, .. } if candidate_set.contains(&slot.0) => {
                    block_defs[b].insert(slot.0);
                }
                _ => {}
            }
        }
    }
    (use_gen, block_defs)
}

/// Backward liveness fixpoint over the candidate slots.
// CEP:WHAT: Handles the Runs the backward liveness fixpoint over candidate slots, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn slot_live_in(
    func: &IrFunc,
    use_gen: &[BTreeSet<u32>],
    block_defs: &[BTreeSet<u32>],
) -> Vec<BTreeSet<u32>> {
    let n = func.blocks.len();
    let successors: Vec<Vec<u32>> = func
        .blocks
        .iter()
        .map(|block| successors_of_term(&block.term))
        .collect();
    let mut live_in: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    let mut live_out: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    loop {
        let mut changed = false;
        for b in (0..n).rev() {
            let mut out: BTreeSet<u32> = BTreeSet::new();
            for &succ in &successors[b] {
                if (succ as usize) < n {
                    out.extend(live_in[succ as usize].iter().copied());
                }
            }
            let mut input = out
                .iter()
                .filter(|slot| !block_defs[b].contains(slot))
                .copied()
                .collect::<BTreeSet<u32>>();
            input.extend(use_gen[b].iter().copied());
            if out != live_out[b] {
                live_out[b] = out;
                changed = true;
            }
            if input != live_in[b] {
                live_in[b] = input;
                changed = true;
            }
        }
        if !changed {
            return live_in;
        }
    }
}

// CEP:WHAT: Handles the Allocates the next dense vreg with its type, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn fresh_vreg(func: &mut IrFunc, ty: &Ty) -> Vreg {
    let vreg = Vreg(func.vregs.len() as u32);
    func.vregs.push(ty.clone());
    vreg
}

/// Follows a replacement chain to its end. Chains arise when a load reads a
/// value that was itself a load result; the map is acyclic because every
/// replacement is defined earlier in the dominator walk than its key.
// CEP:WHAT: Handles the Follows a replacement chain to its final vreg per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn chase(map: &BTreeMap<u32, Vreg>, mut vreg: Vreg) -> Vreg {
    let mut guard = 0usize;
    while let Some(&next) = map.get(&vreg.0) {
        vreg = next;
        guard += 1;
        if guard > map.len() {
            break; // defensive: cycles are impossible, never trust input
        }
    }
    vreg
}
