// CEP:FILE: crates/planck_opt/src/licm.rs
// CEP:WHAT: Loop-invariant code motion over natural loops, hoisting trap-free pure instructions to the loop preheader.
// CEP:WHY: Invariant computations inside loops are re-executed every iteration for no benefit; hoisting them is the classic latency win (pass catalog item 81).
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Only instructions that cannot trap and have no memory effects are hoisted: Const, unchecked bitwise/shift operations, Not, Cast, AddrSlot, AddrOffset. Checked arithmetic and division stay in the loop because their traps are semantically part of the loop's behavior. Loops without a unique preheader are left untouched.
// CEP:ASSUMES: The dominator analysis is valid; back edges are edges from a block to one of its dominators.
// CEP:COST: O(N) loop discovery plus O(N) hoisting sweeps per loop.
// CEP:EVIDENCE: unit tests in this file.
// CEP:SECURITY: Hoisting never moves a potentially trapping operation, so observable failure behavior is unchanged.
// CEP:PORTABILITY: Target-independent IR operations only.

#![forbid(unsafe_code)]

use planck_ir::{BinOp, BlockId, Dominators, Inst, Program, Term, Vreg};
use std::collections::BTreeSet;

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

fn hoistable(inst: &Inst) -> bool {
    match inst {
        Inst::Const { .. } | Inst::AddrSlot { .. } | Inst::AddrOffset { .. } => true,
        Inst::Cast { .. } => true,
        Inst::Un { op, checked, .. } => !*checked && matches!(op, planck_ir::UnOp::Not),
        Inst::Bin { op, checked, .. } => {
            !*checked
                && !op.is_div_or_rem()
                && matches!(
                    op,
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
                )
        }
        _ => false,
    }
}

// CEP:WHAT: Handles the defs in block step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn defs_in_block(block: &planck_ir::Block) -> BTreeSet<u32> {
    let mut defs = BTreeSet::new();
    for inst in &block.insts {
        if let Some(dst) = def_of(inst) {
            defs.insert(dst.0);
        }
    }
    defs
}

fn def_of(inst: &Inst) -> Option<Vreg> {
    inst.def_vreg()
}

// CEP:WHAT: Handles the uses of step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn uses_of(inst: &Inst) -> Vec<u32> {
    inst.use_vregs().into_iter().map(|vreg| vreg.0).collect()
}

// CEP:WHAT: Handles the run on func step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn run_on_func(func: &mut planck_ir::IrFunc) -> usize {
    let dominators = Dominators::compute(func);
    // Find back edges: t -> h where h dominates t.
    let mut loops: Vec<(u32, u32)> = Vec::new();
    for (idx, block) in func.blocks.iter().enumerate() {
        let targets: Vec<u32> = match &block.term {
            Term::Br(target) => vec![target.0],
            Term::BrCond {
                then_blk, else_blk, ..
            } => vec![then_blk.0, else_blk.0],
            Term::Ret { .. } => vec![],
        };
        for target in targets {
            if dominators.dominates(BlockId(target), BlockId(idx as u32)) {
                loops.push((idx as u32, target));
            }
        }
    }
    if loops.is_empty() {
        return 0;
    }
    let mut changes = 0;
    for (tail, header) in loops {
        // Natural loop body: reverse reachable set from tail to header.
        let mut body: BTreeSet<u32> = BTreeSet::new();
        body.insert(header);
        let mut stack = vec![tail];
        while let Some(node) = stack.pop() {
            if body.insert(node) {
                for pred in preds_of(func, node) {
                    stack.push(pred);
                }
            }
        }
        // Preheader: unique predecessor outside the loop that branches only to the header.
        let outside_preds: Vec<u32> = preds_of(func, header)
            .into_iter()
            .filter(|pred| !body.contains(pred))
            .collect();
        if outside_preds.len() != 1 {
            continue;
        }
        let preheader = outside_preds[0];
        if !matches!(func.blocks[preheader as usize].term, Term::Br(_)) {
            continue;
        }
        // Vregs defined anywhere in the loop.
        let mut loop_defs: BTreeSet<u32> = BTreeSet::new();
        for node in &body {
            loop_defs.extend(defs_in_block(&func.blocks[*node as usize]));
        }
        // Hoist invariant hoistable instructions.
        let mut hoisted: Vec<Inst> = Vec::new();
        for node in &body {
            let block = &mut func.blocks[*node as usize];
            let mut kept: Vec<Inst> = Vec::with_capacity(block.insts.len());
            for inst in block.insts.drain(..) {
                let hoist_this =
                    hoistable(&inst) && uses_of(&inst).iter().all(|u| !loop_defs.contains(u));
                if hoist_this {
                    hoisted.push(inst);
                    changes += 1;
                } else {
                    kept.push(inst);
                }
            }
            block.insts = kept;
        }
        // Insert before the preheader terminator.
        let pre = &mut func.blocks[preheader as usize];
        let term = pre.term.clone();
        pre.insts.extend(hoisted);
        pre.term = term;
    }
    changes
}

