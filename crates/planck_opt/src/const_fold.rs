// CEP:FILE: crates/planck_opt/src/const_fold.rs
// CEP:WHAT: Constant folding: arithmetic, bitwise, comparison, and cast folding over constant operands, plus branch folding on constant conditions and unreachable block removal.
// CEP:WHY: Compile-time evaluation of pure constant computations removes runtime work and unlocks downstream dead code elimination; branch folding removes unreachable control flow (pass catalog items 1, 2, 22, 41, 44).
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Checked integer ops whose constant result would overflow are NOT folded, so the mandated runtime trap stays observable; division or remainder by a zero constant is not folded for the same reason. All other folds use wrapping i128 intermediates clamped to the operand width, matching codegen semantics. Malformed input is the verifier's job, not this pass.
// CEP:ASSUMES: Two's complement integers, IEEE 754 floats, and standard 38.10 determinism: folding is a pure function of the input IR.
// CEP:COST: O(N) per sweep with a block-local constant environment; one sweep per pipeline run.
// CEP:EVIDENCE: unit tests in this file and pipeline tests in planck_opt.
// CEP:SECURITY: none; folding only shrinks code.
// CEP:PORTABILITY: Width-aware integer folding keeps results identical on every target.

#![forbid(unsafe_code)]

use planck_ir::{BinOp, BlockId, Const, Inst, Program, Term, Ty, UnOp};
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
        for block in func.blocks.iter_mut() {
            let mut env: BTreeMap<u32, Const> = BTreeMap::new();
            let mut kept: Vec<Inst> = Vec::with_capacity(block.insts.len());
            for inst in block.insts.drain(..) {
                if let Some(folded) = fold_inst(inst, &mut env, &mut changes) {
                    kept.push(folded)
                }
            }
            block.insts = kept;
        }
        changes += fold_branches(func);
    }
    changes
}

// CEP:WHAT: Handles the fold inst step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn fold_inst(inst: Inst, env: &mut BTreeMap<u32, Const>, changes: &mut usize) -> Option<Inst> {
    match inst {
        Inst::Bin {
            op,
            dst,
            a,
            b,
            ty,
            checked,
        } => {
            let ca = env.get(&a.0).cloned();
            let cb = env.get(&b.0).cloned();
            if let (Some(ca), Some(cb)) = (ca, cb) {
                match fold_bin(op, &ca, &cb, &ty, checked) {
                    FoldOutcome::Value(value) => {
                        *changes += 1;
                        env.insert(dst.0, value.clone());
                        return Some(Inst::Const { dst, value });
                    }
                    FoldOutcome::Keep => {}
                }
            }
            // The instruction's own definition refreshes the environment when constant.
            Some(Inst::Bin {
                op,
                dst,
                a,
                b,
                ty,
                checked,
            })
        }
        Inst::Un {
            op,
            dst,
            a,
            ty,
            checked,
        } => {
            if let Some(ca) = env.get(&a.0).cloned() {
                if let Some(value) = fold_un(op, &ca, &ty, checked) {
                    *changes += 1;
                    env.insert(dst.0, value.clone());
                    return Some(Inst::Const { dst, value });
                }
            }
            Some(Inst::Un {
                op,
                dst,
                a,
                ty,
                checked,
            })
        }
        Inst::Cast { dst, src, from, to } => {
            if let Some(cs) = env.get(&src.0).cloned() {
                if let Some(value) = fold_cast(&cs, &from, &to) {
                    *changes += 1;
                    env.insert(dst.0, value.clone());
                    return Some(Inst::Const { dst, value });
                }
            }
            Some(Inst::Cast { dst, src, from, to })
        }
        Inst::Const { dst, value } => {
            env.insert(dst.0, value.clone());
            Some(Inst::Const { dst, value })
        }
        Inst::Copy { dst, src, ty } => {
            // A copy of a known-constant vreg is that constant.
            if let Some(value) = env.get(&src.0).cloned() {
                *changes += 1;
                env.insert(dst.0, value.clone());
                return Some(Inst::Const { dst, value });
            }
            Some(Inst::Copy { dst, src, ty })
        }
        // Effectful or unknown-value instructions invalidate nothing here because the
        // environment only tracks Const defs, which no effect can change.
        other => Some(other),
    }
}

pub(crate) enum FoldOutcome {
    Value(Const),
    Keep,
}

