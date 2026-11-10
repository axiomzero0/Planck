# Conformance matrix

## Abstract

This document maps every normative element of `design.md` (the Planck language and compiler specification) and of `standard.md` (CEP&CC 0.4) to its implementation status in this repository. Statuses use the standard's vocabulary: complete, partial, stub, placeholder. Every non-complete item carries a ticket in `.cep/tickets.md`. This matrix is the honest boundary of the v0.1 milestone.

## design.md conformance

### Part 2: syntax and semantics

| Element | Status | Ticket / evidence |
|---|---|---|
| 2.1 files, entry point, blocks, comments | complete | e2e tests `hello_world`, `main_exit_code` |
| 2.2 variables, explicit mutability | complete | e2e `structs_methods_mutability`; sema tests |
| 2.3 primitives i8..u64, f32/f64, bool, char, `()` | complete | e2e `integer_widths_and_casts` |
| 2.3 raw pointers `*const T` / `*mut T` | stub | PLK-006 (unsafe) |
| 2.4 functions, explicit signatures | complete | e2e suite |
| 2.5 if/else, match (exhaustive via `_`), while/loop/for ranges | complete | e2e `control_flow`, `enums_match_payloads` |
| 2.6 structs and enums | complete | e2e suite |
| 2.7 traits and impls | partial: `impl` methods complete; traits absent | PLK-004 |
| 2.7 `dyn Trait` fat pointers | stub | PLK-005 |
| 2.8 attributes `#[...]` | partial: parsed and retained on functions; `repr(C)` honored by the declaration-order layout | PLK-019 |
| 2.9 unsafe and inline assembly | stub | PLK-006 |
| 2.10 generics | stub | PLK-007 |
| 2.10 comptime | stub | PLK-008 |
| 2.11 `Result`/`Option` and the `try` keyword | stub | PLK-009 |
| 2.12 no implicit casts, no hidden allocation, no GC | complete (within the subset) | doc/sema.md |

### Part 3: type safety and memory model

| Property | Status | Notes |
|---|---|---|
| Static, strong typing; no coercions | complete | sema rejects every implicit conversion |
| One-owner values, move semantics | complete for value types: structs/enums/arrays-of-non-Copy are non-Copy; use-after-move, double moves, partial moves, moves out of references/elements, move-while-borrowed, and moves in loops are compile errors | doc/sema.md; sema ownership tests; e2e `ownership_moves_and_deferred_init_run`, `driver_rejects_use_after_move` |
| Deferred initialization (`let` without initializer) with definite assignment | complete for the core subset: branch intersection, loop conservatism, diverging-arm exemption | sema deferred-init tests; doc/sema.md |
| Heap types, allocators, drop elaboration | stub | PLK-010 (remaining scope) |
| Borrowing, no mutable aliasing | complete (statement-scoped + let-persistent conservative model) | PLK-015 for NLL regions |
| Null safety via `Option` | stub | PLK-009 |
| Layout control attributes | partial: declaration-order layout with natural alignment; `#[repr(...)]` parsed | PLK-019 |
| Bounds checks | complete (always on; `unsafe` elision is PLK-006) | e2e `bounds_check_traps` |
| Overflow: panics in debug, wraps in release | complete | e2e `overflow_traps_in_debug_mode`, `overflow_wraps_in_release_mode` |

### Part 4: the 7-stage pipeline

| Design stage | Status | Implementation mapping | Ticket |
|---|---|---|---|
| Stage 1: VSDG | partial | The frontend lowers AST directly to the CFG IR; the value/state edge separation is realized as pure instructions versus effect instructions, and the borrow checker operates before IR construction rather than on the graph | PLK-002 |
| Stage 2: Sea of Nodes | partial | The IR keeps structured basic blocks instead of a floating sea; pure values are SSA vregs with phis at joins after slot promotion; the unstructured floating-sea form is not built | PLK-003 (remaining: floating nodes) |
| Stage 3: Memory IR extraction (alias analysis, PEA, SROA) | stub | Aggregates are frame-resident; alias analysis is unnecessary at this shape | PLK-011 |
| Stage 4: LLIR materialization | partial | The CFG IR is already materialized: leaders, terminators, dominators, natural loops; phi placement, LCSSA, and SSA destruction run in the optimizer (pipeline planck-opt-0.2.0) | PLK-003 |
| Stage 5: Vector IR | stub | No vector types | PLK-017 |
| Stage 6: Machine IR (DP selection, IRC regalloc) | partial | Stage-1 register caching landed: liveness + loop-depth selection into callee-saved rbx/r12-r15 and call-safe xmm8-12, frame-slot spill fallback; full Machine IR with DP selection and iterated coalescing remains | PLK-012 |
| Stage 7: assembly/object emission | partial | AT&T assembly through the system assembler; ELF via the system linker; no direct object emission or DWARF | PLK-018 |

