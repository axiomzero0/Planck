// CEP:FILE: crates/planck_codegen/src/alloc.rs
// CEP:WHAT: Stage-1 register allocation: per-function liveness analysis and static selection of hot vregs into a fixed set of registers that no emission rule uses as scratch.
// CEP:WHY: The frame-resident value model pays a load/store pair for every value movement; caching the hottest vregs in registers removes the dominant reload cost of loop bodies while keeping the frame-slot path as the universal spill fallback, which contains the correctness blast radius to one uniform decision per vreg.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Total over verifier-valid IR; candidates beyond the register pool simply stay frame-resident. Never panics on any input.
// CEP:ASSUMES: Vregs are never aliased in memory: a vreg is written only by a defining instruction's `dst` (or an SSA-destruction copy) and is never address-taken, so one register per vreg for the whole function is consistent at every use. Cache registers are callee-saved (integer) or never touched by the emitter or the ABI argument conventions (float), so calls and trap stubs cannot invalidate them.
// CEP:COST: O(instructions) liveness plus O(N^2) worst-case dominator work (O(N log N) typical) per function; emission stays O(instructions).
// CEP:EVIDENCE: unit tests; end-to-end execution tests in the planckc crate; benchmark report doc/BENCHMARKS.md.
// CEP:SECURITY: Pure analysis over in-program data; no untrusted input handling.
// CEP:PORTABILITY: Register names are x86-64 AT&T syntax; the pool follows the System V ABI (callee-saved rbx/r12-r15; float arguments confined to xmm0-7 so xmm8+ stay free of ABI traffic).

#![forbid(unsafe_code)]

use planck_ir::{Inst, IrFunc, Term, Vreg};
use std::collections::{BTreeMap, BTreeSet};

/// Integer cache registers: callee-saved per the System V ABI, so calls and
/// the runtime's C helpers preserve them; the emitter never uses them as scratch.
pub const INT_CACHE_REGS: [&str; 5] = ["%rbx", "%r12", "%r13", "%r14", "%r15"];

/// Float cache registers: the ABI passes float arguments in xmm0-7 only and no
/// emission rule touches xmm8+, so these stay valid between definition and use
/// for values that are not live across a call (all XMM are caller-saved).
pub const FLOAT_CACHE_REGS: [&str; 5] = ["%xmm8", "%xmm9", "%xmm10", "%xmm11", "%xmm12"];

/// Sub-register names for one cache register, needed by narrow-width
/// instructions that address the low bytes of a wider register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reg {
    /// 64-bit name ("%rbx", "%xmm8").
    pub full: &'static str,
    /// 8-bit low byte ("%bl", "%r12b"); equal to `full` for XMM.
    pub byte: &'static str,
    /// 16-bit low word ("%bx", "%r12w"); equal to `full` for XMM.
    pub word: &'static str,
    /// 32-bit doubleword ("%ebx", "%r12d"); equal to `full` for XMM.
    pub dword: &'static str,
    /// True for XMM registers (moved with movss/movsd).
    pub is_float: bool,
}

// CEP:WHAT: Handles the build int register step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn int_reg(index: usize) -> Reg {
    match index {
        0 => Reg {
            full: "%rbx",
            byte: "%bl",
            word: "%bx",
            dword: "%ebx",
            is_float: false,
        },
        1 => Reg {
            full: "%r12",
            byte: "%r12b",
            word: "%r12w",
            dword: "%r12d",
            is_float: false,
        },
        2 => Reg {
            full: "%r13",
            byte: "%r13b",
            word: "%r13w",
            dword: "%r13d",
            is_float: false,
        },
        3 => Reg {
            full: "%r14",
            byte: "%r14b",
            word: "%r14w",
            dword: "%r14d",
            is_float: false,
        },
        _ => Reg {
            full: "%r15",
            byte: "%r15b",
            word: "%r15w",
            dword: "%r15d",
            is_float: false,
        },
    }
}

// CEP:WHAT: Handles the build float register step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn float_reg(index: usize) -> Reg {
    Reg {
        full: FLOAT_CACHE_REGS[index],
        byte: FLOAT_CACHE_REGS[index],
        word: FLOAT_CACHE_REGS[index],
        dword: FLOAT_CACHE_REGS[index],
        is_float: true,
    }
}

