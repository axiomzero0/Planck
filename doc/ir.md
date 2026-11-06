# Planck IR

## Abstract

The IR is a typed control-flow graph of basic blocks with SSA temporaries, phi nodes for join values, and frame-slot locals for what the optimizer did not promote, plus a deterministic printer, a verifier, and dominator analysis. It is the contract between the frontend and the optimizer/backend: everything downstream assumes the invariants below.

## Formal Definitions

```rust
pub struct Program { pub funcs: Vec<IrFunc>, pub structs: Vec<StructInfo>, pub enums: Vec<EnumInfo>, pub rodata: Vec<RodataEntry>, pub ir_version: String }
pub struct IrFunc { pub name: String, pub params: Vec<Slot>, pub ret: Ty, pub sret_slot: Option<Slot>, pub slots: Vec<SlotInfo>, pub vregs: Vec<Ty>, pub blocks: Vec<Block> }
pub struct Block { pub insts: Vec<Inst>, pub term: Term }
pub enum Term { Br(BlockId), BrCond { cond, then_blk, else_blk }, Ret { value: Option<Vreg> } }
// SSA stage instructions (plir-0.2):
pub enum Inst { ..., Phi { dst, ty, inputs: Vec<(BlockId, Vreg)> }, Copy { dst, src, ty }, Arg { dst, index, ty } }
```

Value conventions: scalar types travel in vregs; aggregates (`Struct`, `Enum`, `Array`) travel as addresses in vregs of reference type. Aggregate returns use a hidden `sret` slot; the callee memcpy's the result to the caller's pointer.

## Algorithm

The printer emits the format version header, layouts, rodata, then functions with slots and blocks in declaration order — O(nodes). The verifier checks, per function: block count, terminator validity and targets, operand use-def (dominance for single-def vregs, reaching definitions for multi-def vregs), operand types, slot bounds, phi region and phi-input rules, Arg placement rules, and rodata bounds — O(N + E) after dominators (Cooper-Harvey-Kennedy, near-linear typical, O(N^2) worst case with dense bitsets), plus O(defs x blocks) per multi-def vreg for reaching sets.

## Invariants

1. Blocks are dense `0..n`; block 0 is the entry; every block ends in exactly one terminator whose targets exist.
2. Vregs are dense `0..m` per function. Before SSA destruction every vreg is single-assignment and every use is dominated by its definition. After SSA destruction a former phi destination has one definition per incoming edge, and every use is reached by at least one of them (reaching-definitions check).
3. Slots are dense `0..k` per function with declared size and alignment.
4. Comparison instructions produce `Bool`; branch conditions are `Bool`.
5. Reference values are produced only by `AddrSlot`, `AddrOffset`, or loads of reference-typed slots.
6. Phis (when present) occupy a leading prefix of their block, carry exactly one input per structural predecessor in ascending order, and each input value is defined in — or dominates — its input edge's source block. Entry blocks have no phis.
7. `Arg` instructions appear only as the leading prefix of the entry block, one per promoted parameter in parameter order; this is a correctness rule because an `Arg` reads an ABI register that any call would clobber.
8. The printed form is byte-deterministic: same program, same dump.

## Integration Points

- Depends on: `planck_support`.
- Depended on by: `planck_sema` (construction), `planck_opt` (mutation under verification; the SSA stage owns the phi lifecycle from construction to destruction), `planck_codegen` (consumption — requires phi-free input), `planckc` (dumps).

Signatures: `pub fn verify_program(&Program) -> Vec<VerifyError>`, `pub fn print_program(&Program) -> String`, `pub fn Dominators::compute(&IrFunc) -> Dominators`, `pub fn Inst::def_vreg(&self) -> Option<Vreg>`, `pub fn Inst::use_vregs(&self) -> Vec<Vreg>`.

## Known Limitations

- Full Sea-of-Nodes floating (pure nodes unbound from blocks) is not implemented: phis live in a structured CFG, which trades peak optimization power for a simple, verifiable lowering contract (PLK-002/PLK-003 remaining scope).
- Serialization is limited to the printable text form; the hashable/serializable contract items await PLK-021.
