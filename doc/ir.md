# Planck IR

## Abstract

The IR is a typed control-flow graph of basic blocks with SSA temporaries and frame-slot locals, plus a deterministic printer, a verifier, and dominator analysis. It is the contract between the frontend and the optimizer/backend: everything downstream assumes the invariants below.

## Formal Definitions

```rust
pub struct Program { pub funcs: Vec<IrFunc>, pub structs: Vec<StructInfo>, pub enums: Vec<EnumInfo>, pub rodata: Vec<RodataEntry>, pub ir_version: String }
pub struct IrFunc { pub name: String, pub params: Vec<Slot>, pub ret: Ty, pub sret_slot: Option<Slot>, pub slots: Vec<SlotInfo>, pub vregs: Vec<Ty>, pub blocks: Vec<Block> }
pub struct Block { pub insts: Vec<Inst>, pub term: Term }
pub enum Term { Br(BlockId), BrCond { cond, then_blk, else_blk }, Ret { value: Option<Vreg> } }
```

Value conventions: scalar types travel in vregs; aggregates (`Struct`, `Enum`, `Array`) travel as addresses in vregs of reference type. Aggregate returns use a hidden `sret` slot; the callee memcpy's the result to the caller's pointer.

## Algorithm

The printer emits the format version header, layouts, rodata, then functions with slots and blocks in declaration order — O(nodes). The verifier checks, per function: block count, terminator validity and targets, operand use-def with dominance (via dominators), operand types, slot bounds, and rodata bounds — O(N + E) after dominators (Cooper-Harvey-Kennedy, near-linear typical, O(N^2) worst case with dense bitsets).

## Invariants

1. Blocks are dense `0..n`; block 0 is the entry; every block ends in exactly one terminator whose targets exist.
2. Vregs are single-assignment and dense `0..m` per function; every use is dominated by its definition.
3. Slots are dense `0..k` per function with declared size and alignment.
4. Comparison instructions produce `Bool`; branch conditions are `Bool`.
5. Reference values are produced only by `AddrSlot`, `AddrOffset`, or loads of reference-typed slots.
6. The printed form is byte-deterministic: same program, same dump.

## Integration Points

- Depends on: `planck_support`.
- Depended on by: `planck_sema` (construction), `planck_opt` (mutation under verification), `planck_codegen` (consumption), `planckc` (dumps).

Signatures: `pub fn verify_program(&Program) -> Vec<VerifyError>`, `pub fn print_program(&Program) -> String`, `pub fn Dominators::compute(&IrFunc) -> Dominators`.

## Known Limitations

- No phi nodes: locals live in frame slots, which trades peak performance for correctness (PLK-003).
- Serialization is limited to the printable text form; the hashable/serializable contract items await PLK-021.
