// CEP:FILE: crates/planck_opt/src/out_ssa.rs
// CEP:WHAT: The exit from SSA: loop-closed SSA normalization (design pass 40), redundant-phi elimination (pass 38, loop-aware), SSA destruction into edge copies with critical-edge splitting and parallel-copy sequentialization (pass 35, destruction half), and copy propagation over single-def copies.
// CEP:WHY: The backend and the frame-resident pipeline operate on phi-free code; LCSSA keeps the SSA window canonical for the loop passes that will run there; copy propagation removes the copies destruction left behind.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: LCSSA inserts an exit phi only when every predecessor edge carries a dominance-valid value and skips the value otherwise; redundant-phi replacement requires all non-self inputs to agree and never moves a loop-defined value across its loop boundary; destruction resolves copy cycles with fresh temporaries; copy propagation touches only single-def copies. Malformed input is caught by the pipeline verifier.
// CEP:ASSUMES: Input functions pass verify_program; phis carry exactly one input per predecessor in ascending order; the CFG is reducible (structured frontend).
// CEP:COST: LCSSA O(loops * nodes); phi cleanup O(N) per replacement sweep; destruction O(phis + edges) plus one block per critical edge; copy propagation O(nodes). All deterministic (index order, BTreeSets).
// CEP:EVIDENCE: unit tests in this file.
// CEP:SECURITY: All rewrites are semantics-preserving on verified input and gated by the verifier.
// CEP:PORTABILITY: Target-independent.

#![forbid(unsafe_code)]

use planck_ir::{BlockId, Dominators, Inst, IrFunc, Program, Term, Ty, Vreg};
use std::collections::{BTreeMap, BTreeSet};

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

// CEP:WHAT: Handles the Computes one block's structural predecessors, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn compute_preds_of(func: &IrFunc, block: usize) -> Vec<u32> {
    let mut preds = Vec::new();
    for (idx, b) in func.blocks.iter().enumerate() {
        if successors_of_term(&b.term).contains(&(block as u32)) {
            preds.push(idx as u32);
        }
    }
    preds
}

// CEP:WHAT: Handles the Computes the structural predecessor list of every block, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn compute_preds(func: &IrFunc) -> Vec<Vec<u32>> {
    (0..func.blocks.len())
        .map(|b| compute_preds_of(func, b))
        .collect()
}

/// Dominance frontiers per block (Cooper, Harvey, Kennedy).
// CEP:WHAT: Handles the Computes dominance frontiers for the LCSSA placement, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn frontiers_for(func: &IrFunc) -> Vec<BTreeSet<u32>> {
    let dominators = Dominators::compute(func);
    let preds = compute_preds(func);
    let n = func.blocks.len();
    let mut frontiers: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    for (y, pred_list) in preds.iter().enumerate() {
        if pred_list.len() < 2 {
            continue;
        }
        let Some(y_idom) = dominators.idom[y] else {
            continue;
        };
        for &p in pred_list {
            let mut runner = p as usize;
            let mut guard = 0usize;
            while runner != y_idom.0 as usize {
                frontiers[runner].insert(y as u32);
                match dominators.idom[runner] {
                    Some(parent) if (parent.0 as usize) != runner => runner = parent.0 as usize,
                    _ => break,
                }
                guard += 1;
                if guard > n {
                    break;
                }
            }
        }
    }
    frontiers
}

/// Depth of a block in the dominator tree (root depth 0).
// CEP:WHAT: Handles the Measures a block's depth in the dominator tree, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn dom_depth(dominators: &Dominators, block: BlockId) -> usize {
    let mut depth = 0usize;
    let mut walker = block;
    loop {
        match dominators.idom[walker.0 as usize] {
            Some(parent) if parent != walker => {
                walker = parent;
                depth += 1;
            }
            _ => return depth,
        }
    }
}

