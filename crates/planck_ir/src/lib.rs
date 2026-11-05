// CEP:FILE: crates/planck_ir/src/lib.rs
// CEP:WHAT: Planck IR definitions, deterministic printer, and verifier with dominator analysis.
// CEP:WHY: This crate is the contract between the check-and-lower frontend and the optimizer/backend; its invariants are what make verified optimization possible.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: verify_program returns findings instead of panicking; the printer and constructors are total.
// CEP:ASSUMES: See ir.rs module documentation for the full IR contract.
// CEP:COST: Construction O(N); printing O(N); verification O(N + E) after dominance O(N^2) worst case.
// CEP:EVIDENCE: cargo test -p planck_ir; golden dumps in crates/planckc/tests.
// CEP:SECURITY: The verifier is the security boundary between untrusted input (via sema) and code generation.
// CEP:PORTABILITY: Pure core.

#![forbid(unsafe_code)]

pub mod ir;
pub mod print;
pub mod verify;

pub use ir::{
    BinOp, Block, BlockId, CallArg, Callee, Const, EnumId, EnumInfo, FuncId, Inst, Intrinsic,
    IrFunc, Program, RodataEntry, Slot, SlotInfo, StructId, StructInfo, Term, TrapKind, Ty, UnOp,
    Vreg,
};
pub use print::{format_float, print_program, IR_FORMAT_VERSION};
pub use verify::{verify_program, Dominators, VerifyError};

/// Applies a layout alignment to a byte offset, rounding up.
// CEP:WHAT: Handles the align up step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn align_up(offset: u32, align: u32) -> u32 {
    if align <= 1 {
        return offset;
    }
    let remainder = offset % align;
    if remainder == 0 {
        offset
    } else {
        offset + (align - remainder)
    }
}

/// Natural alignment for scalar types; the layout authority for aggregates lives in sema.
pub fn scalar_align(ty: &Ty) -> u32 {
    match ty.scalar_size() {
        Some(1) => 1,
        Some(2) => 2,
        Some(4) => 4,
        Some(8) | None => 8,
        _ => 8,
    }
}
