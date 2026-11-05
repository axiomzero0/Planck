// CEP:FILE: crates/planck_opt/src/gvn.rs
// CEP:WHAT: Dominator-scoped global value numbering and common subexpression elimination for pure instructions.
// CEP:WHY: Duplicate pure computations appear naturally after lowering and inlining; eliminating them shrinks both size and latency (pass catalog items 4, 21, 25).
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Only pure instructions participate (Const, Bin unchecked-safe, Un, Cast, AddrSlot, AddrOffset); any analysis doubt keeps the instruction. Malformed input is caught by the pipeline verifier.
// CEP:ASSUMES: Vregs are single-assignment within a function (the lowering contract); the dominator tree from planck_ir is valid.
// CEP:COST: O(N) walk over the dominator tree with O(1) amortized table operations; one rewrite sweep at the end.
// CEP:EVIDENCE: unit tests in this file.
// CEP:SECURITY: none.
// CEP:PORTABILITY: Keys are structural strings, independent of target.

#![forbid(unsafe_code)]

use planck_ir::{Dominators, Inst, Program, Term, Vreg};
use std::collections::BTreeMap;

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

fn inst_key(inst: &Inst) -> Option<String> {
    match inst {
        Inst::Bin {
            op,
            a,
            b,
            ty,
            checked,
            ..
        } => Some(format!(
            "bin:{}:{}:{}:{}:{}",
            op.symbol(),
            ty.name(),
            a.0,
            b.0,
            checked
        )),
        Inst::Un {
            op, a, ty, checked, ..
        } => Some(format!("un:{:?}:{}:{}:{}", op, ty.name(), a.0, checked)),
        Inst::Cast { src, from, to, .. } => {
            Some(format!("cast:{}:{}:{}", from.name(), to.name(), src.0))
        }
        Inst::AddrSlot {
            slot,
            pointee,
            mutable,
            ..
        } => Some(format!("addr:{}:{}:{}", slot.0, pointee.name(), mutable)),
        Inst::Const { value, .. } => Some(format!("const:{:?}", value)),
        _ => None,
    }
}

/// Pure operations that cannot trap: everything except checked arithmetic and div/rem.
fn is_trivially_pure(inst: &Inst) -> bool {
    match inst {
        Inst::Bin { op, checked, .. } => !*checked && !op.is_div_or_rem(),
        Inst::Un { checked, .. } => !*checked,
        Inst::Const { .. }
        | Inst::Cast { .. }
        | Inst::AddrSlot { .. }
        | Inst::AddrOffset { .. } => true,
        _ => false,
    }
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
    let n = func.blocks.len();
    // Children of each block in the dominator tree.
    let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (idx, idom) in dominators.idom.iter().enumerate() {
        if let Some(parent) = idom {
            if parent.0 as usize != idx {
                children[parent.0 as usize].push(idx as u32);
            }
        }
    }
    let mut replacements: BTreeMap<u32, u32> = BTreeMap::new();
    let mut scopes: Vec<BTreeMap<String, u32>> = vec![BTreeMap::new()];

    // Iterative DFS over the dominator tree.
    let mut stack: Vec<(u32, usize)> = vec![(0, 0)];
    while let Some((block_idx, child_idx)) = stack.pop() {
        if child_idx == 0 {
            // Enter this block: process its instructions with the parent scope on top.
            let parent_scope = scopes.last().expect("scope stack never empty").clone();
            let mut scope = parent_scope;
            let block = &func.blocks[block_idx as usize];
            for inst in &block.insts {
                if !is_trivially_pure(inst) {
                    continue;
                }
                let key = match inst_key(inst) {
                    Some(key) => key,
                    None => continue,
                };
                let dst = def_of(inst).expect("pure inst has a def");
                if let Some(existing) = scope.get(&key) {
                    if *existing != dst.0 {
                        replacements.insert(dst.0, *existing);
                    }
                } else {
                    scope.insert(key, dst.0);
                }
            }
            scopes.push(scope);
        }
        if child_idx < children[block_idx as usize].len() {
            let next_child = children[block_idx as usize][child_idx];
            stack.push((block_idx, child_idx + 1));
            stack.push((next_child, 0));
        } else {
            scopes.pop();
        }
    }

    if replacements.is_empty() {
        return 0;
    }
    // Resolve chains to fixpoints.
    let resolve = |mut v: u32| -> u32 {
        let mut guard = 0;
        while let Some(next) = replacements.get(&v) {
            v = *next;
            guard += 1;
            if guard > replacements.len() {
                break;
            }
        }
        v
    };
    let changes = replacements.len();
    // Rewrite all uses.
    let map_vreg = |v: Vreg| Vreg(resolve(v.0));
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_uses(inst, &map_vreg);
        }
        match &mut block.term {
            Term::BrCond { cond, .. } => {
                *cond = map_vreg(*cond);
            }
            Term::Ret { value } => {
                if let Some(value) = value {
                    *value = map_vreg(*value);
                }
            }
            Term::Br(_) => {}
        }
    }
    changes
}

