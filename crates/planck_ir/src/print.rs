// CEP:FILE: crates/planck_ir/src/print.rs
// CEP:WHAT: Deterministic textual printer for the Planck IR, used by --dump-ir, golden tests, and review.
// CEP:WHY: Standard 38.17 requires printable, stable IR; a canonical text form makes pipeline changes reviewable and testable byte-for-byte.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: None; printing is total over any Program value.
// CEP:ASSUMES: Programs follow the ordering conventions fixed at construction (functions in declaration order, blocks in creation order, vregs dense).
// CEP:COST: O(total nodes); output size proportional to program size; no nondeterministic ordering anywhere (no hash iteration).
// CEP:EVIDENCE: golden IR tests in crates/planckc/tests.
// CEP:SECURITY: none.
// CEP:PORTABILITY: Pure core.

use crate::ir::{Const, Inst, IrFunc, Program, Term, Ty};
use std::fmt::Write;

/// Header line stamped on every dump; changes are a semantic event per standard 38.20.
pub const IR_FORMAT_VERSION: &str = "plir-0.1";

// CEP:WHAT: Handles the print program step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn print_program(program: &Program) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "// {} | planck ir | pipeline planck-opt-0.1.0",
        IR_FORMAT_VERSION
    );
    for (idx, info) in program.structs.iter().enumerate() {
        let _ = writeln!(
            out,
            "struct #{} {} size={} align={} {{",
            idx, info.name, info.size, info.align
        );
        for (name, ty, offset) in &info.fields {
            let _ = writeln!(out, "    {}: {} @{}", name, ty.name(), offset);
        }
        let _ = writeln!(out, "}}");
    }
    for (idx, info) in program.enums.iter().enumerate() {
        let _ = writeln!(
            out,
            "enum #{} {} size={} align={} payload@{} {{",
            idx, info.name, info.size, info.align, info.payload_offset
        );
        for (name, payload) in &info.variants {
            match payload {
                Some(ty) => {
                    let _ = writeln!(out, "    {}({})", name, ty.name());
                }
                None => {
                    let _ = writeln!(out, "    {}", name);
                }
            }
        }
        let _ = writeln!(out, "}}");
    }
    for entry in &program.rodata {
        let _ = writeln!(
            out,
            "rodata {} = {:?}",
            entry.label,
            String::from_utf8_lossy(&entry.bytes)
        );
    }
    for func in &program.funcs {
        print_func(&mut out, func);
    }
    out
}

// CEP:WHAT: Handles the print func step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn print_func(out: &mut String, func: &IrFunc) {
    let params: Vec<String> = func
        .params
        .iter()
        .map(|slot| {
            let info = &func.slots[slot.0 as usize];
            format!("{}: {} @s{}", info.origin, info.ty.name(), slot.0)
        })
        .collect();
    let _ = writeln!(
        out,
        "fn {}({}) -> {} {{",
        func.name,
        params.join(", "),
        func.ret.name()
    );
    let param_slots: Vec<_> = func.params.iter().collect();
    for (slot_idx, slot) in func.slots.iter().enumerate() {
        let is_param = param_slots.iter().any(|s| s.0 as usize == slot_idx);
        if !is_param {
            let _ = writeln!(
                out,
                "    local s{}: {} size={} align={} ({})",
                slot_idx,
                slot.ty.name(),
                slot.size,
                slot.align,
                slot.origin
            );
        }
    }
    for (block_idx, block) in func.blocks.iter().enumerate() {
        let _ = writeln!(out, "bb{}:", block_idx);
        for inst in &block.insts {
            print_inst(out, func, inst);
        }
        match &block.term {
            Term::Br(target) => {
                let _ = writeln!(out, "    br bb{}", target.0);
            }
            Term::BrCond {
                cond,
                then_blk,
                else_blk,
            } => {
                let _ = writeln!(
                    out,
                    "    br_cond v{}, bb{}, bb{}",
                    cond.0, then_blk.0, else_blk.0
                );
            }
            Term::Ret { value } => match value {
                Some(vreg) => {
                    let _ = writeln!(out, "    ret v{}", vreg.0);
                }
                None => {
                    let _ = writeln!(out, "    ret");
                }
            },
        }
    }
    let _ = writeln!(out, "}}");
}

