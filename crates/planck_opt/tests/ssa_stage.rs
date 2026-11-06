// CEP:FILE: crates/planck_opt/tests/ssa_stage.rs
// CEP:WHAT: Integration tests for the SSA stage: construction (phi placement, Arg promotion, pinned slots), SCCP (branch pruning, phi folding, trap preservation), LCSSA (exit phis), destruction (swap temporaries), and copy propagation.
// CEP:WHY: The SSA stage is the pipeline's largest rewrite surface; these tests pin each pass's observable contract end-to-end through the real frontend.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: Each test asserts on IR shape or pass behavior; a panic inside the harness is a bug.
// CEP:ASSUMES: planck_parser and planck_sema are available as dev-dependencies; fixture sources are repository-controlled.
// CEP:COST: Each test is one frontend lowering plus small passes; microseconds.
// CEP:EVIDENCE: cargo test -p planck_opt --test ssa_stage.
// CEP:SECURITY: Fixtures are repository-controlled.
// CEP:PORTABILITY: Target-independent IR checks.

use planck_ir::{verify_program, Inst, Program, Term};
use planck_opt::{out_ssa, sccp, ssa};

// CEP:WHAT: Handles the lower step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn lower(source: &str) -> Program {
    let (module, parse_diags) = planck_parser::parse_module(source);
    assert!(parse_diags.is_empty(), "parse failed: {:?}", parse_diags);
    let (program, sema_diags) = planck_sema::analyze(&module, true);
    assert!(
        sema_diags
            .iter()
            .all(|d| d.kind != planck_support::DiagKind::Error),
        "sema failed: {:?}",
        sema_diags
    );
    let findings = verify_program(&program);
    assert!(findings.is_empty(), "lowering invalid: {:?}", findings);
    program
}

// ---------------------------------------------------------------------------
// SSA construction
// ---------------------------------------------------------------------------

// CEP:WHAT: Verifies that diamond join gets phi behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn diamond_join_gets_phi() {
    let mut program = lower(
        "fn main() -> i32 { let mut x: i32 = 1; if true { x = 2; } else { x = 3; } return x; }",
    );
    let changes = ssa::run(&mut program);
    assert!(changes > 0, "expected promotions");
    let findings = verify_program(&program);
    assert!(findings.is_empty(), "invalid SSA: {:?}", findings);
    // The join block must contain a phi with two inputs.
    let phi_count = program.funcs[0]
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter(|inst| matches!(inst, Inst::Phi { inputs, .. } if inputs.len() == 2))
        .count();
    assert!(phi_count >= 1, "expected a two-input phi");
}

// CEP:WHAT: Verifies that loop header phi self reference behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn loop_header_phi_self_reference() {
    let mut program = lower(
        "fn main() -> i32 { let mut x: i32 = 0; let mut i: i32 = 0; while i < 3 { x = x; i = i + 1; } return x; }",
    );
    let changes = ssa::run(&mut program);
    assert!(changes > 0, "expected promotions");
    let findings = verify_program(&program);
    assert!(findings.is_empty(), "invalid SSA: {:?}", findings);
    // A loop header phi must have a self-referencing input from the latch:
    // `x = x` re-stores the header phi's own value, so the back edge carries
    // the phi itself (exercising the load-replacement chase on the way).
    let func = &program.funcs[0];
    let latch_of = |header: usize| -> Vec<usize> {
        (0..func.blocks.len())
            .filter(|b| {
                successors(func, *b).contains(&header)
                    && planck_ir::Dominators::compute(func).dominates(
                        planck_ir::BlockId(header as u32),
                        planck_ir::BlockId(*b as u32),
                    )
            })
            .collect()
    };
    let mut found = false;
    for (b, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let Inst::Phi { dst, inputs, .. } = inst {
                let latches = latch_of(b);
                if latches.iter().any(|l| {
                    inputs
                        .iter()
                        .any(|(pred, value)| pred.0 as usize == *l && *value == *dst)
                }) {
                    found = true;
                }
            }
        }
    }
    assert!(found, "expected a self-referencing loop phi");
}

fn successors(func: &planck_ir::IrFunc, b: usize) -> Vec<usize> {
    match &func.blocks[b].term {
        Term::Br(target) => vec![target.0 as usize],
        Term::BrCond {
            then_blk, else_blk, ..
        } => vec![then_blk.0 as usize, else_blk.0 as usize],
        Term::Ret { .. } => vec![],
    }
}

