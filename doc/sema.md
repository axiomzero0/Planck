# Sema (check-and-lower)

## Abstract

Semantic analysis validates names, types, mutability, and borrows, and lowers the AST to the CFG IR in a single pass per function. Its inputs are a parsed `Module`; its outputs are an `ir::Program` plus diagnostics with codes PLK-S0001 through PLK-S0022. The output is code-generated only when no error diagnostics exist.

## Formal Definitions

```rust
pub struct SemaOutput { pub program: Program, pub diags: Vec<Diag> }
pub struct FnSig { pub name: String, pub params: Vec<Ty>, pub ret: Ty, pub method_of: Option<String>, pub func_id: u32 }
struct Binding { slot: Slot, ty: Ty, mutable: bool }
struct ActiveBorrow { root: Slot, mutable: bool, persistent: bool, name: String }
pub enum LValue { Slot { slot, offset, ty, root, root_name }, Indirect { addr, offset, ty, root, root_name } }
```

Layout: structs are declaration-order with natural alignment; enums carry a u32 tag at offset 0 with the payload at `align_up(4, payload_align)`. A type is Copy when every component is trivially relocatable; in v0.1 every well-formed type is Copy because no heap types exist.

## Algorithm

1. Build type tables: register aggregate names, resolve field types, compute layout with memoization; reject value-recursive types (`PLK-S0019`).
2. Collect signatures for top-level functions and impl methods (`Type::method`), in declaration order; validate the `main` contract.
3. Per function: lower statements and expressions. Aggregate-typed expressions evaluate to the address of a temporary slot (the address-value convention). Short-circuit operators lower to branches with a result slot. Match lowers to tag/equality compare chains with per-arm payload bindings.
4. Borrow checking: an active-borrow set per function. `&mut x` conflicts with any active borrow of `x`; `&x` conflicts with active mutable borrows; owner uses conflict with mutable borrows, and writes conflict with shared borrows. Borrows are statement-scoped, or live to the end of the enclosing block when bound by `let`.

Time complexity O(program size); scope and borrow sets are per-function.

## Invariants

1. The emitted IR passes `planck_ir::verify_program`; the driver runs the gate immediately after analysis.
2. Every read of a variable checks owner access against active borrows.
3. A `&mut` borrow never coexists with any other borrow of the same root.
4. Writes through references check the reference's mutability, not the root binding's.
5. Missing returns in non-unit functions are compile errors (`PLK-S0022`), verified by reachability.

## Integration Points

- Depends on: `planck_parser` (AST), `planck_lexer` (literal types), `planck_ir`, `planck_support`.
- Depended on by: `planck_opt` (verified IR), `planck_codegen`.

Signature: `pub fn check_and_lower(module: &Module, checked_overflow: bool) -> SemaOutput`.

## Known Limitations

- Borrows are statement-scoped (block-scoped when bound by `let`), which rejects some programs NLL accepts (PLK-015).
- Move semantics are vacuous while all types are Copy (PLK-010).
- No returning references from functions; no closures.
- Match arm expressions in non-unit functions return from the function (match-as-terminator semantics; expression-position match is PLK-022).
