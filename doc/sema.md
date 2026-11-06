# Sema (check-and-lower)

## Abstract

Semantic analysis validates names, types, mutability, borrows, moves, and definite initialization, and lowers the AST to the CFG IR in a single pass per function. Its inputs are a parsed `Module`; its outputs are an `ir::Program` plus diagnostics with codes PLK-S0001 through PLK-S0025. The output is code-generated only when no error diagnostics exist.

## Formal Definitions

```rust
pub struct SemaOutput { pub program: Program, pub diags: Vec<Diag> }
pub struct FnSig { pub name: String, pub params: Vec<Ty>, pub ret: Ty, pub method_of: Option<String>, pub func_id: u32 }
struct Binding { slot: Slot, ty: Ty, mutable: bool }
struct ActiveBorrow { root: Slot, mutable: bool, persistent: bool, name: String }
pub enum LValue { Slot { slot, offset, ty, root, root_name, path }, Indirect { addr, offset, ty, root, root_name, path } }
struct OwnerState { initialized: bool, moved: bool, fields_moved: BTreeSet<u32> }
// owners: BTreeMap<Slot, OwnerState> — flow-sensitive per function
```

Layout: structs are declaration-order with natural alignment; enums carry a u32 tag at offset 0 with the payload at `align_up(4, payload_align)`.

Copy classification (ticket PLK-010): integers, `f32/f64`, `bool`, `char`, `()`, references (`&T`, `&mut T`, `&str`) are `Copy`. Structs, enums, and arrays of non-`Copy` elements are **not** `Copy`: every by-value use of such a value is a *move*, per design Part 1 ("every value has exactly one owner; moving transfers ownership; copying requires explicit `Clone`").

## Algorithm

1. Build type tables: register aggregate names, resolve field types, compute layout with memoization; reject value-recursive types (`PLK-S0019`).
2. Collect signatures for top-level functions and impl methods (`Type::method`), in declaration order; validate the `main` contract.
3. Per function: lower statements and expressions. Aggregate-typed expressions evaluate to the address of a temporary slot (the address-value convention). Short-circuit operators lower to branches with a result slot. Match lowers to tag/equality compare chains with per-arm payload bindings.
4. Borrow checking: an active-borrow set per function. `&mut x` conflicts with any active borrow of `x`; `&x` conflicts with active mutable borrows; owner uses conflict with mutable borrows; writes and *moves* conflict with shared borrows. Borrows are statement-scoped, or live to the end of the enclosing block when bound by `let`.
5. Move checking (ticket PLK-010): every consuming context — a by-value call argument, a by-value method receiver, a returned value, the right-hand side of `let` or assignment with non-`Copy` type, and an enum match scrutinee — validates its source place (definitely initialized, not already moved, no active borrow) and then marks the move. A bare identifier moves the whole value; a field path moves only that field (tracked per field offset). Moves out of references (`*r`), dereference paths, and array elements (`a[i]`) are rejected (`PLK-S0025`) because the original owner would survive. Reading any part of a value requires it to be alive: whole reads are poisoned by any partial move, field reads only by a move of that field. Reassignment restores the state: a whole assignment re-initializes the binding, a field assignment cancels that field's partial move.
6. Definite-assignment analysis (ticket PLK-010): `let x: T;` declares storage without a value. The `owners` map carries an `initialized` bit flow-sensitively. Reads and borrows of a not-definitely-assigned variable are `PLK-S0024`. At an `if` join the initialized bit intersects over live (non-diverging) paths; at a match join it intersects over arm outcomes; moved bits union (a value moved on any live path is treated as moved). Loop bodies are analyzed under the entry state and the post-loop state is the entry state (a body may run zero times); moves of values live at loop entry are rejected outright (`PLK-S0023`) because later iterations would observe a moved value. Statements after a terminator inside a block are not analyzed (divergence is tracked with a dead flag and exempts the arm from join constraints).

Time complexity O(program size); scope, borrow, and owner sets are per-function (the owner map is proportional to the number of bindings).

## Invariants

1. The emitted IR passes `planck_ir::verify_program`; the driver runs the gate immediately after analysis.
2. Every read of a variable checks owner access against active borrows and checks initialization and move state.
3. A `&mut` borrow never coexists with any other borrow of the same root; a move never coexists with any borrow of the same root.
4. Writes through references check the reference's mutability, not the root binding's.
5. Missing returns in non-unit functions are compile errors (`PLK-S0022`), verified by reachability.
6. A non-`Copy` value has at most one live owner at every program point the checker accepts: after a move the source is unusable until reassigned.
7. A value read by the checker is definitely initialized on every path that reaches the read.

## Integration Points

- Depends on: `planck_parser` (AST), `planck_lexer` (literal types), `planck_ir`, `planck_support`.
- Depended on by: `planck_opt` (verified IR), `planck_codegen`.

Signature: `pub fn check_and_lower(module: &Module, checked_overflow: bool) -> SemaOutput`.

## Known Limitations

- Borrows are statement-scoped (block-scoped when bound by `let`), which rejects some programs NLL accepts (PLK-015).
- The move checker is conservative in loops: a binding visible at loop entry cannot be moved anywhere inside the body, and an assignment inside a loop does not establish definite initialization after the loop. Reassignment (`x = make();`) is the supported pattern.
- Aggregate initialization is whole-value: fields cannot be assigned individually until the whole value is initialized.
- Heap types, allocators, and drop elaboration do not exist yet; ownership is enforced over memcpy value semantics (PLK-010 remains partial for them).
- No returning references from functions; no closures.
- Match arm expressions in non-unit functions return from the function (match-as-terminator semantics; expression-position match is PLK-022).