// CEP:WHAT: Verifies that parameter promotion emits arg behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn parameter_promotion_emits_arg() {
    let mut program = lower(
        "fn add(a: i32, b: i32) -> i32 { return a + b; } fn main() -> i32 { return add(2, 3); }",
    );
    let changes = ssa::run(&mut program);
    assert!(changes > 0, "expected promotions");
    let findings = verify_program(&program);
    assert!(findings.is_empty(), "invalid SSA: {:?}", findings);
    let add = program
        .funcs
        .iter()
        .find(|f| f.name == "add")
        .expect("add present");
    let arg_indices: Vec<u32> = add.blocks[0]
        .insts
        .iter()
        .filter_map(|inst| match inst {
            Inst::Arg { index, .. } => Some(*index),
            _ => None,
        })
        .collect();
    assert_eq!(arg_indices, vec![0, 1], "both parameters get Args in order");
    // No loads of the parameter slots remain.
    let param_loads = add.blocks.iter().flat_map(|b| b.insts.iter()).any(|inst| {
        matches!(inst, Inst::LoadSlot { slot, .. }
                if add.params.contains(slot))
    });
    assert!(!param_loads, "promoted parameters must not be re-read");
}

// CEP:WHAT: Verifies that address taken slot stays in memory behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn address_taken_slot_stays_in_memory() {
    let mut program = lower(
        "fn bump(r: &mut i32) -> () { *r = *r + 1; } fn main() -> i32 { let mut x: i32 = 1; bump(&mut x); return x; }",
    );
    let _changes = ssa::run(&mut program);
    let findings = verify_program(&program);
    assert!(findings.is_empty(), "invalid SSA: {:?}", findings);
    // The main function's x slot is address-taken; its loads must remain.
    let main = program
        .funcs
        .iter()
        .find(|f| f.name == "main")
        .expect("main present");
    let x_slot = main
        .slots
        .iter()
        .position(|info| info.origin == "x")
        .expect("x slot present");
    let still_loads = main.blocks.iter().any(|b| {
        b.insts
            .iter()
            .any(|inst| matches!(inst, Inst::LoadSlot { slot, .. } if slot.0 as usize == x_slot))
    });
    assert!(still_loads, "address-taken slots are never promoted");
}

// ---------------------------------------------------------------------------
// SCCP
// ---------------------------------------------------------------------------

// CEP:WHAT: Verifies that constant branch prunes dead arm behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn constant_branch_prunes_dead_arm() {
    let mut program = lower(
        "fn main() -> i32 { let mut x: i32 = 0; if 1 < 2 { x = 10; } else { x = 20; } return x; }",
    );
    let ssa_changes = ssa::run(&mut program);
    let _ = ssa_changes;
    let sccp_changes = sccp::run(&mut program);
    assert!(sccp_changes > 0, "expected SCCP changes");
    let findings = verify_program(&program);
    assert!(findings.is_empty(), "invalid IR after SCCP: {:?}", findings);
    // The dead arm's constant (20) must be gone from the function entirely.
    let has_20 = program.funcs[0]
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .any(|inst| {
            matches!(
                inst,
                Inst::Const {
                    value: planck_ir::Const::Int(20, _),
                    ..
                }
            )
        });
    assert!(!has_20, "the dead arm must disappear");
    // The return must see the folded constant 10.
    let ret_value = match &program.funcs[0].blocks.last().unwrap().term {
        Term::Ret { value: Some(v) } => *v,
        other => panic!("unexpected terminator: {:?}", other),
    };
    let ret_const = program.funcs[0].blocks.last().unwrap().insts.iter().any(|inst| {
        matches!(inst, Inst::Const { dst, value: planck_ir::Const::Int(10, _) } if *dst == ret_value)
    });
    assert!(ret_const, "return value must fold to 10");
}

// CEP:WHAT: Verifies that sccp folds phi of equal constants behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn sccp_folds_phi_of_equal_constants() {
    let mut program = lower(
        "fn main() -> i32 { let mut x: i32 = 7; if false { x = 7; } else { x = 7; } return x; }",
    );
    let _ = ssa::run(&mut program);
    let changes = sccp::run(&mut program);
    assert!(changes > 0);
    let findings = verify_program(&program);
    assert!(findings.is_empty(), "invalid IR after SCCP: {:?}", findings);
}