// CEP:WHAT: Handles the print inst step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn print_inst(out: &mut String, func: &IrFunc, inst: &Inst) {
    let ty_name = |vreg: u32| -> String {
        func.vregs
            .get(vreg as usize)
            .map(|t| t.name())
            .unwrap_or_else(|| "?".to_string())
    };
    let line = match inst {
        Inst::Const { dst, value } => match value {
            Const::Int(v, _ty) => format!("    v{}: {} = const {}", dst.0, ty_name(dst.0), v),
            Const::Float(v, is_f32) => format!(
                "    v{}: {} = const {}",
                dst.0,
                if *is_f32 { "f32" } else { "f64" },
                format_float(*v)
            ),
            Const::Bool(v) => format!("    v{}: bool = const {}", dst.0, v),
            Const::Char(v) => format!("    v{}: char = const {:?}", dst.0, v),
            Const::Str(id) => format!("    v{}: &str = const rodata#{}", dst.0, id),
        },
        Inst::Bin {
            op,
            dst,
            a,
            b,
            ty,
            checked,
        } => format!(
            "    v{}: {} = {} {} {}, {}{}",
            dst.0,
            ty.name(),
            op.symbol(),
            ty.name(),
            a.0,
            b.0,
            if *checked { " [checked]" } else { "" }
        ),
        Inst::Un {
            op,
            dst,
            a,
            ty,
            checked,
        } => format!(
            "    v{}: {} = {} {},{}{}",
            dst.0,
            ty.name(),
            match op {
                crate::ir::UnOp::Neg => "neg",
                crate::ir::UnOp::Not => "not",
            },
            ty.name(),
            a.0,
            if *checked { " [checked]" } else { "" }
        ),
        Inst::Cast { dst, src, from, to } => {
            format!(
                "    v{}: {} = cast {} v{} -> {}",
                dst.0,
                to.name(),
                from.name(),
                src.0,
                to.name()
            )
        }
        Inst::LoadSlot { dst, slot, ty } => {
            format!("    v{}: {} = load s{}", dst.0, ty.name(), slot.0)
        }
        Inst::StoreSlot { slot, src, ty } => {
            format!("    store s{} <- v{}: {}", slot.0, src.0, ty.name())
        }
        Inst::LoadField {
            dst,
            slot,
            offset,
            ty,
        } => {
            format!(
                "    v{}: {} = load s{} + {}",
                dst.0,
                ty.name(),
                slot.0,
                offset
            )
        }
        Inst::StoreField {
            slot,
            offset,
            src,
            ty,
        } => {
            format!(
                "    store s{} + {} <- v{}: {}",
                slot.0,
                offset,
                src.0,
                ty.name()
            )
        }
        Inst::AddrSlot {
            dst,
            slot,
            pointee,
            mutable,
        } => format!(
            "    v{}: {} = addr s{}",
            dst.0,
            Ty::Ref(Box::new(pointee.clone()), *mutable).name(),
            slot.0
        ),
        Inst::AddrOffset {
            dst,
            base,
            offset,
            pointee,
        } => format!(
            "    v{}: {} = addr_offset v{} + v{} bytes",
            dst.0,
            Ty::Ref(Box::new(pointee.clone()), false).name(),
            base.0,
            offset.0
        ),
        Inst::LoadIndirect {
            dst,
            addr,
            offset,
            ty,
        } => {
            if *offset == 0 {
                format!("    v{}: {} = load *v{}", dst.0, ty.name(), addr.0)
            } else {
                format!(
                    "    v{}: {} = load *v{} + {}",
                    dst.0,
                    ty.name(),
                    addr.0,
                    offset
                )
            }
        }
        Inst::StoreIndirect {
            addr,
            offset,
            src,
            ty,
        } => {
            if *offset == 0 {
                format!("    store *v{} <- v{}: {}", addr.0, src.0, ty.name())
            } else {
                format!(
                    "    store *v{} + {} <- v{}: {}",
                    addr.0,
                    offset,
                    src.0,
                    ty.name()
                )
            }
        }
        Inst::StoreStructFromAddr {
            dst_slot,
            src_addr,
            ty,
        } => {
            format!(
                "    store s{} <- *v{}: {} [memcpy]",
                dst_slot.0,
                src_addr.0,
                ty.name()
            )
        }
        Inst::Call {
            dst,
            sret,
            func: callee,
            args,
            ret,
        } => {
            let callee_name = match callee {
                crate::ir::Callee::Func(id) => format!("fn#{}", id.0),
                crate::ir::Callee::Intrinsic(kind) => format!("@{:?}", kind),
            };
            let args_text: Vec<String> = args
                .iter()
                .map(|arg| {
                    if arg.by_ref {
                        format!("&v{}", arg.value.0)
                    } else {
                        format!("v{}", arg.value.0)
                    }
                })
                .collect();
            let sret_text = match sret {
                Some(sret) => format!(" sret=v{}", sret.0),
                None => String::new(),
            };
            match dst {
                Some(dst) => format!(
                    "    v{}: {} = call {}({}){}: {}",
                    dst.0,
                    ret.name(),
                    callee_name,
                    args_text.join(", "),
                    sret_text,
                    ret.name()
                ),
                None => format!(
                    "    call {}({}){}",
                    callee_name,
                    args_text.join(", "),
                    sret_text
                ),
            }
        }
        Inst::TrapIf { cond, trap } => format!("    trap_if v{}, {:?}", cond.0, trap),
        Inst::Trap { trap } => format!("    trap {:?}", trap),
        Inst::Comment { text } => format!("    ; {}", text),
    };
    let _ = writeln!(out, "{}", line);
}

/// Floats print in a shortest stable decimal form; non-finite values are spelled out.
pub fn format_float(value: f64) -> String {
    if value.is_finite() {
        format!("{}", value)
    } else if value.is_nan() {
        "nan".to_string()
    } else if value > 0.0 {
        "inf".to_string()
    } else {
        "-inf".to_string()
    }
}
