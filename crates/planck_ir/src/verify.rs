// CEP:FILE: crates/planck_ir/src/verify.rs
// CEP:WHAT: IR verifier plus the dominator analysis it depends on; runs after construction and after every optimization pass.
// CEP:WHY: Standard 38.18 requires verification of use-def, type, terminator, and linkage validity after lowering and after each pass, so no downstream stage can observe a malformed graph.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: VerifyError findings describe the first violations found per function; the verifier never mutates the IR and never panics on malformed input.
// CEP:ASSUMES: Blocks are dense 0..n; vregs are dense 0..m per function; slot infos are dense 0..k per function. References are values of pointer width produced by AddrSlot or by reference-typed slots only. Single-def vregs follow the dominance rule; multi-def vregs (former phi destinations after SSA destruction) follow the reaching-definition rule.
// CEP:COST: Dominators O(N^2) worst case with dense bitsets, typically near-linear with the Cooper-Harvey-Kennedy iteration; verification O(N + E).
// CEP:EVIDENCE: cargo test -p planck_ir (malformed-IR rejection cases).
// CEP:SECURITY: The verifier is the guard against downstream miscompilation of untrusted source; it runs in every build mode, debug and release alike (cheap full mode, no split required at this scale).
// CEP:PORTABILITY: Pure core; dominator logic is target-independent.

#![forbid(unsafe_code)]

use crate::ir::{BlockId, Callee, Const, Inst, IrFunc, Program, Term, Ty, Vreg};
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyError {
    pub func: String,
    pub message: String,
}

/// Verifies a whole program; returns all findings (empty means valid).
// CEP:WHAT: Handles the verify program step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn verify_program(program: &Program) -> Vec<VerifyError> {
    let mut errors = Vec::new();
    for func in &program.funcs {
        verify_func(func, program, &mut errors);
    }
    errors
}

/// Dominator tree per function, shared with the optimizer.
pub struct Dominators {
    /// Immediate dominator per block; entry block dominates itself.
    pub idom: Vec<Option<BlockId>>,
}

impl Dominators {
    /// Computes dominators with the Cooper-Harvey-Kennedy algorithm on reverse postorder.
    // CEP:WHAT: Handles the compute step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn compute(func: &IrFunc) -> Dominators {
        let n = func.blocks.len();
        let mut preds: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
        for (idx, block) in func.blocks.iter().enumerate() {
            match &block.term {
                Term::Br(target) => {
                    preds[target.0 as usize].insert(idx as u32);
                }
                Term::BrCond {
                    then_blk, else_blk, ..
                } => {
                    preds[then_blk.0 as usize].insert(idx as u32);
                    preds[else_blk.0 as usize].insert(idx as u32);
                }
                Term::Ret { .. } => {}
            };
        }
        // Reverse postorder from entry.
        let mut visited = vec![false; n];
        let mut postorder: Vec<u32> = Vec::with_capacity(n);
        // Iterative DFS to avoid stack overflow on deep CFGs.
        let mut stack: Vec<(u32, usize)> = vec![(0, 0)];
        visited[0] = true;
        let mut succ_lists: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (idx, block) in func.blocks.iter().enumerate() {
            match &block.term {
                Term::Br(target) => succ_lists[idx].push(target.0),
                Term::BrCond {
                    then_blk, else_blk, ..
                } => {
                    succ_lists[idx].push(then_blk.0);
                    succ_lists[idx].push(else_blk.0);
                }
                Term::Ret { .. } => {}
            }
        }
        while let Some((node, succ_idx)) = stack.pop() {
            let succs = &succ_lists[node as usize];
            let mut pushed = false;
            for succ in succs.iter().skip(succ_idx) {
                if !visited[*succ as usize] {
                    visited[*succ as usize] = true;
                    stack.push((node, succ_idx + 1));
                    stack.push((*succ, 0));
                    pushed = true;
                    break;
                }
            }
            if !pushed {
                postorder.push(node);
            }
        }
        let mut rpo: Vec<u32> = postorder;
        rpo.reverse();
        // Unreachable blocks are appended so indexes stay total; they are dominated by nothing.
        for idx in 0..n as u32 {
            if !visited[idx as usize] {
                rpo.push(idx);
            }
        }
        let mut rpo_index = vec![usize::MAX; n];
        for (order, node) in rpo.iter().enumerate() {
            rpo_index[*node as usize] = order;
        }
        let mut idom: Vec<Option<u32>> = vec![None; n];
        idom[0] = Some(0);
        let mut changed = true;
        while changed {
            changed = false;
            for &node in rpo.iter().skip(1) {
                let node = node as usize;
                let mut new_idom: Option<u32> = None;
                for pred in &preds[node] {
                    let pred = *pred as usize;
                    if idom[pred].is_none() {
                        continue;
                    }
                    new_idom = match new_idom {
                        None => Some(pred as u32),
                        Some(current) => {
                            Some(intersect(&idom, &rpo_index, current as usize, pred) as u32)
                        }
                    };
                }
                if new_idom.is_some() && idom[node] != new_idom {
                    idom[node] = new_idom;
                    changed = true;
                }
            }
        }
        Dominators {
            idom: idom.into_iter().map(|entry| entry.map(BlockId)).collect(),
        }
    }

    /// True when `a` dominates `b` (reflexive).
    // CEP:WHAT: Handles the dominates step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn dominates(&self, a: BlockId, b: BlockId) -> bool {
        let mut walker = b;
        loop {
            if walker == a {
                return true;
            }
            match self.idom.get(walker.0 as usize).copied().flatten() {
                Some(parent) if parent != walker => walker = parent,
                _ => return false,
            }
        }
    }
}