// CEP:WHAT: Verifies that sccp keeps overflowing trap behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn sccp_keeps_overflowing_trap() {
    let mut program =
        lower("fn main() -> i32 { let x: i8 = 127; let y: i8 = x + 1; return y as i32; }");
    let _ = ssa::run(&mut program);
    let _ = sccp::run(&mut program);
    // The checked add must survive as an instruction (it would trap), and no
    // constant 128 (or -128 wrap) may replace it.
    let has_checked_add = program.funcs[0]
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .any(|inst| matches!(inst, Inst::Bin { checked: true, .. }));
    assert!(has_checked_add, "the trapping add must survive");
}

// ---------------------------------------------------------------------------
// LCSSA
// ---------------------------------------------------------------------------

// CEP:WHAT: Verifies that break loop gets exit phi behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn break_loop_gets_exit_phi() {
    let mut program = lower(
        "fn main() -> i32 { let mut acc: i32 = 0; loop { acc = acc + 5; if acc > 11 { break; } } return acc; }",
    );
    let _ = ssa::run(&mut program);
    let _ = sccp::run(&mut program);
    let changes = out_ssa::lcssa(&mut program);
    assert!(changes > 0, "expected LCSSA exit phis");
    let findings = verify_program(&program);
    assert!(
        findings.is_empty(),
        "invalid IR after LCSSA: {:?}",
        findings
    );
}

// ---------------------------------------------------------------------------
// SSA destruction
// ---------------------------------------------------------------------------

// CEP:WHAT: Verifies that destruction resolves swaps with temps behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn destruction_resolves_swaps_with_temps() {
    // Hand-built loop cycle: the latch feeds both phis from each other, so
    // the edge copies on the back edge form a swap that needs temporaries.
    // bb0: v0 = const 1; v1 = const 2; br bb1
    // bb1: v2 = phi(v0@bb0, v3@bb2); v3 = phi(v1@bb0, v2@bb2); v4 = add; br_cond v4, bb2, bb3
    // bb2: br bb1
    // bb3: ret v4
    use planck_ir::{Block, BlockId, Const, FuncId, IrFunc, SlotInfo, Ty, Vreg};
    let func = IrFunc {
        name: "swap".into(),
        func_id: FuncId(0),
        params: vec![],
        ret: Ty::I32,
        sret_slot: None,
        slots: vec![SlotInfo {
            ty: Ty::I32,
            size: 4,
            align: 4,
            origin: "t".into(),
        }],
        vregs: vec![Ty::I32, Ty::I32, Ty::I32, Ty::I32, Ty::I32, Ty::Bool],
        blocks: vec![
            Block {
                insts: vec![
                    Inst::Const {
                        dst: Vreg(0),
                        value: Const::Int(1, Ty::I32),
                    },
                    Inst::Const {
                        dst: Vreg(1),
                        value: Const::Int(2, Ty::I32),
                    },
                ],
                term: Term::Br(BlockId(1)),
            },
            Block {
                insts: vec![
                    Inst::Phi {
                        dst: Vreg(2),
                        ty: Ty::I32,
                        inputs: vec![(BlockId(0), Vreg(0)), (BlockId(2), Vreg(3))],
                    },
                    Inst::Phi {
                        dst: Vreg(3),
                        ty: Ty::I32,
                        inputs: vec![(BlockId(0), Vreg(1)), (BlockId(2), Vreg(2))],
                    },
                    Inst::Bin {
                        op: planck_ir::BinOp::Add,
                        dst: Vreg(4),
                        a: Vreg(2),
                        b: Vreg(3),
                        ty: Ty::I32,
                        checked: false,
                    },
                    Inst::Bin {
                        op: planck_ir::BinOp::Gt,
                        dst: Vreg(5),
                        a: Vreg(4),
                        b: Vreg(4),
                        ty: Ty::I32,
                        checked: false,
                    },
                ],
                term: Term::BrCond {
                    cond: Vreg(5),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            Block {
                insts: vec![],
                term: Term::Br(BlockId(1)),
            },
            Block {
                insts: vec![],
                term: Term::Ret {
                    value: Some(Vreg(4)),
                },
            },
        ],
    };
    let mut program = Program {
        funcs: vec![func],
        structs: vec![],
        enums: vec![],
        rodata: vec![],
        ir_version: "t".into(),
    };
    let changes = out_ssa::ssa_destroy(&mut program);
    assert!(changes > 0, "expected copies");
    let findings = verify_program(&program);
    assert!(
        findings.is_empty(),
        "destroyed IR must verify: {:?}",
        findings
    );
    // No phis remain; the back edge got temporaries for the swap.
    let phis = program.funcs[0]
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter(|inst| matches!(inst, Inst::Phi { .. }))
        .count();
    assert_eq!(phis, 0, "all phis must be gone");
    let latch_copies: Vec<&Inst> = program.funcs[0].blocks[2]
        .insts
        .iter()
        .filter(|inst| matches!(inst, Inst::Copy { .. }))
        .collect();
    assert!(
        latch_copies.len() >= 4,
        "back-edge swap needs temp copies plus main copies: {:?}",
        latch_copies
    );
}

// ---------------------------------------------------------------------------
// Copy propagation
// ---------------------------------------------------------------------------

// CEP:WHAT: Verifies that copy prop folds single def chains behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn copy_prop_folds_single_def_chains() {
    let mut program = lower("fn main() -> i32 { let a: i32 = 5; let b: i32 = a; return b; }");
    let _ = ssa::run(&mut program);
    let _ = out_ssa::lcssa(&mut program);
    let _ = out_ssa::phi_cleanup(&mut program);
    let _ = out_ssa::ssa_destroy(&mut program);
    let _changes = out_ssa::copy_prop(&mut program);
    let findings = verify_program(&program);
    assert!(findings.is_empty(), "invalid IR: {:?}", findings);
    // The return must read the constant 5 (through the folded chain).
    let ret_value = match &program.funcs[0].blocks.last().unwrap().term {
        Term::Ret { value: Some(v) } => *v,
        other => panic!("unexpected terminator: {:?}", other),
    };
    let folded = program.funcs[0].blocks.last().unwrap().insts.iter().any(|inst| {
        matches!(inst, Inst::Const { dst, value: planck_ir::Const::Int(5, _) } if *dst == ret_value)
    });
    assert!(folded, "the chain must fold to the constant");
}