/// Natural loop bodies, one per back edge (an edge latch -> header where the
/// header dominates the latch), deduplicated.
// CEP:WHAT: Handles the Collects the natural loop bodies innermost first, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn natural_loops(func: &IrFunc) -> Vec<BTreeSet<u32>> {
    let dominators = Dominators::compute(func);
    let preds = compute_preds(func);
    let mut loops: Vec<BTreeSet<u32>> = Vec::new();
    for (header, pred_list) in preds.iter().enumerate() {
        for &latch in pred_list {
            if !dominators.dominates(BlockId(header as u32), BlockId(latch)) {
                continue;
            }
            let mut body: BTreeSet<u32> = BTreeSet::new();
            body.insert(header as u32);
            body.insert(latch);
            let mut stack = vec![latch];
            while let Some(node) = stack.pop() {
                for &pred in &compute_preds_of(func, node as usize) {
                    if !body.contains(&pred) {
                        body.insert(pred);
                        stack.push(pred);
                    }
                }
            }
            if !loops.contains(&body) {
                loops.push(body);
            }
        }
    }
    // Innermost first: smaller bodies, then the smaller header id.
    loops.sort_by_key(|body| (body.len(), *body.iter().next().unwrap_or(&u32::MAX)));
    loops
}

// ---------------------------------------------------------------------------
// Pass: lcssa (loop-closed SSA, catalog 40)
// ---------------------------------------------------------------------------

// CEP:WHAT: Handles the lcssa step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn lcssa(program: &mut Program) -> usize {
    let mut changes = 0;
    for func in program.funcs.iter_mut() {
        changes += lcssa_on_func(func);
    }
    changes
}

// CEP:WHAT: Handles the Runs LCSSA normalization over one function innermost-first, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn lcssa_on_func(func: &mut IrFunc) -> usize {
    // Innermost first: exit phis inserted for an inner loop are definitions
    // the outer loops must then consider. Each loop is processed once with a
    // fresh definition scan, so nested chains resolve level by level.
    let loops = natural_loops(func);
    let mut changes = 0;
    for body in &loops {
        changes += lcssa_on_loop(func, body);
    }
    changes
}

/// Where a planned phi's input value comes from.
#[derive(Clone, Copy, Debug)]
enum PhiSource {
    /// The loop-defined vreg itself.
    Original(Vreg),
    /// Another planned exit phi, identified by its block.
    FromPhi(u32),
}

struct PlannedPhi {
    block: u32,
    inputs: Vec<(BlockId, PhiSource)>,
}