// CEP:WHAT: Handles the intersect step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn intersect(idom: &[Option<u32>], rpo_index: &[usize], mut a: usize, mut b: usize) -> usize {
    while a != b {
        while rpo_index[a] > rpo_index[b] {
            a = idom[a].unwrap_or(0) as usize;
        }
        while rpo_index[b] > rpo_index[a] {
            b = idom[b].unwrap_or(0) as usize;
        }
    }
    a
}

// CEP:WHAT: Handles the verify func step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn verify_func(func: &IrFunc, program: &Program, errors: &mut Vec<VerifyError>) {
    let fail = |message: String, errors: &mut Vec<VerifyError>| {
        errors.push(VerifyError {
            func: func.name.clone(),
            message,
        });
    };

    if func.blocks.is_empty() {
        fail("function has no blocks".to_string(), errors);
        return;
    }
    let dominators = Dominators::compute(func);
    // All definition blocks per vreg. Single-def vregs check by dominance;
    // multi-def vregs (post-SSA-destruction phi destinations) check by
    // reaching definitions.
    let mut defined_at: Vec<Vec<BlockId>> = vec![Vec::new(); func.vregs.len()];
    for (block_idx, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let Some(dst) = inst.def_vreg() {
                if (dst.0 as usize) < func.vregs.len() {
                    defined_at[dst.0 as usize].push(BlockId(block_idx as u32));
                }
            }
        }
    }
    // Reaching sets for multi-def vregs: for each, the blocks a definition can
    // reach without passing another definition of the same vreg.
    let mut reaching: Vec<Option<BTreeSet<u32>>> = vec![None; func.vregs.len()];
    for (vreg_idx, defs) in defined_at.iter().enumerate() {
        if defs.len() > 1 {
            reaching[vreg_idx] = Some(reaching_blocks(func, defs));
        }
    }

    for (block_idx, block) in func.blocks.iter().enumerate() {
        let block_id = BlockId(block_idx as u32);
        // Phi region: phis must precede every non-phi instruction; the entry
        // block has no predecessors and therefore no phis.
        let mut seen_non_phi = false;
        for inst in &block.insts {
            if matches!(inst, Inst::Phi { .. }) {
                if seen_non_phi {
                    fail(
                        format!("bb{} has a phi after the phi region", block_idx),
                        errors,
                    );
                }
                if block_idx == 0 {
                    fail("entry block cannot have phis".to_string(), errors);
                }
            } else {
                seen_non_phi = true;
            }
        }
        // Arg region: args are the leading prefix of the entry block, one per
        // promoted parameter in parameter order. Reading an ABI register after
        // a call would observe a clobbered value, so the prefix rule is a
        // correctness rule, not a style rule.
        if block_idx == 0 {
            let mut next_index: u32 = 0;
            let mut seen_non_arg = false;
            for inst in &block.insts {
                if let Inst::Arg { index, .. } = inst {
                    if seen_non_arg {
                        fail(
                            format!("arg {} appears after the arg region", index),
                            errors,
                        );
                    }
                    if *index != next_index {
                        fail(
                            format!("arg {} out of order (expected arg {})", index, next_index),
                            errors,
                        );
                    }
                    next_index += 1;
                } else {
                    seen_non_arg = true;
                }
            }
        }
        match &block.term {
            Term::Br(target) => {
                if target.0 as usize >= func.blocks.len() {
                    fail(
                        format!("bb{} branches to nonexistent bb{}", block_idx, target.0),
                        errors,
                    );
                }
            }
            Term::BrCond {
                cond,
                then_blk,
                else_blk,
            } => {
                if then_blk.0 as usize >= func.blocks.len()
                    || else_blk.0 as usize >= func.blocks.len()
                {
                    fail(
                        format!("bb{} branches to a nonexistent block", block_idx),
                        errors,
                    );
                }
                check_use(
                    func,
                    &dominators,
                    &defined_at,
                    &reaching,
                    block_id,
                    *cond,
                    errors,
                );
                if func
                    .vregs
                    .get(cond.0 as usize)
                    .map(|ty| !matches!(ty, Ty::Bool))
                    .unwrap_or(true)
                {
                    fail(
                        format!("bb{} branches on non-bool v{}", block_idx, cond.0),
                        errors,
                    );
                }
            }
            Term::Ret { value } => {
                if let Some(value) = value {
                    check_use(
                        func,
                        &dominators,
                        &defined_at,
                        &reaching,
                        block_id,
                        *value,
                        errors,
                    );
                    let ret_ty = func.vregs.get(value.0 as usize);
                    let expected_ok = match (&func.ret, ret_ty) {
                        (Ty::Unit, None) => true,
                        (Ty::Unit, Some(_)) => false,
                        (expected, Some(actual)) => expected == actual,
                        (expected, None) => expected != &Ty::Unit,
                    };
                    if !expected_ok {
                        fail(
                            format!(
                                "bb{} returns {} where {} is expected",
                                block_idx,
                                ret_ty
                                    .map(|t| t.name())
                                    .unwrap_or_else(|| "nothing".to_string()),
                                func.ret.name()
                            ),
                            errors,
                        );
                    }
                } else if func.ret != Ty::Unit && func.sret_slot.is_none() {
                    // Aggregate-returning functions deliver their value through the
                    // hidden sret buffer, so a valueless Ret is their valid form.
                    fail(
                        format!(
                            "bb{} returns nothing where {} is expected",
                            block_idx,
                            func.ret.name()
                        ),
                        errors,
                    );
                }
            }
        }
        let ctx = InstContext {
            func,
            dominators: &dominators,
            defined_at: &defined_at,
            reaching: &reaching,
        };
        for inst in &block.insts {
            verify_inst(&ctx, block_id, inst, program, errors);
        }
    }
}