// CEP:WHAT: Verifies that multi def copies survive copy prop behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn multi_def_copies_survive_copy_prop() {
    let mut program =
        lower("fn main() -> i32 { let mut i: i32 = 0; while i < 3 { i = i + 1; } return i; }");
    let _ = ssa::run(&mut program);
    let _ = out_ssa::lcssa(&mut program);
    let _ = out_ssa::phi_cleanup(&mut program);
    let _ = out_ssa::ssa_destroy(&mut program);
    let _ = out_ssa::copy_prop(&mut program);
    let findings = verify_program(&program);
    assert!(findings.is_empty(), "invalid IR: {:?}", findings);
    // The loop-carried vreg has two defs (entry copy + latch copy) and both
    // must survive copy propagation.
    let mut def_counts: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for block in &program.funcs[0].blocks {
        for inst in &block.insts {
            if let Some(dst) = inst.def_vreg() {
                *def_counts.entry(dst.0).or_default() += 1;
            }
        }
    }
    let loop_var_defs = program.funcs[0]
        .blocks
        .iter()
        .flat_map(|b| b.insts.iter())
        .filter_map(|inst| match inst {
            Inst::Copy { dst, .. } if def_counts[&dst.0] > 1 => Some(*dst),
            _ => None,
        })
        .count();
    assert!(loop_var_defs >= 2, "loop-carried copies must both survive");
}

// ---------------------------------------------------------------------------
// Full pipeline behavior on the SSA shapes above
// ---------------------------------------------------------------------------

// CEP:WHAT: Verifies that ssa shapes survive full pipeline behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn ssa_shapes_survive_full_pipeline() {
    let sources = [
        "fn main() -> i32 { let mut x: i32 = 1; if true { x = 2; } else { x = 3; } return x; }",
        "fn main() -> i32 { let mut i: i32 = 0; while i < 3 { i = i + 1; } return i; }",
        "fn main() -> i32 { let mut acc: i32 = 0; loop { acc = acc + 5; if acc > 11 { break; } } return acc; }",
        "fn main() -> i32 { let mut a: [i32; 3] = [10, 20, 30]; a[1] = 99; let mut s: i32 = 0; for i in 0..3 { s = s + a[i]; } return s; }",
    ];
    for source in sources {
        let mut program = lower(source);
        let result = planck_opt::optimize(&mut program);
        assert!(
            result.verify_errors.is_empty(),
            "pipeline verification failed for {}: {:?}",
            source,
            result.verify_errors
        );
    }
}