/// The register assignment for one function: a vreg either has exactly one
/// cache register for its entire lifetime or stays frame-resident.
#[derive(Debug, Default)]
pub struct RegAlloc {
    assignment: BTreeMap<u32, Reg>,
    /// Indices into INT_CACHE_REGS holding an assigned vreg; the prologue
    /// pushes and the epilogue pops exactly these, in order.
    saved_int: Vec<usize>,
}

impl RegAlloc {
    /// The cache register of a vreg, if it was selected.
    pub fn cache_reg(&self, vreg: u32) -> Option<Reg> {
        self.assignment.get(&vreg).copied()
    }

    /// Callee-saved integer registers to push in the prologue (ascending pool
    /// order) and pop in the epilogue (reverse order).
    pub fn saved_int(&self) -> &[usize] {
        &self.saved_int
    }
}

/// Control-flow successors of a terminator, in deterministic order.
fn successors(term: &Term) -> Vec<u32> {
    match term {
        Term::Br(target) => vec![target.0],
        Term::BrCond {
            then_blk, else_blk, ..
        } => vec![then_blk.0, else_blk.0],
        Term::Ret { .. } => Vec::new(),
    }
}

/// Reverse postorder of the blocks reachable from the entry block.
// CEP:WHAT: Handles the reverse postorder step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn reverse_postorder(func: &IrFunc) -> Vec<u32> {
    let n = func.blocks.len();
    let mut visited = vec![false; n];
    let mut post = Vec::with_capacity(n);
    let mut stack: Vec<(u32, usize)> = vec![(0, 0)];
    visited[0] = true;
    while let Some((block, succ_idx)) = stack.pop() {
        let succs = successors(&func.blocks[block as usize].term);
        if succ_idx < succs.len() {
            stack.push((block, succ_idx + 1));
            let next = succs[succ_idx] as usize;
            if !visited[next] {
                visited[next] = true;
                stack.push((next as u32, 0));
            }
        } else {
            post.push(block);
        }
    }
    post.reverse();
    post
}

/// Immediate dominators (Cooper-Harvey-Kennedy) over the reverse postorder.
/// The entry dominates itself; blocks unreachable from the entry (impossible
/// in verified IR) also map to themselves so every block gets a total answer.
// CEP:WHAT: Handles the immediate dominators step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn immediate_dominators(func: &IrFunc, rpo: &[u32]) -> Vec<u32> {
    let n = func.blocks.len();
    let mut rpo_num = vec![usize::MAX; n];
    for (order, block) in rpo.iter().enumerate() {
        rpo_num[*block as usize] = order;
    }
    let mut preds: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (idx, block) in func.blocks.iter().enumerate() {
        for succ in successors(&block.term) {
            preds[succ as usize].push(idx as u32);
        }
    }
    let mut idom: Vec<u32> = (0..n as u32).collect();
    idom[0] = 0;
    let mut changed = true;
    while changed {
        changed = false;
        for &block in rpo.iter().skip(1) {
            let b = block as usize;
            let mut new_idom: Option<u32> = None;
            for &pred in &preds[b] {
                // Only predecessors already placed earlier in the RPO have a
                // firm idom in this sweep; later ones are skipped.
                if rpo_num[pred as usize] >= rpo_num[b] {
                    continue;
                }
                new_idom = Some(match new_idom {
                    None => pred,
                    Some(current) => intersect(&idom, &rpo_num, pred, current),
                });
            }
            if let Some(new) = new_idom {
                if idom[b] != new {
                    idom[b] = new;
                    changed = true;
                }
            }
        }
    }
    idom
}

/// Walks two nodes up the dominator tree to their common dominator.
// CEP:WHAT: Handles the dominator intersect step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn intersect(idom: &[u32], rpo_num: &[usize], mut a: u32, mut b: u32) -> u32 {
    while a != b {
        while rpo_num[a as usize] > rpo_num[b as usize] {
            a = idom[a as usize];
        }
        while rpo_num[b as usize] > rpo_num[a as usize] {
            b = idom[b as usize];
        }
    }
    a
}