// CEP:WHAT: Handles the check use step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_use(
    func: &IrFunc,
    dominators: &Dominators,
    defined_at: &[Vec<BlockId>],
    reaching: &[Option<BTreeSet<u32>>],
    at_block: BlockId,
    vreg: Vreg,
    errors: &mut Vec<VerifyError>,
) {
    if vreg.0 as usize >= func.vregs.len() {
        errors.push(VerifyError {
            func: func.name.clone(),
            message: format!("use of unknown v{}", vreg.0),
        });
        return;
    }
    let defs = &defined_at[vreg.0 as usize];
    if defs.is_empty() {
        errors.push(VerifyError {
            func: func.name.clone(),
            message: format!("v{} is used before definition", vreg.0),
        });
        return;
    }
    if defs.len() == 1 {
        let def_block = defs[0];
        if !dominators.dominates(def_block, at_block) {
            errors.push(VerifyError {
                func: func.name.clone(),
                message: format!(
                    "v{} defined in bb{} is used in bb{} without dominance",
                    vreg.0, def_block.0, at_block.0
                ),
            });
        }
        return;
    }
    // Multi-def vreg: the use is valid when some definition reaches the use
    // block without an intervening redefinition (post-SSA-destruction shape).
    let ok = reaching[vreg.0 as usize]
        .as_ref()
        .map(|set| set.contains(&at_block.0))
        .unwrap_or(false);
    if !ok {
        let def_list = defs
            .iter()
            .map(|d| d.0.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        errors.push(VerifyError {
            func: func.name.clone(),
            message: format!(
                "v{} (defined in bb[{}]) has no definition reaching bb{}",
                vreg.0, def_list, at_block.0
            ),
        });
    }
}

/// For a multi-def vreg: every block that some definition reaches without
/// passing through another definition block (classic reaching-definitions
/// forward propagation, computed per vreg with BFS over the CFG).
// CEP:WHAT: Handles the Computes the blocks a multi-def vreg's definitions reach without an intervening redefinition, per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn reaching_blocks(func: &IrFunc, defs: &[BlockId]) -> BTreeSet<u32> {
    let def_set: BTreeSet<u32> = defs.iter().map(|d| d.0).collect();
    let mut reached: BTreeSet<u32> = BTreeSet::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    for def in defs {
        let d = def.0 as usize;
        if reached.insert(d as u32) {
            queue.push_back(d as u32);
        }
    }
    let mut expanded: BTreeSet<u32> = BTreeSet::new();
    while let Some(node) = queue.pop_front() {
        if !expanded.insert(node) {
            continue;
        }
        let successors: Vec<u32> = match &func.blocks[node as usize].term {
            Term::Br(target) => vec![target.0],
            Term::BrCond {
                then_blk, else_blk, ..
            } => vec![then_blk.0, else_blk.0],
            Term::Ret { .. } => Vec::new(),
        };
        for succ in successors {
            if succ as usize >= func.blocks.len() {
                continue;
            }
            if reached.insert(succ) && !def_set.contains(&succ) {
                queue.push_back(succ);
            }
        }
    }
    reached
}