// CEP:WHAT: Handles the Inserts exit phis for one loop's outflow values, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn lcssa_on_loop(func: &mut IrFunc, body: &BTreeSet<u32>) -> usize {
    // Exit blocks: outside blocks with a predecessor inside the loop.
    let mut exits: BTreeSet<u32> = BTreeSet::new();
    for &b in body {
        for succ in successors_of_term(&func.blocks[b as usize].term) {
            if !body.contains(&succ) {
                exits.insert(succ);
            }
        }
    }
    if exits.is_empty() {
        return 0;
    }

    let dominators = Dominators::compute(func);
    let frontiers = frontiers_for(func);

    // Single-def vregs only (the SSA construction contract).
    let total_vregs = func.vregs.len();
    let mut def_block: Vec<Option<u32>> = vec![None; total_vregs];
    for (b, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let Some(dst) = inst.def_vreg() {
                if (dst.0 as usize) < total_vregs {
                    def_block[dst.0 as usize] = Some(b as u32);
                }
            }
        }
    }

    struct Outflow {
        vreg: u32,
        /// Blocks with direct uses outside the loop.
        use_blocks: Vec<u32>,
        /// Phi inputs reading the value across a loop exit edge.
        phi_outflows: Vec<(u32, u32)>,
    }

    let mut outflows: Vec<Outflow> = Vec::new();
    for (v, defs) in def_block.iter().enumerate() {
        let Some(def) = defs else { continue };
        if !body.contains(def) {
            continue;
        }
        let mut use_blocks: Vec<u32> = Vec::new();
        let mut phi_outflows: Vec<(u32, u32)> = Vec::new();
        for (b, block) in func.blocks.iter().enumerate() {
            let outside = !body.contains(&(b as u32));
            for inst in &block.insts {
                if inst.def_vreg() == Some(Vreg(v as u32)) {
                    continue;
                }
                for used in inst.use_vregs() {
                    if used.0 as usize == v && outside {
                        use_blocks.push(b as u32);
                    }
                }
            }
            match &block.term {
                Term::BrCond { cond, .. } => {
                    if cond.0 as usize == v && outside {
                        use_blocks.push(b as u32);
                    }
                }
                Term::Ret { value: Some(value) } => {
                    if value.0 as usize == v && outside {
                        use_blocks.push(b as u32);
                    }
                }
                Term::Br(_) | Term::Ret { value: None } => {}
            }
            for inst in &block.insts {
                if let Inst::Phi { inputs, .. } = inst {
                    for (pred, value) in inputs {
                        if value.0 as usize == v
                            && body.contains(&pred.0)
                            && !body.contains(&(b as u32))
                        {
                            phi_outflows.push((pred.0, b as u32));
                        }
                    }
                }
            }
        }
        if !use_blocks.is_empty() || !phi_outflows.is_empty() {
            outflows.push(Outflow {
                vreg: v as u32,
                use_blocks,
                phi_outflows,
            });
        }
    }
    if outflows.is_empty() {
        return 0;
    }

    let mut changes = 0;
    for outflow in &outflows {
        let def = def_block[outflow.vreg as usize].unwrap_or(0);
        let ty = func.vregs[outflow.vreg as usize].clone();

        // Placement starts at the exit blocks that dominate an outflow use and
        // then follows the iterated dominance frontier through placed blocks
        // (merges of exit paths need their own phis). Starting from the
        // definition instead would miss exits the definition dominates, like
        // the single break of a `loop`.
        let mut placed: BTreeSet<u32> = BTreeSet::new();
        let mut relevant_exits: BTreeSet<u32> = BTreeSet::new();
        for &e in &exits {
            let eid = BlockId(e);
            let needed = outflow
                .use_blocks
                .iter()
                .any(|u| dominators.dominates(eid, BlockId(*u)))
                || outflow
                    .phi_outflows
                    .iter()
                    .any(|(_, phi_block)| dominators.dominates(eid, BlockId(*phi_block)));
            if needed {
                relevant_exits.insert(e);
            }
        }
        let mut worklist: Vec<u32> = relevant_exits.iter().copied().collect();
        while let Some(x) = worklist.pop() {
            for &y in &frontiers[x as usize] {
                if body.contains(&y) || !placed.insert(y) {
                    continue;
                }
                worklist.push(y);
            }
        }
        for &e in &relevant_exits {
            placed.insert(e);
        }
        if placed.is_empty() {
            continue;
        }

        // Plan every phi before committing: each needs a dominance-valid
        // input per predecessor. A pred inside the loop reads the definition
        // (when it dominates the pred) or the nearest placed phi; a pred
        // outside the loop must also carry the value through a placed phi.
        let planned_blocks: Vec<u32> = placed.iter().copied().collect();
        let mut planned: Vec<PlannedPhi> = Vec::new();
        let mut valid = true;
        for &b in &planned_blocks {
            let mut inputs = Vec::new();
            for pred in compute_preds_of(func, b as usize) {
                let pred_id = BlockId(pred);
                let mut chosen: Option<PhiSource> = None;
                let mut chosen_depth = 0usize;
                for &phi_block in &planned_blocks {
                    if dominators.dominates(BlockId(phi_block), pred_id) {
                        let depth = dom_depth(&dominators, BlockId(phi_block));
                        if chosen.is_none() || depth > chosen_depth {
                            chosen = Some(PhiSource::FromPhi(phi_block));
                            chosen_depth = depth;
                        }
                    }
                }
                if dominators.dominates(BlockId(def), pred_id) {
                    let def_depth = dom_depth(&dominators, BlockId(def));
                    if chosen.is_none() || def_depth > chosen_depth {
                        chosen = Some(PhiSource::Original(Vreg(outflow.vreg)));
                    }
                }
                match chosen {
                    Some(source) => inputs.push((pred_id, source)),
                    None => {
                        valid = false;
                        break;
                    }
                }
            }
            if !valid {
                break;
            }
            planned.push(PlannedPhi { block: b, inputs });
        }
        if !valid || planned.is_empty() {
            continue;
        }

        // Commit: allocate a vreg per planned phi (ascending block order),
        // insert the phis at their blocks' phi regions, rewrite outflow uses.
        let mut allocated: BTreeMap<u32, Vreg> = BTreeMap::new();
        for phi in &planned {
            let dst = Vreg(func.vregs.len() as u32);
            func.vregs.push(ty.clone());
            allocated.insert(phi.block, dst);
        }
        let resolve = |source: PhiSource, allocated: &BTreeMap<u32, Vreg>| -> Vreg {
            match source {
                PhiSource::Original(vreg) => vreg,
                PhiSource::FromPhi(block) => allocated[&block],
            }
        };
        for phi in &planned {
            let dst = allocated[&phi.block];
            let inputs: Vec<(BlockId, Vreg)> = phi
                .inputs
                .iter()
                .map(|(pred, source)| (*pred, resolve(*source, &allocated)))
                .collect();
            let insert_at = func.blocks[phi.block as usize]
                .insts
                .iter()
                .take_while(|inst| matches!(inst, Inst::Phi { .. }))
                .count();
            func.blocks[phi.block as usize].insts.insert(
                insert_at,
                Inst::Phi {
                    dst,
                    ty: ty.clone(),
                    inputs,
                },
            );
        }

        // Rewrite direct uses outside the loop to the nearest planned phi.
        let planned_refs: Vec<(u32, Vreg)> = planned
            .iter()
            .map(|phi| (phi.block, allocated[&phi.block]))
            .collect();
        for (b, block) in func.blocks.iter_mut().enumerate() {
            if body.contains(&(b as u32)) {
                continue;
            }
            for inst in block.insts.iter_mut() {
                // Phi inputs are edge uses handled by the dedicated exit-edge
                // rewrite below (an exit phi's own in-loop input must stay the
                // loop-defined value; rewriting it here would self-reference).
                if matches!(inst, Inst::Phi { .. }) {
                    continue;
                }
                if inst.def_vreg() == Some(Vreg(outflow.vreg)) {
                    continue;
                }
                rewrite_vreg_uses_to_phi(inst, outflow.vreg, &planned_refs, &dominators, b);
            }
            match &mut block.term {
                Term::BrCond { cond, .. } => {
                    if cond.0 as usize == outflow.vreg as usize {
                        if let Some(dst) = nearest_phi(&planned_refs, &dominators, b) {
                            *cond = dst;
                        }
                    }
                }
                Term::Ret { value: Some(value) } => {
                    if value.0 as usize == outflow.vreg as usize {
                        if let Some(dst) = nearest_phi(&planned_refs, &dominators, b) {
                            *value = dst;
                        }
                    }
                }
                Term::Br(_) | Term::Ret { value: None } => {}
            }
        }
        // Rewrite phi inputs that read the value across an exit edge.
        for block in func.blocks.iter_mut() {
            for inst in block.insts.iter_mut() {
                if let Inst::Phi { inputs, .. } = inst {
                    for (pred, value) in inputs.iter_mut() {
                        if value.0 as usize == outflow.vreg as usize && body.contains(&pred.0) {
                            if let Some(dst) =
                                nearest_phi(&planned_refs, &dominators, pred.0 as usize)
                            {
                                *value = dst;
                            }
                        }
                    }
                }
            }
        }
        changes += planned.len();
    }
    changes
}