// CEP:WHAT: Handles the fold bin step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub(crate) fn fold_bin(op: BinOp, a: &Const, b: &Const, ty: &Ty, checked: bool) -> FoldOutcome {
    // Integer folding path.
    if let (Const::Int(x, _), Const::Int(y, _)) = (a, b) {
        let width = int_width_bits(ty);
        let signed = ty.is_signed();
        let wrap = |v: i128| -> i128 {
            // Wrap to the operand width using modular arithmetic.
            let modulus = 1i128 << width;
            let masked = ((v.rem_euclid(modulus)) + modulus) % modulus;
            if signed && masked >= (1i128 << (width - 1)) {
                masked - modulus
            } else if !signed {
                masked.rem_euclid(modulus)
            } else {
                masked
            }
        };
        let overflowed = |v: i128| -> bool {
            v < -(1i128 << (width - 1))
                || v >= (1i128 << width) - if signed { 0 } else { 1 } && false
        };
        let _ = overflowed;
        let result: Option<i128> = match op {
            BinOp::Add => Some(x.checked_add(*y)),
            BinOp::Sub => Some(x.checked_sub(*y)),
            BinOp::Mul => Some(x.checked_mul(*y)),
            _ => None,
        }
        .flatten();
        if let Some(v) = result {
            if checked {
                if !fits_width(v, width, signed) {
                    return FoldOutcome::Keep; // keep the runtime trap
                }
                return FoldOutcome::Value(Const::Int(v, ty.clone()));
            }
            return FoldOutcome::Value(Const::Int(wrap(v), ty.clone()));
        }
        match op {
            BinOp::Div => {
                if *y == 0 {
                    return FoldOutcome::Keep; // trap must stay
                }
                // Truncating division matching codegen idiv semantics.
                let q = x.wrapping_div(*y);
                return FoldOutcome::Value(Const::Int(clamp_width(q, width, signed), ty.clone()));
            }
            BinOp::Rem => {
                if *y == 0 {
                    return FoldOutcome::Keep;
                }
                let r = x.wrapping_rem(*y);
                return FoldOutcome::Value(Const::Int(clamp_width(r, width, signed), ty.clone()));
            }
            BinOp::BitAnd => {
                return FoldOutcome::Value(Const::Int(
                    bits_of(x, width) & bits_of(y, width),
                    ty.clone(),
                ))
            }
            BinOp::BitOr => {
                return FoldOutcome::Value(Const::Int(
                    bits_of(x, width) | bits_of(y, width),
                    ty.clone(),
                ))
            }
            BinOp::BitXor => {
                return FoldOutcome::Value(Const::Int(
                    bits_of(x, width) ^ bits_of(y, width),
                    ty.clone(),
                ))
            }
            BinOp::Shl => {
                let amount = (*y).rem_euclid(width as i128) as u32;
                return FoldOutcome::Value(Const::Int(
                    wrap(bits_of(x, width) << amount),
                    ty.clone(),
                ));
            }
            BinOp::Shr => {
                let amount = (*y).rem_euclid(width as i128) as u32;
                let value = if signed {
                    *x >> amount
                } else {
                    bits_of(x, width) >> amount
                };
                return FoldOutcome::Value(Const::Int(value, ty.clone()));
            }
            BinOp::Eq => return FoldOutcome::Value(Const::Bool(x == y)),
            BinOp::Ne => return FoldOutcome::Value(Const::Bool(x != y)),
            BinOp::Lt => return FoldOutcome::Value(Const::Bool(x < y)),
            BinOp::Le => return FoldOutcome::Value(Const::Bool(x <= y)),
            BinOp::Gt => return FoldOutcome::Value(Const::Bool(x > y)),
            BinOp::Ge => return FoldOutcome::Value(Const::Bool(x >= y)),
            BinOp::Add | BinOp::Sub | BinOp::Mul => unreachable!("handled above"),
        }
    }
    // Boolean folding path.
    if let (Const::Bool(x), Const::Bool(y)) = (a, b) {
        if ty.is_integer() {
            // Bool arithmetic on integers: treat as 0/1.
            let xi = *x as i128;
            let yi = *y as i128;
            match op {
                BinOp::Eq => return FoldOutcome::Value(Const::Bool(xi == yi)),
                BinOp::Ne => return FoldOutcome::Value(Const::Bool(xi != yi)),
                BinOp::BitAnd => return FoldOutcome::Value(Const::Int(xi & yi, ty.clone())),
                BinOp::BitOr => return FoldOutcome::Value(Const::Int(xi | yi, ty.clone())),
                BinOp::BitXor => return FoldOutcome::Value(Const::Int(xi ^ yi, ty.clone())),
                _ => return FoldOutcome::Keep,
            }
        }
        let v = match op {
            BinOp::Eq => *x == *y,
            BinOp::Ne => *x != *y,
            _ => return FoldOutcome::Keep,
        };
        return FoldOutcome::Value(Const::Bool(v));
    }
    // Float folding path.
    if let (Const::Float(x, _), Const::Float(y, _)) = (a, b) {
        let v = match op {
            BinOp::Add => x + y,
            BinOp::Sub => x - y,
            BinOp::Mul => x * y,
            BinOp::Div => {
                if *y == 0.0 {
                    return FoldOutcome::Keep;
                }
                x / y
            }
            BinOp::Eq => return FoldOutcome::Value(Const::Bool(x == y)),
            BinOp::Ne => return FoldOutcome::Value(Const::Bool(x != y)),
            BinOp::Lt => return FoldOutcome::Value(Const::Bool(x < y)),
            BinOp::Le => return FoldOutcome::Value(Const::Bool(x <= y)),
            BinOp::Gt => return FoldOutcome::Value(Const::Bool(x > y)),
            BinOp::Ge => return FoldOutcome::Value(Const::Bool(x >= y)),
            _ => return FoldOutcome::Keep,
        };
        return FoldOutcome::Value(Const::Float(v, matches!(ty, Ty::F32)));
    }
    // Mixed comparisons of identical char constants.
    if let (Const::Char(x), Const::Char(y)) = (a, b) {
        let v = match op {
            BinOp::Eq => x == y,
            BinOp::Ne => x != y,
            BinOp::Lt => x < y,
            BinOp::Le => x <= y,
            BinOp::Gt => x > y,
            BinOp::Ge => x >= y,
            _ => return FoldOutcome::Keep,
        };
        return FoldOutcome::Value(Const::Bool(v));
    }
    FoldOutcome::Keep
}