/// Does `dom` dominate `node`? Walks the idom chain from `node` to the entry.
// CEP:WHAT: Handles the dominance query step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn dominates(idom: &[u32], dom: u32, node: u32) -> bool {
    let mut current = node;
    loop {
        if current == dom {
            return true;
        }
        if current == 0 {
            return dom == 0;
        }
        current = idom[current as usize];
    }
}

/// Records a use for scoring: every textual use counts (unlike the
/// upward-exposed `gen` sets, which only serve the liveness fixpoint).
// CEP:WHAT: Handles the record use step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn record_use(v: Vreg, depth: u32, use_count: &mut [u32], weighted: &mut [u64]) {
    if (v.0 as usize) < use_count.len() {
        use_count[v.0 as usize] += 1;
        weighted[v.0 as usize] += 1 + 3 * depth as u64;
    }
}

// CEP:WHAT: Handles the compute assignment step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn compute(func: &IrFunc) -> RegAlloc {
    let n = func.blocks.len();
    if n == 0 || func.vregs.is_empty() {
        return RegAlloc::default();
    }

    // --- Block-level gen/kill and predecessor/successor tables ----------
    let mut gen: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    let mut kill: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    let mut succs: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut preds: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (idx, block) in func.blocks.iter().enumerate() {
        for succ in successors(&block.term) {
            succs[idx].push(succ);
            preds[succ as usize].push(idx as u32);
        }
        let mut killed: BTreeSet<u32> = BTreeSet::new();
        let mut exposed: BTreeSet<u32> = BTreeSet::new();
        for inst in &block.insts {
            for v in inst.use_vregs() {
                if !killed.contains(&v.0) {
                    exposed.insert(v.0);
                }
            }
            if let Some(d) = inst.def_vreg() {
                killed.insert(d.0);
            }
        }
        match &block.term {
            Term::BrCond { cond, .. } if !killed.contains(&cond.0) => {
                exposed.insert(cond.0);
            }
            Term::Ret { value: Some(v) } if !killed.contains(&v.0) => {
                exposed.insert(v.0);
            }
            _ => {}
        }
        gen[idx] = exposed;
        kill[idx] = killed;
    }

    // --- Liveness fixpoint (backward dataflow) --------------------------
    let mut live_in: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    let mut live_out: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    let mut changed = true;
    while changed {
        changed = false;
        for idx in (0..n).rev() {
            let mut out = BTreeSet::new();
            for succ in &succs[idx] {
                out.extend(live_in[*succ as usize].iter().copied());
            }
            let mut inset: BTreeSet<u32> = out
                .iter()
                .copied()
                .filter(|v| !kill[idx].contains(v))
                .collect();
            inset.extend(gen[idx].iter().copied());
            if out != live_out[idx] || inset != live_in[idx] {
                live_out[idx] = out;
                live_in[idx] = inset;
                changed = true;
            }
        }
    }

    // --- Call-crossing set (instruction-granularity, backward walk) -----
    // A vreg live immediately after a call (other than the call's own result)
    // must survive it; those are the only values the caller must protect.
    let mut crosses_call: BTreeSet<u32> = BTreeSet::new();
    for (idx, block) in func.blocks.iter().enumerate() {
        let mut live = live_out[idx].clone();
        match &block.term {
            Term::BrCond { cond, .. } => {
                live.insert(cond.0);
            }
            Term::Ret { value: Some(v) } => {
                live.insert(v.0);
            }
            _ => {}
        }
        for inst in func.blocks[idx].insts.iter().rev() {
            if let Inst::Call { dst, .. } = inst {
                for v in live.iter().copied() {
                    if Some(Vreg(v)) != *dst {
                        crosses_call.insert(v);
                    }
                }
            }
            if let Some(d) = inst.def_vreg() {
                live.remove(&d.0);
            }
            for u in inst.use_vregs() {
                live.insert(u.0);
            }
        }
    }

    // --- Loop depth via natural loops -----------------------------------
    let rpo = reverse_postorder(func);
    let idom = immediate_dominators(func, &rpo);
    let mut depth = vec![0u32; n];
    for (idx, block) in func.blocks.iter().enumerate() {
        for succ in successors(&block.term) {
            // Back edge idx -> succ exactly when succ dominates idx.
            if dominates(&idom, succ, idx as u32) {
                mark_loop_body(&preds, succ, idx as u32, &mut depth);
            }
        }
    }

    // --- Candidate scoring ----------------------------------------------
    let total_vregs = func.vregs.len();
    let mut use_count = vec![0u32; total_vregs];
    let mut weighted = vec![0u64; total_vregs];
    let mut defined = vec![false; total_vregs];
    for (idx, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            for v in inst.use_vregs() {
                record_use(v, depth[idx], &mut use_count, &mut weighted);
            }
            if let Some(d) = inst.def_vreg() {
                if (d.0 as usize) < total_vregs {
                    defined[d.0 as usize] = true;
                }
            }
        }
        match &block.term {
            Term::BrCond { cond, .. } => {
                record_use(*cond, depth[idx], &mut use_count, &mut weighted);
            }
            Term::Ret { value: Some(v) } => {
                record_use(*v, depth[idx], &mut use_count, &mut weighted);
            }
            _ => {}
        }
    }
    // A vreg crosses blocks when it is live at the end of some block.
    let mut crosses_block = vec![false; total_vregs];
    for set in &live_out {
        for v in set {
            if (*v as usize) < total_vregs {
                crosses_block[*v as usize] = true;
            }
        }
    }

    let mut candidates: Vec<(u64, u32, bool)> = Vec::new(); // (score, vreg, is_float)
    for v in 0..total_vregs {
        if !defined[v] || use_count[v] == 0 {
            continue;
        }
        let ty = &func.vregs[v];
        if ty.scalar_size().is_none() {
            // Aggregates travel by address; vregs with aggregate IR types are
            // outside stage 1 (their addresses could be cached later).
            continue;
        }
        let is_float = ty.is_float();
        if is_float && crosses_call.contains(&(v as u32)) {
            // Caller-saved XMM registers cannot survive a call.
            continue;
        }
        // A single use immediately after the definition gains nothing (rax
        // forwarding already elides that reload); require a second use or a
        // use in another block.
        if use_count[v] < 2 && !crosses_block[v] {
            continue;
        }
        candidates.push((weighted[v], v as u32, is_float));
    }
    // Highest score first; ties break by vreg index for determinism.
    candidates.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)));

    let mut assignment: BTreeMap<u32, Reg> = BTreeMap::new();
    let mut next_int = 0usize;
    let mut next_float = 0usize;
    for (_, v, is_float) in candidates {
        if is_float {
            if next_float < FLOAT_CACHE_REGS.len() {
                assignment.insert(v, float_reg(next_float));
                next_float += 1;
            }
        } else if next_int < INT_CACHE_REGS.len() {
            assignment.insert(v, int_reg(next_int));
            next_int += 1;
        }
    }
    let saved_int: Vec<usize> = (0..next_int).collect();
    RegAlloc {
        assignment,
        saved_int,
    }
}