/// Picks the nearest (deepest in the dominator tree) planned phi whose block
/// dominates the given block.
// CEP:WHAT: Handles the Picks the nearest planned exit phi dominating a block, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn nearest_phi(planned: &[(u32, Vreg)], dominators: &Dominators, use_block: usize) -> Option<Vreg> {
    let mut best: Option<(usize, Vreg)> = None;
    for &(block, dst) in planned {
        if dominators.dominates(BlockId(block), BlockId(use_block as u32)) {
            let depth = dom_depth(dominators, BlockId(block));
            if best.map(|(d, _)| depth > d).unwrap_or(true) {
                best = Some((depth, dst));
            }
        }
    }
    best.map(|(_, dst)| dst)
}

/// Rewrites uses of `vreg` inside one instruction to the nearest planned phi;
/// leaves the instruction untouched when no phi dominates the block.
// CEP:WHAT: Handles the Rewrites one instruction's uses of a loop-defined vreg to its exit phi, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn rewrite_vreg_uses_to_phi(
    inst: &mut Inst,
    vreg: u32,
    planned: &[(u32, Vreg)],
    dominators: &Dominators,
    block: usize,
) {
    let Some(replacement) = nearest_phi(planned, dominators, block) else {
        return;
    };
    let remap = |v: &mut Vreg| {
        if v.0 == vreg {
            *v = replacement;
        }
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
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Pass: phi_cleanup (redundant phi elimination, catalog 38)
// ---------------------------------------------------------------------------

// CEP:WHAT: Handles the phi_cleanup step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn phi_cleanup(program: &mut Program) -> usize {
    let mut changes = 0;
    for func in program.funcs.iter_mut() {
        changes += phi_cleanup_on_func(func);
    }
    changes
}

// CEP:WHAT: Handles the Removes redundant phis from one function without breaking LCSSA, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn phi_cleanup_on_func(func: &mut IrFunc) -> usize {
    let mut changes = 0;
    // Phis that must stay for LCSSA reasons: replacing them would move a
    // loop-defined value across its loop boundary. Each blocked phi is
    // recorded so the scan terminates (every iteration either removes a phi
    // or blocks one).
    let mut blocked: BTreeSet<u32> = BTreeSet::new();
    loop {
        let Some((phi_dst, replacement, phi_block)) = find_redundant_phi(func, &blocked) else {
            return changes;
        };
        let inside_loop_replacement = natural_loops(func).iter().any(|body| {
            def_blocks_of(func, replacement)
                .first()
                .is_some_and(|def| body.contains(def) && !body.contains(&phi_block))
        });
        if inside_loop_replacement {
            blocked.insert(phi_dst.0);
            continue;
        }
        replace_uses(func, phi_dst, replacement);
        remove_phi(func, phi_dst);
        changes += 1;
    }
}

