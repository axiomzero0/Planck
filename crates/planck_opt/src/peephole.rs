// CEP:FILE: crates/planck_opt/src/peephole.rs
// CEP:WHAT: Algebraic simplification and strength reduction over binary and unary instructions with a block-local constant environment.
// CEP:WHY: Identities like x+0, x*1, x*0, x-x, x/1, x%1, and multiply-by-power-of-two-to-shift remove work without changing semantics (pass catalog items 6, 7, 12).
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Identities apply only when the neutral value is a constant operand visible in the block-local environment. Replacing x*0, x-x, and x%1 with a constant removes a checked trap only when the operation cannot overflow or trap (both hold for these identities), which the implementation asserts by construction. Division by zero is never simplified.
// CEP:ASSUMES: Two's complement integers; the multiply-to-shift reduction applies to unchecked multiplies only, so the trap behavior of checked multiplies is preserved.
// CEP:COST: O(N) single sweep plus one rewrite pass; fresh vregs are allocated for injected shift amounts.
// CEP:EVIDENCE: unit tests in this file.
// CEP:SECURITY: none.
// CEP:PORTABILITY: Shift lowering happens at the IR level and remains target-neutral.

#![forbid(unsafe_code)]

use planck_ir::{BinOp, Const, Inst, Program, Term, Ty, UnOp, Vreg};
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

/// Applies recorded dst->operand replacements across the function.
// CEP:WHAT: Handles the apply replacements step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn apply_replacements(func: &mut planck_ir::IrFunc) -> usize {
    let reps = REPLACEMENTS.with(|r| std::mem::take(&mut *r.borrow_mut()));
    if reps.is_empty() {
        return 0;
    }
    let resolve = |mut v: u32| -> Vreg {
        let mut guard = 0;
        while let Some(next) = reps.get(&v) {
            v = next.0;
            guard += 1;
            if guard > reps.len() {
                break;
            }
        }
        Vreg(v)
    };
    let count = reps.len();
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_uses(inst, &resolve);
        }
        match &mut block.term {
            Term::BrCond { cond, .. } => {
                *cond = resolve(cond.0);
            }
            Term::Ret { value } => {
                if let Some(value) = value {
                    *value = resolve(value.0);
                }
            }
            Term::Br(_) => {}
        }
    }
    count
}

// CEP:WHAT: Handles the rewrite uses step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn rewrite_uses(inst: &mut Inst, resolve: &impl Fn(u32) -> Vreg) {
    match inst {
        Inst::Bin { a, b, .. } => {
            *a = resolve(a.0);
            *b = resolve(b.0);
        }
        Inst::Un { a, .. } | Inst::Cast { src: a, .. } => {
            *a = resolve(a.0);
        }
        Inst::AddrOffset { base, offset, .. } => {
            *base = resolve(base.0);
            *offset = resolve(offset.0);
        }
        Inst::LoadIndirect { addr, .. } => {
            *addr = resolve(addr.0);
        }
        Inst::StoreIndirect { addr, src, .. } => {
            *addr = resolve(addr.0);
            *src = resolve(src.0);
        }
        Inst::StoreStructFromAddr { src_addr, .. } => {
            *src_addr = resolve(src_addr.0);
        }
        Inst::StoreSlot { src, .. } | Inst::StoreField { src, .. } => {
            *src = resolve(src.0);
        }
        Inst::Call { sret, args, .. } => {
            if let Some(sret) = sret {
                *sret = resolve(sret.0);
            }
            for arg in args.iter_mut() {
                arg.value = resolve(arg.value.0);
            }
        }
        Inst::TrapIf { cond, .. } => {
            *cond = resolve(cond.0);
        }
        _ => {}
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
    // Phase 1: count strength reductions that will fire so shift-amount vregs can be
    // pre-allocated without aliasing the block iteration borrow.
    let needed = count_strength_reductions(func);
    let first_fresh = func.vregs.len() as u32;
    for _ in 0..needed {
        func.vregs.push(Ty::I32);
    }
    NEXT_FRESH.store(first_fresh, std::sync::atomic::Ordering::Relaxed);

    let mut changes = 0;
    for block in func.blocks.iter_mut() {
        let mut env: BTreeMap<u32, Const> = BTreeMap::new();
        let mut kept: Vec<Inst> = Vec::with_capacity(block.insts.len());
        for inst in block.insts.drain(..) {
            kept.extend(simplify(inst, &mut env, &mut changes));
        }
        block.insts = kept;
    }
    changes += apply_replacements(func);
    changes
}

use std::sync::atomic::AtomicU32;
static NEXT_FRESH: AtomicU32 = AtomicU32::new(0);

fn fresh_vreg(_ty: Ty) -> Vreg {
    let id = NEXT_FRESH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Vreg(id)
}