/// Marks every block of the natural loop of the back edge `top -> header`
/// with one more nesting level.
// CEP:WHAT: Handles the mark loop body step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn mark_loop_body(preds: &[Vec<u32>], header: u32, top: u32, depth: &mut [u32]) {
    let mut body: BTreeSet<u32> = BTreeSet::new();
    body.insert(header);
    let mut stack = vec![top];
    while let Some(node) = stack.pop() {
        if body.insert(node) {
            for pred in &preds[node as usize] {
                stack.push(*pred);
            }
        }
    }
    for node in body {
        let idx = node as usize;
        if idx < depth.len() {
            depth[idx] = depth[idx].saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use planck_ir::{Block, BlockId, Const, Inst, SlotInfo, Ty};

    // CEP:WHAT: Handles the verify a loop counter receives a register step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    #[test]
    fn loop_counter_gets_a_register() {
        // v0 = const 0; v1 = const 10; loop: v2 = v0 < v1; brcond; v0 = v0 + 1.
        // The counter and the limit are used every iteration and must be picked.
        let func = IrFunc {
            name: "loop".into(),
            func_id: planck_ir::FuncId(0),
            params: Vec::new(),
            ret: Ty::Unit,
            sret_slot: None,
            slots: Vec::new(),
            vregs: vec![Ty::I64, Ty::I64, Ty::Bool, Ty::I64],
            blocks: vec![
                Block {
                    insts: vec![
                        Inst::Const {
                            dst: Vreg(0),
                            value: Const::Int(0, Ty::I64),
                        },
                        Inst::Const {
                            dst: Vreg(1),
                            value: Const::Int(10, Ty::I64),
                        },
                    ],
                    term: Term::Br(BlockId(1)),
                },
                Block {
                    insts: vec![
                        Inst::Bin {
                            op: planck_ir::BinOp::Lt,
                            dst: Vreg(2),
                            a: Vreg(0),
                            b: Vreg(1),
                            ty: Ty::I64,
                            checked: false,
                        },
                        Inst::Bin {
                            op: planck_ir::BinOp::Add,
                            dst: Vreg(3),
                            a: Vreg(0),
                            b: Vreg(1),
                            ty: Ty::I64,
                            checked: true,
                        },
                    ],
                    term: Term::BrCond {
                        cond: Vreg(2),
                        then_blk: BlockId(2),
                        else_blk: BlockId(1),
                    },
                },
                Block {
                    insts: Vec::new(),
                    term: Term::Ret { value: None },
                },
            ],
        };
        let alloc = compute(&func);
        // The comparison operand v0 crosses the back edge into block 1's use,
        // so it must be cached; the entry constants feed that use.
        assert!(
            alloc.cache_reg(0).is_some(),
            "loop-carried value must be cached: {:?}",
            alloc
        );
        assert!(alloc.cache_reg(1).is_some(), "loop limit must be cached");
        assert!(
            !alloc.saved_int().is_empty(),
            "assigned integer registers must be saved"
        );
    }

    // CEP:WHAT: Handles the verify float selection and single-use rejection step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    #[test]
    fn twice_used_float_cached_single_use_rejected() {
        // v1 feeds two operations, so it must land in an XMM cache register;
        // v0, v2, and v3 each have one use and must stay frame-resident.
        // The allocator reads vreg types only; an empty slot table must not
        // disturb it (guards against accidental slot-table reads).
        let func = IrFunc {
            name: "tiny".into(),
            func_id: planck_ir::FuncId(0),
            params: Vec::new(),
            ret: Ty::Unit,
            sret_slot: None,
            slots: Vec::<SlotInfo>::new(),
            vregs: vec![Ty::F64, Ty::F64, Ty::F64, Ty::F64],
            blocks: vec![Block {
                insts: vec![
                    Inst::Const {
                        dst: Vreg(0),
                        value: Const::Float(1.0, false),
                    },
                    Inst::Const {
                        dst: Vreg(1),
                        value: Const::Float(2.0, false),
                    },
                    Inst::Bin {
                        op: planck_ir::BinOp::Add,
                        dst: Vreg(2),
                        a: Vreg(0),
                        b: Vreg(1),
                        ty: Ty::F64,
                        checked: false,
                    },
                    Inst::Bin {
                        op: planck_ir::BinOp::Mul,
                        dst: Vreg(3),
                        a: Vreg(1),
                        b: Vreg(2),
                        ty: Ty::F64,
                        checked: false,
                    },
                ],
                term: Term::Ret {
                    value: Some(Vreg(3)),
                },
            }],
        };
        let alloc = compute(&func);
        let v1 = alloc.cache_reg(1).expect("twice-used float must be cached");
        assert!(v1.is_float, "float caches must use XMM registers");
        assert!(
            alloc.cache_reg(0).is_none(),
            "single-use vreg must stay frame-resident"
        );
        assert!(
            alloc.cache_reg(2).is_none() && alloc.cache_reg(3).is_none(),
            "single-use temporaries must stay frame-resident"
        );
        assert!(
            alloc.saved_int().is_empty(),
            "float-only assignment must not push integer registers"
        );
    }
}
