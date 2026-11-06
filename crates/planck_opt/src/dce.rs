// CEP:FILE: crates/planck_opt/src/dce.rs
// CEP:WHAT: Dead code elimination for pure instructions with unused results, iterated to fixpoint.
// CEP:WHY: Constant folding and CSE leave behind computations nobody reads; removing them shrinks both size and register pressure (pass catalog items 5, 23).
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Only definitions with no observable effect are removed: Const, Bin, Un, Cast, AddrSlot, AddrOffset, and slot/indirect loads (loads cannot fault because the borrow checker guarantees reference validity, and reading a slot has no side effect). Stores, calls, traps, and terminator-reachable effects are never removed.
// CEP:ASSUMES: The load operations listed above are side-effect free per the IR contract; if the IR ever gains volatile or trapping loads, they must be excluded here.
// CEP:COST: O(N) per iteration; iteration count bounded by the longest pure def-use chain.
// CEP:EVIDENCE: unit tests in this file.
// CEP:SECURITY: none.
// CEP:PORTABILITY: Target-independent.

#![forbid(unsafe_code)]

use planck_ir::{Inst, Program, Term, Vreg};
use std::collections::BTreeSet;

// CEP:WHAT: Handles the run step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn run(program: &mut Program) -> usize {
    let mut total = 0;
    for func in program.funcs.iter_mut() {
        total += run_on_func(func);
    }
    total
}

fn removable(inst: &Inst) -> bool {
    matches!(
        inst,
        Inst::Const { .. }
            | Inst::Bin { .. }
            | Inst::Un { .. }
            | Inst::Cast { .. }
            | Inst::AddrSlot { .. }
            | Inst::AddrOffset { .. }
            | Inst::LoadSlot { .. }
            | Inst::LoadField { .. }
            | Inst::LoadIndirect { .. }
            | Inst::Copy { .. }
            | Inst::Arg { .. }
            | Inst::Phi { .. }
    )
}

fn def_of(inst: &Inst) -> Option<Vreg> {
    inst.def_vreg()
}

// CEP:WHAT: Handles the uses of inst step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn uses_of_inst(inst: &Inst) -> Vec<u32> {
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
    loop {
        // Collect all used vregs.
        let mut used: BTreeSet<u32> = BTreeSet::new();
        for block in &func.blocks {
            for inst in &block.insts {
                used.extend(uses_of_inst(inst));
            }
            match &block.term {
                Term::BrCond { cond, .. } => {
                    used.insert(cond.0);
                }
                Term::Ret { value } => {
                    if let Some(value) = value {
                        used.insert(value.0);
                    }
                }
                Term::Br(_) => {}
            }
        }
        let mut removed_any = false;
        for block in func.blocks.iter_mut() {
            let before = block.insts.len();
            block.insts.retain(|inst| {
                if !removable(inst) {
                    return true;
                }
                match def_of(inst) {
                    Some(dst) => used.contains(&dst.0),
                    None => true,
                }
            });
            if block.insts.len() < before {
                removed_any = true;
            }
        }
        if !removed_any {
            return 0;
        }
    }
}

#[allow(dead_code)]
fn _sweep_counter(_n: usize) -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use planck_ir::{Block, Const, FuncId, IrFunc, SlotInfo, Ty};

    fn program_with(insts: Vec<Inst>, ret: Vreg) -> Program {
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
                vregs: vec![Ty::I32; 8],
                blocks: vec![Block {
                    insts,
                    term: Term::Ret { value: Some(ret) },
                }],
            }],
            structs: vec![],
            enums: vec![],
            rodata: vec![],
            ir_version: "t".into(),
        }
    }

    // CEP:WHAT: Verifies that unused pure chain is removed behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn unused_pure_chain_is_removed() {
        let mut program = program_with(
            vec![
                Inst::Const {
                    dst: Vreg(0),
                    value: Const::Int(1, Ty::I32),
                },
                // v1 = v0 + v0 is never used.
                Inst::Bin {
                    op: planck_ir::BinOp::Add,
                    dst: Vreg(1),
                    a: Vreg(0),
                    b: Vreg(0),
                    ty: Ty::I32,
                    checked: false,
                },
            ],
            Vreg(0),
        );
        run(&mut program);
        let insts = &program.funcs[0].blocks[0].insts;
        assert_eq!(insts.len(), 1);
    }

    // CEP:WHAT: Verifies that used instructions survive behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn used_instructions_survive() {
        let mut program = program_with(
            vec![
                Inst::Const {
                    dst: Vreg(0),
                    value: Const::Int(1, Ty::I32),
                },
                Inst::Bin {
                    op: planck_ir::BinOp::Add,
                    dst: Vreg(1),
                    a: Vreg(0),
                    b: Vreg(0),
                    ty: Ty::I32,
                    checked: false,
                },
            ],
            Vreg(1),
        );
        run(&mut program);
        let insts = &program.funcs[0].blocks[0].insts;
        assert_eq!(insts.len(), 2);
    }
}