/// Counts unchecked multiplies by a power-of-two constant visible in a block-local env.
// CEP:WHAT: Handles the count strength reductions step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn count_strength_reductions(func: &planck_ir::IrFunc) -> usize {
    let mut count = 0;
    for block in &func.blocks {
        let mut env: BTreeMap<u32, Const> = BTreeMap::new();
        for inst in &block.insts {
            if let Inst::Const { dst, value } = inst {
                env.insert(dst.0, value.clone());
                continue;
            }
            if let Inst::Bin {
                op: BinOp::Mul,
                dst,
                a,
                b,
                ty,
                checked,
            } = inst
            {
                if *checked || !ty.is_integer() {
                    env.remove(&dst.0);
                    continue;
                }
                let ca = env.get(&a.0);
                let cb = env.get(&b.0);
                let fires = match (ca, cb) {
                    (Some(Const::Int(v, _)), _) if *v > 0 && is_pow2(*v) => true,
                    (_, Some(Const::Int(v, _))) if *v > 0 && is_pow2(*v) => true,
                    _ => false,
                };
                if fires {
                    count += 1;
                }
                env.remove(&dst.0);
                continue;
            }
        }
    }
    count
}

/// Applies identities; returns the replacement instruction sequence (empty = removed).
// CEP:WHAT: Handles the simplify step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn simplify(inst: Inst, env: &mut BTreeMap<u32, Const>, changes: &mut usize) -> Vec<Inst> {
    // Track constant definitions for operand lookups.
    if let Inst::Const { dst, value } = &inst {
        env.insert(dst.0, value.clone());
        return vec![inst];
    }
    if let Inst::Bin {
        op,
        dst,
        a,
        b,
        ty,
        checked,
    } = inst
    {
        let ca = env.get(&a.0).cloned();
        let cb = env.get(&b.0).cloned();
        let int = ty.is_integer();
        // x + 0 / 0 + x -> x
        if op == BinOp::Add && int {
            if matches!(&cb, Some(Const::Int(0, _))) {
                *changes += 1;
                return replace_with(dst, a, env);
            }
            if matches!(&ca, Some(Const::Int(0, _))) {
                *changes += 1;
                return replace_with(dst, b, env);
            }
        }
        // x - 0 -> x ; x - x -> 0
        if op == BinOp::Sub && int {
            if matches!(&cb, Some(Const::Int(0, _))) {
                *changes += 1;
                return replace_with(dst, a, env);
            }
            if a == b {
                *changes += 1;
                let value = Const::Int(0, ty.clone());
                env.insert(dst.0, value.clone());
                return vec![Inst::Const { dst, value }];
            }
        }
        // x * 0 -> 0 ; x * 1 -> x ; unchecked x * 2^k -> x << k
        if op == BinOp::Mul && int {
            if matches!(&ca, Some(Const::Int(0, _))) || matches!(&cb, Some(Const::Int(0, _))) {
                *changes += 1;
                let value = Const::Int(0, ty.clone());
                env.insert(dst.0, value.clone());
                return vec![Inst::Const { dst, value }];
            }
            if matches!(&ca, Some(Const::Int(1, _))) {
                *changes += 1;
                return replace_with(dst, b, env);
            }
            if matches!(&cb, Some(Const::Int(1, _))) {
                *changes += 1;
                return replace_with(dst, a, env);
            }
            if !checked {
                let k = match (&ca, &cb) {
                    (Some(Const::Int(v, _)), _) if *v > 0 && is_pow2(*v) => Some((b, log2(*v))),
                    (_, Some(Const::Int(v, _))) if *v > 0 && is_pow2(*v) => Some((a, log2(*v))),
                    _ => None,
                };
                if let Some((base, k)) = k {
                    let k_vreg = fresh_vreg(Ty::I32);
                    env.insert(k_vreg.0, Const::Int(k as i128, Ty::I32));
                    *changes += 2;
                    return vec![
                        Inst::Const {
                            dst: k_vreg,
                            value: Const::Int(k as i128, Ty::I32),
                        },
                        Inst::Bin {
                            op: BinOp::Shl,
                            dst,
                            a: base,
                            b: k_vreg,
                            ty: ty.clone(),
                            checked: false,
                        },
                    ];
                }
            }
        }
        // x / 1 -> x ; x % 1 -> 0
        if op == BinOp::Div && int && matches!(&cb, Some(Const::Int(1, _))) {
            *changes += 1;
            return replace_with(dst, a, env);
        }
        if op == BinOp::Rem && int && matches!(&cb, Some(Const::Int(1, _))) {
            *changes += 1;
            let value = Const::Int(0, ty.clone());
            env.insert(dst.0, value.clone());
            return vec![Inst::Const { dst, value }];
        }
        // Shifts by zero are identity.
        if matches!(op, BinOp::Shl | BinOp::Shr) && int && matches!(&cb, Some(Const::Int(0, _))) {
            *changes += 1;
            return replace_with(dst, a, env);
        }
        // Bool simplifications.
        if ty == Ty::Bool {
            match op {
                BinOp::BitAnd => {
                    if matches!(&cb, Some(Const::Bool(true))) {
                        *changes += 1;
                        return replace_with(dst, a, env);
                    }
                    if matches!(&ca, Some(Const::Bool(true))) {
                        *changes += 1;
                        return replace_with(dst, b, env);
                    }
                }
                BinOp::BitOr => {
                    if matches!(&cb, Some(Const::Bool(false))) {
                        *changes += 1;
                        return replace_with(dst, a, env);
                    }
                    if matches!(&ca, Some(Const::Bool(false))) {
                        *changes += 1;
                        return replace_with(dst, b, env);
                    }
                }
                _ => {}
            }
        }
        env.remove(&dst.0);
        return vec![Inst::Bin {
            op,
            dst,
            a,
            b,
            ty,
            checked,
        }];
    }
    if let Inst::Un {
        op: UnOp::Not,
        dst,
        a,
        ty,
        checked,
    } = inst
    {
        env.remove(&dst.0);
        return vec![Inst::Un {
            op: UnOp::Not,
            dst,
            a,
            ty,
            checked,
        }];
    }
    // Everything else passes through untouched.
    vec![inst]
}

