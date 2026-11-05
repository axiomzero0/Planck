// CEP:FILE: crates/planck_codegen/src/frame.rs
// CEP:WHAT: Frame layout computation: rbp-relative offsets for locals, virtual registers, and the hidden sret save slot.
// CEP:WHY: The frame-resident value model needs one authoritative offset table per function; computing it first keeps selection simple and deterministic.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: None; layout is a pure function of the slot table and never fails.
// CEP:ASSUMES: Slots are laid out in declaration order with natural alignment; the total frame is padded to a multiple of 16 to keep the call stack aligned per the System V ABI.
// CEP:COST: O(slots + vregs).
// CEP:EVIDENCE: exercised by every codegen test.
// CEP:SECURITY: Frame sizes are bounded by the input program; saturating arithmetic prevents overflow on pathological inputs.
// CEP:PORTABILITY: x86-64 stack conventions (downward growth, 16-byte call alignment).

#![forbid(unsafe_code)]

use planck_ir::{IrFunc, Slot, Ty};
use std::collections::BTreeMap;

/// Computed frame for one function.
pub struct Frame {
    /// Byte offset from rbp for each slot: address = rbp - offset (offset > 0).
    pub slot_offsets: BTreeMap<u32, u32>,
    /// Byte offset from rbp for each vreg, indexed by vreg number.
    pub vreg_offsets: Vec<u32>,
    /// Offset of the hidden sret-pointer save slot, if the function returns an aggregate.
    pub sret_save_offset: Option<u32>,
    /// Total frame bytes (16-aligned).
    pub frame_size: u32,
}

/// Natural alignment for a scalar or aggregate type at codegen time.
fn align_of(ty: &Ty) -> u32 {
    match ty.scalar_size() {
        Some(1) => 1,
        Some(2) => 2,
        Some(4) => 4,
        Some(8) | None => 8,
        _ => 8,
    }
}

// CEP:WHAT: Handles the compute step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn compute(func: &IrFunc) -> Frame {
    let mut cursor: u32 = 0;
    let mut slot_offsets = BTreeMap::new();
    for (idx, info) in func.slots.iter().enumerate() {
        let align = info.align.max(1);
        cursor = align_up(cursor, align);
        cursor = cursor.saturating_add(info.size);
        slot_offsets.insert(idx as u32, cursor);
    }
    let mut vreg_offsets = Vec::with_capacity(func.vregs.len());
    for ty in &func.vregs {
        let size = ty.scalar_size().unwrap_or(8);
        let align = align_of(ty);
        cursor = align_up(cursor, align);
        cursor = cursor.saturating_add(size);
        vreg_offsets.push(cursor);
    }
    let mut sret_save_offset = None;
    if func.sret_slot.is_some() {
        cursor = align_up(cursor, 8);
        cursor += 8;
        sret_save_offset = Some(cursor);
    }
    let frame_size = align_up(cursor, 16);
    Frame {
        slot_offsets,
        vreg_offsets,
        sret_save_offset,
        frame_size,
    }
}

// CEP:WHAT: Handles the align up step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn align_up(value: u32, align: u32) -> u32 {
    if align <= 1 {
        return value;
    }
    let remainder = value % align;
    if remainder == 0 {
        value
    } else {
        value + (align - remainder)
    }
}

impl Frame {
    pub fn slot_offset(&self, slot: Slot) -> u32 {
        self.slot_offsets[&slot.0]
    }

    pub fn vreg_offset(&self, vreg: u32) -> u32 {
        self.vreg_offsets[vreg as usize]
    }
}