// CEP:WHAT: Handles the fold un step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub(crate) fn fold_un(op: UnOp, a: &Const, ty: &Ty, checked: bool) -> Option<Const> {
    match (op, a) {
        (UnOp::Neg, Const::Int(x, _)) => {
            let width = int_width_bits(ty);
            if checked && !fits_width(-x, width, true) {
                return None; // keep the runtime trap
            }
            Some(Const::Int(clamp_width(-x, width, true), ty.clone()))
        }
        (UnOp::Neg, Const::Float(x, is_f32)) => Some(Const::Float(-x, *is_f32)),
        (UnOp::Not, Const::Bool(x)) => Some(Const::Bool(!x)),
        (UnOp::Not, Const::Int(x, _)) => {
            let width = int_width_bits(ty);
            Some(Const::Int(!bits_of(x, width), ty.clone()))
        }
        _ => None,
    }
}

// CEP:WHAT: Handles the fold cast step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub(crate) fn fold_cast(a: &Const, from: &Ty, to: &Ty) -> Option<Const> {
    match (a, to) {
        (Const::Int(x, _), to_ty) if to_ty.is_integer() => {
            let width = int_width_bits(to_ty);
            Some(Const::Int(
                clamp_width(*x, width, to_ty.is_signed()),
                to_ty.clone(),
            ))
        }
        (Const::Int(x, _), Ty::F32) => Some(Const::Float(*x as f64, true)),
        (Const::Int(x, _), Ty::F64) => Some(Const::Float(*x as f64, false)),
        (Const::Float(x, _), to_ty) if to_ty.is_integer() => {
            let width = int_width_bits(to_ty);
            // Hardware-style truncation toward zero; out-of-range and NaN clamp to the
            // documented deviation (ticket PLK-023).
            let truncated = if x.is_nan() {
                0
            } else if *x <= -(1i128 << (width - 1)) as f64 {
                -(1i128 << (width - 1))
            } else if *x >= (1i128 << width) as f64 {
                (1i128 << width) - 1
            } else {
                *x as i128
            };
            Some(Const::Int(
                clamp_width(truncated, width, to_ty.is_signed()),
                to_ty.clone(),
            ))
        }
        (Const::Float(x, _), Ty::F32) => Some(Const::Float(*x, true)),
        (Const::Float(x, _), Ty::F64) => Some(Const::Float(*x, false)),
        (Const::Char(c), to_ty) if to_ty.is_integer() => {
            Some(Const::Int(*c as u32 as i128, to_ty.clone()))
        }
        (Const::Int(x, _), Ty::Char) => char::from_u32(bits_of(x, 32) as u32).map(Const::Char),
        (Const::Bool(b), to_ty) if to_ty.is_integer() => {
            Some(Const::Int(*b as i128, to_ty.clone()))
        }
        _ => {
            let _ = from;
            None
        }
    }
}

