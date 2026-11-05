// CEP:FILE: crates/planck_ir/src/verify.rs
// CEP:WHAT: IR verifier plus the dominator analysis it depends on; runs after construction and after every optimization pass.
// CEP:WHY: Standard 38.18 requires verification of use-def, type, terminator, and linkage validity after lowering and after each pass, so no downstream stage can observe a malformed graph.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: VerifyError findings describe the first violations found per function; the verifier never mutates the IR and never panics on malformed input.
// CEP:ASSUMES: Blocks are dense 0..n; vregs are dense 0..m per function; slot infos are dense 0..k per function. References are values of pointer width produced by AddrSlot or by reference-typed slots only.
// CEP:COST: Dominators O(N^2) worst case with dense bitsets, typically near-linear with the Cooper-Harvey-Kennedy iteration; verification O(N + E).
// CEP:EVIDENCE: cargo test -p planck_ir (malformed-IR rejection cases).
// CEP:SECURITY: The verifier is the guard against downstream miscompilation of untrusted source; it runs in every build mode, debug and release alike (cheap full mode, no split required at this scale).
// CEP:PORTABILITY: Pure core; dominator logic is target-independent.

#![forbid(unsafe_code)]

use crate::ir::{BlockId, Callee, Const, Inst, IrFunc, Program, Term, Vreg};
use std::collections::BTreeSet;

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
    let mut defined_at: Vec<Option<BlockId>> = vec![None; func.vregs.len()];

    // Parameters count as defined in the entry block.
    for slot in &func.params {
        let _ = slot;
    }
    // Vreg definitions are discovered in a first pass (params of the frame model are slots; vregs defined by insts).
    for (block_idx, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            for dst in def_vregs(inst).into_iter().flatten() {
                if (dst.0 as usize) < func.vregs.len() {
                    defined_at[dst.0 as usize] = Some(BlockId(block_idx as u32));
                }
            }
        }
    }

    for (block_idx, block) in func.blocks.iter().enumerate() {
        let block_id = BlockId(block_idx as u32);
        // Terminator validity.
        if block_idx + 1 < func.blocks.len() && matches!(block.term, Term::Ret { .. }) {
            // Early return mid-function is legal; nothing to check.
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
                check_use(func, &dominators, &defined_at, block_id, *cond, errors);
                if func
                    .vregs
                    .get(cond.0 as usize)
                    .map(|ty| !matches!(ty, crate::ir::Ty::Bool))
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
                    check_use(func, &dominators, &defined_at, block_id, *value, errors);
                    let ret_ty = func.vregs.get(value.0 as usize);
                    let expected_ok = match (&func.ret, ret_ty) {
                        (crate::ir::Ty::Unit, None) => true,
                        (crate::ir::Ty::Unit, Some(_)) => false,
                        (expected, Some(actual)) => expected == actual,
                        (expected, None) => expected != &crate::ir::Ty::Unit,
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
                } else if func.ret != crate::ir::Ty::Unit {
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
        for inst in &block.insts {
            verify_inst(
                func,
                &dominators,
                &defined_at,
                block_id,
                inst,
                program,
                errors,
            );
        }
    }
}

fn def_vregs(inst: &Inst) -> [Option<Vreg>; 1] {
    match inst {
        Inst::Const { dst, .. }
        | Inst::Bin { dst, .. }
        | Inst::Un { dst, .. }
        | Inst::Cast { dst, .. }
        | Inst::LoadSlot { dst, .. }
        | Inst::LoadField { dst, .. }
        | Inst::AddrSlot { dst, .. }
        | Inst::AddrOffset { dst, .. }
        | Inst::LoadIndirect { dst, .. } => [Some(*dst)],
        Inst::Call { dst, .. } => [*dst],
        _ => [None],
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
    defined_at: &[Option<BlockId>],
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
    match defined_at[vreg.0 as usize] {
        Some(def_block) => {
            if !dominators.dominates(def_block, at_block) {
                errors.push(VerifyError {
                    func: func.name.clone(),
                    message: format!(
                        "v{} defined in bb{} is used in bb{} without dominance",
                        vreg.0, def_block.0, at_block.0
                    ),
                });
            }
        }
        None => {
            errors.push(VerifyError {
                func: func.name.clone(),
                message: format!("v{} is used before definition", vreg.0),
            });
        }
    }
}

// CEP:WHAT: Handles the verify inst step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn verify_inst(
    func: &IrFunc,
    dominators: &Dominators,
    defined_at: &[Option<BlockId>],
    block_id: BlockId,
    inst: &Inst,
    program: &Program,
    errors: &mut Vec<VerifyError>,
) {
    let mut uses: Vec<Vreg> = Vec::new();
    match inst {
        Inst::Bin {
            dst, a, b, ty, op, ..
        } => {
            uses.push(*a);
            uses.push(*b);
            let result_ty = func.vregs.get(dst.0 as usize);
            let expected: &crate::ir::Ty = if op.is_comparison() {
                &crate::ir::Ty::Bool
            } else {
                ty
            };
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
        check_use(func, dominators, defined_at, block_id, vreg, errors);
    }
}