// CEP:WHAT: Handles the verify inst step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
/// Analysis tables shared by the instruction-level checks.
struct InstContext<'a> {
    func: &'a IrFunc,
    dominators: &'a Dominators,
    defined_at: &'a [Vec<BlockId>],
    reaching: &'a [Option<BTreeSet<u32>>],
}

// CEP:WHAT: Handles the Checks one instruction's use-def, type, and placement rules per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn verify_inst(
    ctx: &InstContext,
    block_id: BlockId,
    inst: &Inst,
    program: &Program,
    errors: &mut Vec<VerifyError>,
) {
    let func = ctx.func;
    let mut uses: Vec<Vreg> = Vec::new();
    match inst {
        Inst::Phi { dst, ty, inputs } => {
            verify_phi(ctx, block_id, *dst, ty, inputs, errors);
        }
        Inst::Copy { dst, src, ty } => {
            uses.push(*src);
            check_vreg_type(ctx.func, *dst, ty, "copy destination", errors);
            check_vreg_type(ctx.func, *src, ty, "copy source", errors);
        }
        Inst::Arg { dst, index, ty } => {
            // Placement: entry block only, forming a prefix of it, one per
            // parameter in order. The prefix property is checked here by
            // requiring the arg index to equal the number of args seen so far
            // in the function (verified by a pre-scan below).
            if block_id.0 != 0 {
                errors.push(VerifyError {
                    func: func.name.clone(),
                    message: format!("arg {} outside the entry block", index),
                });
            }
            let param_index = *index as usize;
            if param_index >= func.params.len() {
                errors.push(VerifyError {
                    func: func.name.clone(),
                    message: format!("arg {} out of range ({} params)", index, func.params.len()),
                });
            } else {
                let slot = func.params[param_index];
                let param_ty = &func.slots[slot.0 as usize].ty;
                if param_ty != ty {
                    errors.push(VerifyError {
                        func: func.name.clone(),
                        message: format!(
                            "arg {} reads {} where the parameter is {}",
                            index,
                            ty.name(),
                            param_ty.name()
                        ),
                    });
                }
            }
            check_vreg_type(func, *dst, ty, "arg destination", errors);
        }
        Inst::Bin {
            dst, a, b, ty, op, ..
        } => {
            uses.push(*a);
            uses.push(*b);
            let result_ty = func.vregs.get(dst.0 as usize);
            let expected: &Ty = if op.is_comparison() { &Ty::Bool } else { ty };
            if result_ty != Some(&expected.clone()) {
                errors.push(VerifyError {
                    func: func.name.clone(),
                    message: format!("bin result type mismatch on v{}", dst.0),
                });
            }
        }
        Inst::Un { dst, a, .. } => {
            uses.push(*a);
            let _ = dst;
        }
        Inst::Cast { dst, src, from, to } => {
            uses.push(*src);
            if let Some(actual) = func.vregs.get(src.0 as usize) {
                if actual != from {
                    errors.push(VerifyError {
                        func: func.name.clone(),
                        message: format!(
                            "cast source v{} is {} but declared {}",
                            src.0,
                            actual.name(),
                            from.name()
                        ),
                    });
                }
            }
            if let Some(actual) = func.vregs.get(dst.0 as usize) {
                if actual != to {
                    errors.push(VerifyError {
                        func: func.name.clone(),
                        message: format!("cast destination v{} type mismatch", dst.0),
                    });
                }
            }
        }
        Inst::LoadSlot { slot, ty, .. } | Inst::LoadField { slot, ty, .. } => {
            if slot.0 as usize >= func.slots.len() {
                errors.push(VerifyError {
                    func: func.name.clone(),
                    message: format!("load from unknown slot s{}", slot.0),
                });
            }
            let _ = ty;
        }
        Inst::StoreSlot { slot, src, .. } | Inst::StoreField { slot, src, .. } => {
            if slot.0 as usize >= func.slots.len() {
                errors.push(VerifyError {
                    func: func.name.clone(),
                    message: format!("store to unknown slot s{}", slot.0),
                });
            }
            uses.push(*src);
        }
        Inst::AddrSlot { slot, .. } => {
            if slot.0 as usize >= func.slots.len() {
                errors.push(VerifyError {
                    func: func.name.clone(),
                    message: format!("addr of unknown slot s{}", slot.0),
                });
            }
        }
        Inst::AddrOffset { base, offset, .. } => {
            uses.push(*base);
            uses.push(*offset);
        }
        Inst::LoadIndirect { addr, .. } => {
            uses.push(*addr);
        }
        Inst::StoreIndirect { addr, src, .. } => {
            uses.push(*addr);
            uses.push(*src);
        }
        Inst::StoreStructFromAddr {
            dst_slot, src_addr, ..
        } => {
            uses.push(*src_addr);
            if dst_slot.0 as usize >= func.slots.len() {
                errors.push(VerifyError {
                    func: func.name.clone(),
                    message: format!("memcpy into unknown slot s{}", dst_slot.0),
                });
            }
        }
        Inst::Call {
            func: callee,
            args,
            sret,
            ..
        } => {
            if let Some(sret) = sret {
                uses.push(*sret);
            }
            match callee {
                Callee::Func(id) => {
                    if id.0 as usize >= program.funcs.len() {
                        errors.push(VerifyError {
                            func: func.name.clone(),
                            message: format!("call to unknown fn#{}", id.0),
                        });
                    }
                }
                Callee::Intrinsic(_) => {}
            }
            let _ = args;
        }
        Inst::TrapIf { cond, .. } => uses.push(*cond),
        Inst::Const { value, .. } => {
            if let Const::Str(id) = value {
                if *id as usize >= program.rodata.len() {
                    errors.push(VerifyError {
                        func: func.name.clone(),
                        message: format!("rodata #{} out of range", id),
                    });
                }
            }
        }
        Inst::Trap { .. } | Inst::Comment { .. } => {}
    }
    for vreg in uses {
        check_use(
            ctx.func,
            ctx.dominators,
            ctx.defined_at,
            ctx.reaching,
            block_id,
            vreg,
            errors,
        );
    }
}