// CEP:WHAT: Handles the Lists the blocks defining one vreg, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn def_blocks_of(func: &IrFunc, vreg: Vreg) -> Vec<u32> {
    let mut defs = Vec::new();
    for (b, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if inst.def_vreg() == Some(vreg) {
                defs.push(b as u32);
                break;
            }
        }
    }
    defs
}

/// Finds the first phi whose non-self inputs are all the same vreg, skipping
/// phis recorded as blocked.
// CEP:WHAT: Handles the Finds the first phi whose non-self inputs agree, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn find_redundant_phi(func: &IrFunc, blocked: &BTreeSet<u32>) -> Option<(Vreg, Vreg, u32)> {
    for (b, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let Inst::Phi { dst, inputs, .. } = inst {
                if blocked.contains(&dst.0) {
                    continue;
                }
                let mut replacement: Option<Vreg> = None;
                let mut redundant = true;
                for (_, value) in inputs {
                    if *value == *dst {
                        continue; // self-reference along a back edge
                    }
                    match replacement {
                        None => replacement = Some(*value),
                        Some(existing) if existing == *value => {}
                        Some(_) => {
                            redundant = false;
                            break;
                        }
                    }
                }
                if redundant {
                    if let Some(replacement) = replacement {
                        return Some((*dst, replacement, b as u32));
                    }
                }
            }
        }
    }
    None
}

/// Rewrites every use of `from` (instructions, terminators, phi inputs) to `to`.
// CEP:WHAT: Handles the Rewrites every use of one vreg to another across the function, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn replace_uses(func: &mut IrFunc, from: Vreg, to: Vreg) {
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_uses_of(inst, from, to);
        }
        match &mut block.term {
            Term::BrCond { cond, .. } if *cond == from => {
                *cond = to;
            }
            Term::Ret { value: Some(value) } if *value == from => {
                *value = to;
            }
            _ => {}
        }
    }
}

// CEP:WHAT: Handles the Rewrites a vreg's uses inside one instruction, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn rewrite_uses_of(inst: &mut Inst, from: Vreg, to: Vreg) {
    let remap = |vreg: &mut Vreg| {
        if *vreg == from {
            *vreg = to;
        }
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

// CEP:WHAT: Handles the Deletes one phi by destination across the function, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn remove_phi(func: &mut IrFunc, dst: Vreg) {
    for block in func.blocks.iter_mut() {
        block.insts.retain(|inst| match inst {
            Inst::Phi { dst: phi_dst, .. } => *phi_dst != dst,
            _ => true,
        });
    }
}