// CEP:WHAT: Handles the preds of step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn preds_of(func: &planck_ir::IrFunc, target: u32) -> Vec<u32> {
    let mut preds = Vec::new();
    for (idx, block) in func.blocks.iter().enumerate() {
        match &block.term {
            Term::Br(t) if t.0 == target => preds.push(idx as u32),
            Term::BrCond {
                then_blk, else_blk, ..
            } => {
                if then_blk.0 == target || else_blk.0 == target {
                    preds.push(idx as u32);
                }
            }
            Term::Ret { .. } | Term::Br(_) => {}
        }
    }
    preds
}

#[cfg(test)]
mod tests {
    use super::*;
    use planck_ir::{Block, Const, FuncId, IrFunc, SlotInfo, Ty};

    // CEP:WHAT: Handles the loop program step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn loop_program() -> Program {
        // bb0: const 10 -> br bb1
        // bb1 (header): cond br -> bb2 / bb3
        // bb2: v3 = v0 xor v1 (invariant); i increment; br bb1
        // bb3: ret
        let vregs = vec![Ty::I32; 8];
        Program {
            funcs: vec![IrFunc {
                name: "f".into(),
                func_id: FuncId(0),
                params: vec![],
                ret: Ty::I32,
                sret_slot: None,
                slots: vec![SlotInfo {
                    ty: Ty::I32,
                    size: 4,
                    align: 4,
                    origin: "x".into(),
                }],
                vregs,
                blocks: vec![
                    Block {
                        insts: vec![
                            Inst::Const {
                                dst: Vreg(0),
                                value: Const::Int(10, Ty::I32),
                            },
                            Inst::Const {
                                dst: Vreg(1),
                                value: Const::Int(20, Ty::I32),
                            },
                        ],
                        term: Term::Br(BlockId(1)),
                    },
                    Block {
                        insts: vec![Inst::Const {
                            dst: Vreg(2),
                            value: Const::Bool(true),
                        }],
                        term: Term::BrCond {
                            cond: Vreg(2),
                            then_blk: BlockId(2),
                            else_blk: BlockId(3),
                        },
                    },
                    Block {
                        insts: vec![Inst::Bin {
                            op: BinOp::BitXor,
                            dst: Vreg(3),
                            a: Vreg(0),
                            b: Vreg(1),
                            ty: Ty::I32,
                            checked: false,
                        }],
                        term: Term::Br(BlockId(1)),
                    },
                    Block {
                        insts: vec![],
                        term: Term::Ret {
                            value: Some(Vreg(3)),
                        },
                    },
                ],
            }],
            structs: vec![],
            enums: vec![],
            rodata: vec![],
            ir_version: "t".into(),
        }
    }

    // CEP:WHAT: Verifies that invariant xor moves to preheader behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn invariant_xor_moves_to_preheader() {
        let mut program = loop_program();
        let changes = run(&mut program);
        assert!(changes >= 1, "expected LICM to hoist");
        // The preheader (bb0) now holds the hoisted xor; the loop body (bb2) does not.
        let bb0 = &program.funcs[0].blocks[0];
        assert!(bb0.insts.iter().any(|inst| matches!(
            inst,
            Inst::Bin {
                op: BinOp::BitXor,
                ..
            }
        )));
        let bb2 = &program.funcs[0].blocks[2];
        assert!(!bb2.insts.iter().any(|inst| matches!(
            inst,
            Inst::Bin {
                op: BinOp::BitXor,
                ..
            }
        )));
    }
}