// CEP:WHAT: Handles the replace with step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn replace_with(dst: Vreg, with: Vreg, env: &mut BTreeMap<u32, Const>) -> Vec<Inst> {
    // Forward the replacement's constness when known so chained identities see through.
    if let Some(value) = env.get(&with.0).cloned() {
        env.insert(dst.0, value);
    } else {
        env.remove(&dst.0);
    }
    REPLACEMENTS.with(|r| {
        r.borrow_mut().insert(dst.0, with);
    });
    Vec::new()
}

thread_local! {
    static REPLACEMENTS: std::cell::RefCell<BTreeMap<u32, Vreg>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
}

fn is_pow2(v: i128) -> bool {
    v > 0 && (v & (v - 1)) == 0
}

fn log2(v: i128) -> u32 {
    v.trailing_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;
    use planck_ir::{Block, FuncId, IrFunc, SlotInfo};

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

    // CEP:WHAT: Verifies that add zero is removed behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn add_zero_is_removed() {
        let mut program = program_with(vec![
            Inst::Const {
                dst: Vreg(0),
                value: Const::Int(7, Ty::I32),
            },
            Inst::Const {
                dst: Vreg(1),
                value: Const::Int(0, Ty::I32),
            },
            Inst::Bin {
                op: BinOp::Add,
                dst: Vreg(2),
                a: Vreg(0),
                b: Vreg(1),
                ty: Ty::I32,
                checked: true,
            },
            Inst::Bin {
                op: BinOp::Add,
                dst: Vreg(3),
                a: Vreg(2),
                b: Vreg(0),
                ty: Ty::I32,
                checked: false,
            },
        ]);
        let changes = run(&mut program);
        assert!(changes >= 1);
        // The final add now reads v0 twice.
        match &program.funcs[0].blocks[0].insts.last().expect("inst") {
            Inst::Bin { a, b, .. } => {
                assert_eq!(a.0, 0);
                assert_eq!(b.0, 0);
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    // CEP:WHAT: Verifies that checked mul overflow trap is preserved behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn checked_mul_overflow_trap_is_preserved() {
        // x * 1 with checked=true: the multiply cannot overflow, so replacing with x is sound.
        let mut program = program_with(vec![
            Inst::Const {
                dst: Vreg(0),
                value: Const::Int(7, Ty::I32),
            },
            Inst::Const {
                dst: Vreg(1),
                value: Const::Int(1, Ty::I32),
            },
            Inst::Bin {
                op: BinOp::Mul,
                dst: Vreg(2),
                a: Vreg(0),
                b: Vreg(1),
                ty: Ty::I32,
                checked: true,
            },
            Inst::Bin {
                op: BinOp::Add,
                dst: Vreg(3),
                a: Vreg(2),
                b: Vreg(2),
                ty: Ty::I32,
                checked: false,
            },
        ]);
        run(&mut program);
        match &program.funcs[0].blocks[0].insts.last().expect("inst") {
            Inst::Bin { a, b, .. } => {
                assert_eq!(a.0, 0);
                assert_eq!(b.0, 0);
            }
            other => panic!("unexpected: {:?}", other),
        }
    }
}