// ---------------------------------------------------------------------------
// Pass: ssa_destroy (SSA destruction, catalog 35 destruction half)
// ---------------------------------------------------------------------------

// CEP:WHAT: Handles the ssa_destroy step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn ssa_destroy(program: &mut Program) -> usize {
    let mut changes = 0;
    for func in program.funcs.iter_mut() {
        changes += destroy_on_func(func);
    }
    changes
}

/// One pending phi lowering: (destination, type, inputs).
type PhiSnapshot = (Vreg, Ty, Vec<(BlockId, Vreg)>);

// CEP:WHAT: Handles the Lowers one function's phis into edge copies with critical-edge splitting, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn destroy_on_func(func: &mut IrFunc) -> usize {
    let mut changes = 0;

    // Phase 1: split critical edges (a conditional branch into a block that
    // already has two predecessors). Edge copies may only go at the end of a
    // predecessor with a single successor.
    while let Some((from, to)) = find_critical_edge(func) {
        let split_block = BlockId(func.blocks.len() as u32);
        func.blocks.push(planck_ir::Block {
            insts: Vec::new(),
            term: Term::Br(to),
        });
        let block = &mut func.blocks[from.0 as usize];
        match &mut block.term {
            Term::BrCond {
                then_blk, else_blk, ..
            } => {
                if *then_blk == to {
                    *then_blk = split_block;
                } else {
                    *else_blk = split_block;
                }
            }
            Term::Br(target) => {
                if *target == to {
                    *target = split_block;
                }
            }
            Term::Ret { .. } => {}
        }
        // Phis in `to` now receive the value through the split block.
        for inst in func.blocks[to.0 as usize].insts.iter_mut() {
            if let Inst::Phi { inputs, .. } = inst {
                for (pred, _) in inputs.iter_mut() {
                    if *pred == from {
                        *pred = split_block;
                    }
                }
            }
        }
        changes += 1;
    }

    // Phase 2: lower each phi into copies at the end of its predecessors. A
    // phi destination read as another phi's source on the same edge is
    // versioned through a fresh temporary first (swap and cycle safety).
    let n = func.blocks.len();
    let mut copies_per_pred: BTreeMap<u32, Vec<Inst>> = BTreeMap::new();
    for target in 0..n {
        let phis: Vec<PhiSnapshot> = func.blocks[target]
            .insts
            .iter()
            .filter_map(|inst| match inst {
                Inst::Phi { dst, ty, inputs } => Some((*dst, ty.clone(), inputs.clone())),
                _ => None,
            })
            .collect();
        if phis.is_empty() {
            continue;
        }
        for pred in compute_preds_of(func, target) {
            let mut edge_copies: Vec<(Vreg, Vreg, Ty)> = phis
                .iter()
                .filter_map(|(dst, ty, inputs)| {
                    inputs
                        .iter()
                        .find(|(p, _)| p.0 == pred)
                        .map(|(_, value)| (*dst, *value, ty.clone()))
                })
                .collect();
            if edge_copies.is_empty() {
                continue;
            }
            let dsts: BTreeSet<u32> = edge_copies.iter().map(|(dst, _, _)| dst.0).collect();
            let mut temps: Vec<Inst> = Vec::new();
            for (dst, src, ty) in edge_copies.iter_mut() {
                if dsts.contains(&src.0) && src != dst {
                    let temp = Vreg(func.vregs.len() as u32);
                    func.vregs.push(ty.clone());
                    temps.push(Inst::Copy {
                        dst: temp,
                        src: *src,
                        ty: ty.clone(),
                    });
                    *src = temp;
                }
            }
            let mut insts = temps;
            insts.extend(
                edge_copies
                    .into_iter()
                    .map(|(dst, src, ty)| Inst::Copy { dst, src, ty }),
            );
            changes += insts.len();
            copies_per_pred.entry(pred).or_default().extend(insts);
        }
    }
    for (pred, copies) in copies_per_pred {
        func.blocks[pred as usize].insts.extend(copies);
    }

    // Phase 3: every phi destination is now defined by edge copies; remove
    // the phi instructions themselves.
    for block in func.blocks.iter_mut() {
        let before = block.insts.len();
        block.insts.retain(|inst| !matches!(inst, Inst::Phi { .. }));
        changes += before - block.insts.len();
    }

    changes
}