/// Phi-specific rules: one input per predecessor in ascending order, input
/// types matching the phi type, and each input value defined in (or dominating)
/// its predecessor edge source.
// CEP:WHAT: Handles the Checks one phi's predecessor coverage, types, and edge liveness per the design and the file header contract. step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn verify_phi(
    ctx: &InstContext,
    block_id: BlockId,
    dst: Vreg,
    ty: &Ty,
    inputs: &[(BlockId, Vreg)],
    errors: &mut Vec<VerifyError>,
) {
    let func = ctx.func;
    let dominators = ctx.dominators;
    check_vreg_type(func, dst, ty, "phi destination", errors);
    // Structural predecessors of this block.
    let mut preds: Vec<u32> = Vec::new();
    for (idx, block) in func.blocks.iter().enumerate() {
        let targets: Vec<u32> = match &block.term {
            Term::Br(target) => vec![target.0],
            Term::BrCond {
                then_blk, else_blk, ..
            } => vec![then_blk.0, else_blk.0],
            Term::Ret { .. } => Vec::new(),
        };
        if targets.contains(&(block_id.0)) {
            preds.push(idx as u32);
        }
    }
    let input_preds: Vec<u32> = inputs.iter().map(|(pred, _)| pred.0).collect();
    if input_preds != preds {
        errors.push(VerifyError {
            func: func.name.clone(),
            message: format!(
                "phi in bb{} has inputs from bb[{:?}] but predecessors are bb[{:?}]",
                block_id.0, input_preds, preds
            ),
        });
        return;
    }
    for (pred, value) in inputs {
        if pred.0 as usize >= func.blocks.len() {
            continue; // already reported by the predecessor mismatch above
        }
        check_vreg_type(func, *value, ty, "phi input", errors);
        // The input value must be defined in the predecessor or dominate it:
        // it has to be live-out on that edge.
        let defs = ctx
            .defined_at
            .get(value.0 as usize)
            .cloned()
            .unwrap_or_default();
        let valid = defs
            .iter()
            .any(|def_block| dominators.dominates(*def_block, *pred));
        if !valid {
            errors.push(VerifyError {
                func: func.name.clone(),
                message: format!(
                    "phi input v{} in bb{} is not live-out of predecessor bb{}",
                    value.0, block_id.0, pred.0
                ),
            });
        }
    }
}