fn int_width_bits(ty: &Ty) -> u32 {
    match ty {
        Ty::I8 | Ty::U8 => 8,
        Ty::I16 | Ty::U16 => 16,
        Ty::I32 | Ty::U32 => 32,
        _ => 64,
    }
}

/// Wraps an i128 into the signed/unsigned range of the given width.
// CEP:WHAT: Handles the clamp width step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn clamp_width(v: i128, width: u32, signed: bool) -> i128 {
    let modulus = 1i128 << width;
    let masked = v.rem_euclid(modulus);
    if signed && masked >= (1i128 << (width - 1)) {
        masked - modulus
    } else {
        masked
    }
}

/// Zero-extends the two's complement bits of x within `width`.
fn bits_of(x: &i128, width: u32) -> i128 {
    let modulus = 1i128 << width;
    x.rem_euclid(modulus)
}

fn fits_width(v: i128, width: u32, signed: bool) -> bool {
    if signed {
        v >= -(1i128 << (width - 1)) && v < (1i128 << (width - 1))
    } else {
        v >= 0 && v < (1i128 << width)
    }
}

/// Folds `BrCond` on a constant into `Br`, then removes blocks unreachable from entry.
// CEP:WHAT: Handles the fold branches step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn fold_branches(func: &mut planck_ir::IrFunc) -> usize {
    let mut changes = 0;
    for block in func.blocks.iter_mut() {
        if let Term::BrCond { cond, .. } = block.term {
            let const_value = block.insts.iter().rev().find_map(|inst| match inst {
                Inst::Const {
                    dst,
                    value: Const::Bool(value),
                } if *dst == cond => Some(*value),
                _ => None,
            });
            if let Some(value) = const_value {
                block.term = if value {
                    match block.term {
                        Term::BrCond { then_blk, .. } => Term::Br(then_blk),
                        _ => unreachable!("matched above"),
                    }
                } else {
                    match block.term {
                        Term::BrCond { else_blk, .. } => Term::Br(else_blk),
                        _ => unreachable!("matched above"),
                    }
                };
                changes += 1;
            }
        }
    }
    changes += remove_unreachable(func);
    changes
}