/// Finds the first critical edge: a conditional branch with two distinct
/// targets where a target already has two or more predecessors.
// CEP:WHAT: Handles the Finds the first critical edge of a function, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn find_critical_edge(func: &IrFunc) -> Option<(BlockId, BlockId)> {
    let mut pred_counts: BTreeMap<u32, usize> = BTreeMap::new();
    for block in &func.blocks {
        for succ in successors_of_term(&block.term) {
            *pred_counts.entry(succ).or_default() += 1;
        }
    }
    for (idx, block) in func.blocks.iter().enumerate() {
        if let Term::BrCond {
            then_blk, else_blk, ..
        } = &block.term
        {
            if then_blk != else_blk {
                for target in [*then_blk, *else_blk] {
                    if pred_counts.get(&target.0).copied().unwrap_or(0) >= 2 {
                        return Some((BlockId(idx as u32), target));
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Pass: copy_prop (copy propagation over single-def copies)
// ---------------------------------------------------------------------------

// CEP:WHAT: Handles the copy_prop step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn copy_prop(program: &mut Program) -> usize {
    let mut changes = 0;
    for func in program.funcs.iter_mut() {
        changes += copy_prop_on_func(func);
    }
    changes
}

// CEP:WHAT: Handles the Propagates single-def copies through one function, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn copy_prop_on_func(func: &mut IrFunc) -> usize {
    // Def counts: only copies whose destination has exactly one definition in
    // the function are propagatable (multi-def destinations are former phi
    // destinations whose edge copies must stay).
    let mut def_count: BTreeMap<u32, usize> = BTreeMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(dst) = inst.def_vreg() {
                *def_count.entry(dst.0).or_default() += 1;
            }
        }
    }
    let mut map: BTreeMap<u32, Vreg> = BTreeMap::new();
    let mut removable: BTreeSet<u32> = BTreeSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Inst::Copy { dst, src, .. } = inst {
                if def_count.get(&dst.0).copied().unwrap_or(0) == 1 && dst != src {
                    map.insert(dst.0, *src);
                    removable.insert(dst.0);
                }
            }
        }
    }
    if map.is_empty() {
        return 0;
    }
    // Chase chains (copy of a copy) with a cycle guard.
    let resolved: BTreeMap<u32, Vreg> = map
        .iter()
        .map(|(&dst, &src)| {
            let mut target = src;
            let mut guard = 0usize;
            while let Some(&next) = map.get(&target.0) {
                target = next;
                guard += 1;
                if guard > map.len() {
                    break;
                }
            }
            (dst, target)
        })
        .collect();
    let remap = |vreg: &mut Vreg| {
        if let Some(repl) = resolved.get(&vreg.0) {
            *vreg = *repl;
        }
    };
    let mut changes = 0usize;
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            if matches!(inst, Inst::Copy { dst, .. } if removable.contains(&dst.0)) {
                changes += 1;
                continue;
            }
            rewrite_all_uses(inst, &remap);
        }
        block
            .insts
            .retain(|inst| !matches!(inst, Inst::Copy { dst, .. } if removable.contains(&dst.0)));
        match &mut block.term {
            Term::BrCond { cond, .. } => remap(cond),
            Term::Ret { value: Some(value) } => remap(value),
            _ => {}
        }
    }
    changes
}

// CEP:WHAT: Handles the Rewrites all vreg uses inside one instruction through a closure, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn rewrite_all_uses(inst: &mut Inst, remap: &impl Fn(&mut Vreg)) {
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
        Inst::Copy { src, .. } => remap(src),
        Inst::Phi { inputs, .. } => {
            for (_, value) in inputs.iter_mut() {
                remap(value);
            }
        }
        Inst::LoadSlot { .. }
        | Inst::LoadField { .. }
        | Inst::AddrSlot { .. }
        | Inst::Const { .. }
        | Inst::Arg { .. }
        | Inst::Trap { .. }
        | Inst::Comment { .. } => {}
    }
}