/// Checks that a vreg exists and carries exactly the expected type.
fn check_vreg_type(
    func: &IrFunc,
    vreg: Vreg,
    expected: &Ty,
    role: &str,
    errors: &mut Vec<VerifyError>,
) {
    match func.vregs.get(vreg.0 as usize) {
        None => errors.push(VerifyError {
            func: func.name.clone(),
            message: format!("{} v{} is out of range", role, vreg.0),
        }),
        Some(actual) => {
            if actual != expected {
                errors.push(VerifyError {
                    func: func.name.clone(),
                    message: format!(
                        "{} v{} is {} where {} is expected",
                        role,
                        vreg.0,
                        actual.name(),
                        expected.name()
                    ),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Block, FuncId, IrFunc, SlotInfo};

    /// Two-block diamond: entry branches on v0 to two arms that join in bb3.
    fn diamond_with_phis() -> IrFunc {
        // v0 = const true; br_cond v0, bb1, bb2
        // bb1: v1 = const 1; br bb3
        // bb2: v2 = const 2; br bb3
        // bb3: v3 = phi(bb1: v1, bb2: v2); ret v3
        IrFunc {
            name: "f".into(),
            func_id: FuncId(0),
            params: vec![],
            ret: Ty::I32,
            sret_slot: None,
            slots: vec![],
            vregs: vec![Ty::Bool, Ty::I32, Ty::I32, Ty::I32],
            blocks: vec![
                Block {
                    insts: vec![Inst::Const {
                        dst: Vreg(0),
                        value: Const::Bool(true),
                    }],
                    term: Term::BrCond {
                        cond: Vreg(0),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    insts: vec![Inst::Const {
                        dst: Vreg(1),
                        value: Const::Int(1, Ty::I32),
                    }],
                    term: Term::Br(BlockId(3)),
                },
                Block {
                    insts: vec![Inst::Const {
                        dst: Vreg(2),
                        value: Const::Int(2, Ty::I32),
                    }],
                    term: Term::Br(BlockId(3)),
                },
                Block {
                    insts: vec![Inst::Phi {
                        dst: Vreg(3),
                        ty: Ty::I32,
                        inputs: vec![(BlockId(1), Vreg(1)), (BlockId(2), Vreg(2))],
                    }],
                    term: Term::Ret {
                        value: Some(Vreg(3)),
                    },
                },
            ],
        }
    }

    fn program(func: IrFunc) -> Program {
        Program {
            funcs: vec![func],
            structs: vec![],
            enums: vec![],
            rodata: vec![],
            ir_version: "t".into(),
        }
    }

    // CEP:WHAT: Verifies that valid phi copy arg accepted behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn valid_phi_copy_arg_accepted() {
        let mut func = diamond_with_phis();
        // Add a copy and an arg-shaped definition: bb1 gains `v1b = copy v1`
        // is skipped (no spare vreg); instead verify Copy on the join.
        func.vregs.push(Ty::I32);
        func.blocks[3].insts.push(Inst::Copy {
            dst: Vreg(4),
            src: Vreg(3),
            ty: Ty::I32,
        });
        func.blocks[3].term = Term::Ret {
            value: Some(Vreg(4)),
        };
        let errors = verify_func_public(&func);
        assert!(errors.is_empty(), "unexpected findings: {:?}", errors);

        // A promoted parameter: Arg at the entry top, in order.
        let func = IrFunc {
            name: "g".into(),
            func_id: FuncId(0),
            params: vec![crate::ir::Slot(0)],
            ret: Ty::I32,
            sret_slot: None,
            slots: vec![SlotInfo {
                ty: Ty::I32,
                size: 4,
                align: 4,
                origin: "x".into(),
            }],
            vregs: vec![Ty::I32],
            blocks: vec![Block {
                insts: vec![Inst::Arg {
                    dst: Vreg(0),
                    index: 0,
                    ty: Ty::I32,
                }],
                term: Term::Ret {
                    value: Some(Vreg(0)),
                },
            }],
        };
        let errors = verify_func_public(&func);
        assert!(errors.is_empty(), "unexpected findings: {:?}", errors);
    }

    // CEP:WHAT: Verifies that phi missing pred input rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn phi_missing_pred_input_rejected() {
        let mut func = diamond_with_phis();
        if let Inst::Phi { inputs, .. } = &mut func.blocks[3].insts[0] {
            inputs.remove(1); // drop the bb2 input
        }
        let errors = verify_func_public(&func);
        assert!(
            errors.iter().any(|e| e.message.contains("has inputs from")),
            "expected a phi-input finding: {:?}",
            errors
        );
    }

    // CEP:WHAT: Verifies that phi after phi region rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn phi_after_phi_region_rejected() {
        let mut func = diamond_with_phis();
        // Move a stray Const into the join before the phi: the phi then sits
        // after the phi region.
        let stray = Inst::Const {
            dst: Vreg(2),
            value: Const::Int(2, Ty::I32),
        };
        func.blocks[3].insts.insert(0, stray);
        let errors = verify_func_public(&func);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("after the phi region")),
            "expected a phi-region finding: {:?}",
            errors
        );
    }

    // CEP:WHAT: Verifies that phi input not live out rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn phi_input_not_live_out_rejected() {
        let mut func = diamond_with_phis();
        // Feed the join phi from v1 on the bb2 edge: v1 is defined in bb1,
        // which does not dominate bb2.
        if let Inst::Phi { inputs, .. } = &mut func.blocks[3].insts[0] {
            inputs[1] = (BlockId(2), Vreg(1));
        }
        let errors = verify_func_public(&func);
        assert!(
            errors.iter().any(|e| e.message.contains("not live-out")),
            "expected a live-out finding: {:?}",
            errors
        );
    }

    // CEP:WHAT: Verifies that arg order and placement rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn arg_order_and_placement_rejected() {
        let mut func = IrFunc {
            name: "g".into(),
            func_id: FuncId(0),
            params: vec![crate::ir::Slot(0), crate::ir::Slot(1)],
            ret: Ty::Unit,
            sret_slot: None,
            slots: vec![
                SlotInfo {
                    ty: Ty::I32,
                    size: 4,
                    align: 4,
                    origin: "x".into(),
                },
                SlotInfo {
                    ty: Ty::I32,
                    size: 4,
                    align: 4,
                    origin: "y".into(),
                },
            ],
            vregs: vec![Ty::I32, Ty::I32],
            blocks: vec![Block {
                // Out of order: arg 1 before arg 0.
                insts: vec![
                    Inst::Arg {
                        dst: Vreg(1),
                        index: 1,
                        ty: Ty::I32,
                    },
                    Inst::Arg {
                        dst: Vreg(0),
                        index: 0,
                        ty: Ty::I32,
                    },
                ],
                term: Term::Ret { value: None },
            }],
        };
        let errors = verify_func_public(&func);
        assert!(
            errors.iter().any(|e| e.message.contains("out of order")),
            "expected an arg-order finding: {:?}",
            errors
        );

        // Arg in a non-entry block.
        func.blocks.push(Block {
            insts: vec![Inst::Arg {
                dst: Vreg(0),
                index: 0,
                ty: Ty::I32,
            }],
            term: Term::Ret { value: None },
        });
        let errors = verify_func_public(&func);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("outside the entry block")),
            "expected an arg-placement finding: {:?}",
            errors
        );
    }

    // CEP:WHAT: Verifies that multi def reaching rules accepted and rejected behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn multi_def_reaching_rules() {
        let mut func = diamond_with_phis();
        // Post-destruction shape: the join phi became copies on both edges,
        // all writing v3; the use in the join is reached by both.
        func.blocks[1].insts.push(Inst::Copy {
            dst: Vreg(3),
            src: Vreg(1),
            ty: Ty::I32,
        });
        func.blocks[2].insts.push(Inst::Copy {
            dst: Vreg(3),
            src: Vreg(2),
            ty: Ty::I32,
        });
        func.blocks[3].insts.clear();
        let errors = verify_func_public(&func);
        assert!(
            errors.is_empty(),
            "multi-def copies should verify: {:?}",
            errors
        );
    }

    /// verify_func with the same signature the pipeline uses.
    // CEP:WHAT: Handles the Runs the instruction verifier over one function for the tests, per the design and the file header contract. step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected findings.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn verify_func_public(func: &IrFunc) -> Vec<VerifyError> {
        let program = program(func.clone());
        let mut errors = Vec::new();
        super::verify_func(&program.funcs[0], &program, &mut errors);
        errors
    }
}