### Part 5: the 212 optimizations

Implemented and verified in the pipeline (pass catalog numbers): constant
folding (1, 2), local and global CSE/GVN (4, 21, 25), strength reduction (6),
algebraic simplification (7), bitwise and boolean simplification (10, 11),
shift folding (12), commutativity reordering (13), sparse conditional constant
propagation with branch pruning (22), SSA construction and destruction (35),
redundant phi elimination (38), loop-closed SSA normalization (40), branch
folding (41, 48), unreachable code elimination (44), jump threading through
constant conditions (41), loop-invariant code motion (81),
induction-variable simplification (88, 96), global DCE (23, 5), and copy
propagation feeding the classic stage.

All other catalog entries are stubs tracked by PLK-013 (remaining local and
global passes), PLK-016 (vectorization block), PLK-012 (backend block), and
PLK-014 (interprocedural block). The full 212-row matrix lives in the ticket;
each row gains a row here when its pass lands.

### Part 6: JIT and runtime

The speculative JIT layer (profiling, inline caching, OSR, deoptimization) is
a stub: PLK-020. The AOT baseline is the whole of v0.1, which matches the
design's "correct-by-construction AOT baseline" property by construction.

### Part 7: LTO

Stubs: PLK-021. The IR is versioned and printable, which are the prerequisites
for serialized IR (SIR) emission.

### Part 8: standard library philosophy

Partial. The runtime is Tier 3 only (`rt`): print builtins and the panic
handler in `runtime/planck_rt.c`, explicitly linked, no hidden state. Tier 1
`core` traits and Tier 2 `std` collections with explicit allocators are
PLK-010 (remaining scope: heap types and allocators; the ownership machinery
they require — move checking and deferred initialization — is complete).

## standard.md (CEP&CC 0.4) conformance

| Requirement | Status | Evidence |
|---|---|---|
| Ch. 4 laws 1-8 | complete within scope | doc/cep_lint.md gates |
| Ch. 10 comment standard: file headers | complete | `cep_lint .` clean at severity 0/1 |
| Ch. 10 comment standard: function blocks | complete | same gate |
| Ch. 16/50 mechanical enforcement: cep_lint | complete | 17 configured rules; DOC-SYNC live; self-test 30/30 |
| Ch. 25 Rust policy | complete for CEP-1/CEP-2 files; no CEP-0 files exist in v0.1 (Law 4: no unmeasured hot code is labeled hot) | doc/cep_lint.md |
| Ch. 32 file structure | partial: crate layout mirrors the pipeline; the hot/cold directory split is deferred until CEP-0 code exists | PLK-022 |
| Ch. 33 naming | complete | NAMING rules enforced |
| Ch. 38.10 deterministic translation | complete | e2e `compilation_is_deterministic` (byte-identical binaries) |
| Ch. 38.17-38.19 IR contract | complete | doc/ir.md invariants (plir-0.2: phis, copies, args, multi-def reaching rule); printable, hashable-shape, versioned, verified |
| Ch. 38.18 IR verification | complete | verifier gates after lowering and after every pass; phi/Arg placement rules; reaching-definitions check for post-destruction vregs |
| Ch. 38.20 pass pipeline contract | complete | versioned pipeline `planck-opt-0.2.0` with fixed order |
| Ch. 38.11-38.13 compile budgets, trust model | complete | every HPC-COMPILE-COST field states O(n) bounds; inputs are untrusted by construction |
| Ch. 61 industrial testing | partial: 125 unit/integration/e2e tests, golden IR, determinism gate, plus a cross-verified benchmark suite (`benches/`, doc/BENCHMARKS.md) that diff-tests generated-code output against gcc-compiled mirrors; no fuzzing harness | PLK-023 |

## Known limitations

- The borrow checker is conservative: borrows live to the end of their
  statement (or enclosing block when bound by `let`), so some programs that
  NLL would accept are rejected.
- The move checker is conservative in loops: values live at loop entry cannot
  be moved inside the body, and loop-body assignments do not establish
  definite initialization after the loop.
- Struct passing uses hidden-pointer sret returns and by-reference caller
  copies rather than the full System V aggregate classification.
- Float printing uses a fixed `%.6f` format; `u64`-to-float conversions above
  2^63 use the signed conversion; float-to-integer casts use hardware
  truncation semantics without Rust-style saturation.
- The lint tool is lexical and structural; semantic cost claims still require
  the evidence artifacts under `.cep/evidence/`.