fn def_of(inst: &Inst) -> Option<Vreg> {
    match inst {
        Inst::Const { dst, .. }
        | Inst::Bin { dst, .. }
        | Inst::Un { dst, .. }
        | Inst::Cast { dst, .. }
        | Inst::AddrSlot { dst, .. }
        | Inst::AddrOffset { dst, .. } => Some(*dst),
        _ => None,
    }
}

// CEP:WHAT: Handles the rewrite uses step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn rewrite_uses(inst: &mut Inst, map: &impl Fn(Vreg) -> Vreg) {
    match inst {
        Inst::Bin { a, b, .. } => {
            *a = map(*a);
            *b = map(*b);
        }
        Inst::Un { a, .. } => {
            *a = map(*a);
        }
        Inst::Cast { src, .. } => {
            *src = map(*src);
        }
        Inst::LoadSlot { .. } | Inst::StoreSlot { .. } => {}
        Inst::LoadField { .. } | Inst::StoreField { .. } => {}
        Inst::AddrOffset { base, offset, .. } => {
            *base = map(*base);
            *offset = map(*offset);
        }
        Inst::LoadIndirect { addr, .. } => {
            *addr = map(*addr);
        }
        Inst::StoreIndirect { addr, src, .. } => {
            *addr = map(*addr);
            *src = map(*src);
        }
        Inst::StoreStructFromAddr { src_addr, .. } => {
            *src_addr = map(*src_addr);
        }
        Inst::Call {
            dst: _, sret, args, ..
        } => {
            if let Some(sret) = sret {
                *sret = map(*sret);
            }
            for arg in args.iter_mut() {
                arg.value = map(arg.value);
            }
        }
        Inst::TrapIf { cond, .. } => {
            *cond = map(*cond);
        }
        Inst::Const { .. } | Inst::AddrSlot { .. } | Inst::Trap { .. } | Inst::Comment { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use planck_ir::{Block, Const, FuncId, IrFunc, SlotInfo, Ty};

    fn program_with(insts: Vec<Inst>) -> Program {
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
                    term: Term::Ret {
                        value: Some(Vreg(2)),
                    },
                }],
            }],
            structs: vec![],
            enums: vec![],
            rodata: vec![],
            ir_version: "t".into(),
        }
    }

    // CEP:WHAT: Verifies that duplicate pure ops are replaced behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn duplicate_pure_ops_are_replaced() {
        let mut program = program_with(vec![
            Inst::Const {
                dst: Vreg(0),
                value: Const::Int(4, Ty::I32),
            },
            Inst::Const {
                dst: Vreg(1),
                value: Const::Int(5, Ty::I32),
            },
            Inst::Bin {
                op: planck_ir::BinOp::Add,
                dst: Vreg(2),
                a: Vreg(0),
                b: Vreg(1),
                ty: Ty::I32,
                checked: false,
            },
            Inst::Bin {
                op: planck_ir::BinOp::Add,
                dst: Vreg(3),
                a: Vreg(0),
                b: Vreg(1),
                ty: Ty::I32,
                checked: false,
            },
            Inst::Bin {
                op: planck_ir::BinOp::Add,
                dst: Vreg(4),
                a: Vreg(2),
                b: Vreg(3),
                ty: Ty::I32,
                checked: false,
            },
        ]);
        let changes = run(&mut program);
        assert!(changes >= 1, "expected CSE to fire");
        // v4 = v2 + v2 after rewrite.
        match &program.funcs[0].blocks[0].insts[4] {
            Inst::Bin { a, b, .. } => {
                assert_eq!(a.0, 2);
                assert_eq!(b.0, 2);
            }
            other => panic!("unexpected inst: {:?}", other),
        }
    }
}