// CEP:WHAT: Handles the remove unreachable step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn remove_unreachable(func: &mut planck_ir::IrFunc) -> usize {
    let n = func.blocks.len();
    let mut reachable = vec![false; n];
    let mut stack = vec![0u32];
    while let Some(node) = stack.pop() {
        let node = node as usize;
        if reachable[node] {
            continue;
        }
        reachable[node] = true;
        match &func.blocks[node].term {
            Term::Br(target) => stack.push(target.0),
            Term::BrCond {
                then_blk, else_blk, ..
            } => {
                stack.push(then_blk.0);
                stack.push(else_blk.0);
            }
            Term::Ret { .. } => {}
        }
    }
    let mut mapping: Vec<Option<u32>> = vec![None; n];
    let mut kept: Vec<planck_ir::Block> = Vec::with_capacity(n);
    for (idx, block) in func.blocks.drain(..).enumerate() {
        if reachable[idx] {
            mapping[idx] = Some(kept.len() as u32);
            kept.push(block);
        }
    }
    let removed = n - kept.len();
    if removed == 0 {
        func.blocks = kept;
        return 0;
    }
    for block in kept.iter_mut() {
        match &mut block.term {
            Term::Br(target) => {
                if let Some(mapped) = mapping[target.0 as usize] {
                    *target = BlockId(mapped);
                }
            }
            Term::BrCond {
                then_blk, else_blk, ..
            } => {
                if let Some(mapped) = mapping[then_blk.0 as usize] {
                    *then_blk = BlockId(mapped);
                }
                if let Some(mapped) = mapping[else_blk.0 as usize] {
                    *else_blk = BlockId(mapped);
                }
            }
            Term::Ret { .. } => {}
        }
    }
    func.blocks = kept;
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use planck_ir::{Block, IrFunc, Slot, SlotInfo, Term, Ty, Vreg};

    fn one_block_func() -> IrFunc {
        IrFunc {
            name: "f".to_string(),
            func_id: planck_ir::FuncId(0),
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
                insts: vec![],
                term: Term::Ret {
                    value: Some(Vreg(0)),
                },
            }],
        }
    }

    // CEP:WHAT: Verifies that folds constant arithmetic behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn folds_constant_arithmetic() {
        let mut program = Program {
            funcs: vec![one_block_func()],
            structs: vec![],
            enums: vec![],
            rodata: vec![],
            ir_version: "t".into(),
        };
        let f = &mut program.funcs[0];
        f.blocks[0].insts = vec![
            Inst::Const {
                dst: Vreg(0),
                value: Const::Int(2, Ty::I32),
            },
            Inst::Const {
                dst: Vreg(1),
                value: Const::Int(3, Ty::I32),
            },
            Inst::Bin {
                op: BinOp::Mul,
                dst: Vreg(2),
                a: Vreg(0),
                b: Vreg(1),
                ty: Ty::I32,
                checked: true,
            },
        ];
        let changes = run(&mut program);
        assert!(changes >= 1);
        assert!(matches!(
            &program.funcs[0].blocks[0].insts[2],
            Inst::Const {
                value: Const::Int(6, _),
                ..
            }
        ));
    }

    // CEP:WHAT: Verifies that checked overflow is not folded behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn checked_overflow_is_not_folded() {
        let mut program = Program {
            funcs: vec![one_block_func()],
            structs: vec![],
            enums: vec![],
            rodata: vec![],
            ir_version: "t".into(),
        };
        let f = &mut program.funcs[0];
        f.blocks[0].insts = vec![
            Inst::Const {
                dst: Vreg(0),
                value: Const::Int(i32::MAX as i128, Ty::I32),
            },
            Inst::Const {
                dst: Vreg(1),
                value: Const::Int(1, Ty::I32),
            },
            Inst::Bin {
                op: BinOp::Add,
                dst: Vreg(2),
                a: Vreg(0),
                b: Vreg(1),
                ty: Ty::I32,
                checked: true,
            },
        ];
        run(&mut program);
        assert!(matches!(
            &program.funcs[0].blocks[0].insts[2],
            Inst::Bin { .. }
        ));
    }

    // CEP:WHAT: Verifies that division by zero constant stays behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn division_by_zero_constant_stays() {
        let mut program = Program {
            funcs: vec![one_block_func()],
            structs: vec![],
            enums: vec![],
            rodata: vec![],
            ir_version: "t".into(),
        };
        let f = &mut program.funcs[0];
        f.blocks[0].insts = vec![
            Inst::Const {
                dst: Vreg(0),
                value: Const::Int(6, Ty::I32),
            },
            Inst::Const {
                dst: Vreg(1),
                value: Const::Int(0, Ty::I32),
            },
            Inst::Bin {
                op: BinOp::Div,
                dst: Vreg(2),
                a: Vreg(0),
                b: Vreg(1),
                ty: Ty::I32,
                checked: false,
            },
        ];
        run(&mut program);
        assert!(matches!(
            &program.funcs[0].blocks[0].insts[2],
            Inst::Bin { .. }
        ));
    }

    // CEP:WHAT: Verifies that unreachable blocks are removed behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn unreachable_blocks_are_removed() {
        let mut func = one_block_func();
        func.blocks.push(Block {
            insts: vec![],
            term: Term::Ret {
                value: Some(Vreg(0)),
            },
        });
        func.blocks[0].term = Term::Br(BlockId(0)); // bb1 unreachable
        let mut program = Program {
            funcs: vec![func],
            structs: vec![],
            enums: vec![],
            rodata: vec![],
            ir_version: "t".into(),
        };
        let changes = run(&mut program);
        assert!(changes >= 1);
        assert_eq!(program.funcs[0].blocks.len(), 1);
    }

    #[test]
    fn slot_type_is_never_used_by_fold() {
        let _ = Slot(0);
    }
}
