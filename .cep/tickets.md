# Tickets

## Abstract

Every non-complete implementation item in this repository carries exactly one ticket here. Tickets are referenced by `CEP:TICKET:` fields in source comments and by `doc/conformance.md`. Status vocabulary is the standard's: stub (interface only), partial (intentionally incomplete for known cases).

| Ticket | Title | Status | Scope |
|---|---|---|---|
| PLK-001 | Workspace scaffold | complete | Initial crate layout; retained as the anchor for placeholder stubs that existed during staged construction. |
| PLK-002 | VSDG stage (design Stage 1) | partial | Lower the borrow checker onto the graph and add explicit value/state edge types; the frontend currently lowers AST directly to the CFG IR. |
| PLK-003 | Sea of Nodes + phi SSA + LCSSA (design Stages 2 and 4) | stub | Floating pure nodes, phi placement, loop-closed SSA. |
| PLK-004 | Traits and impl dispatch | stub | Trait definitions, impl blocks for traits, static dispatch via monomorphization. |
| PLK-005 | `dyn Trait` fat pointers | stub | Dynamic dispatch with explicit opt-in. |
| PLK-006 | `unsafe`, raw pointers, bounds-check elision, inline assembly | stub | Design 2.9 surface with explicit unsafe boundaries. |
| PLK-007 | Generics and monomorphization | stub | Design 2.10 generics. |
| PLK-008 | Bounded comptime execution | stub | Design 2.10 comptime interpreter with resource limits. |
| PLK-009 | `Result`, `Option`, and the prefix `try` keyword | stub | Design 2.11 error handling. |
| PLK-010 | Heap types, ownership move checking, allocators | stub | Engages the existing move/borrow machinery with non-Copy types; lets without initializers. |
| PLK-011 | Memory IR: alias analysis, PEA, SROA (design Stage 3) | stub | Deep alias analysis over abstract locations. |
| PLK-012 | Machine IR: DP instruction selection, iterated register coalescing, scheduling (design Stage 6, passes 139-158) | stub | Replaces the frame-resident model. |
| PLK-013 | Remaining local/global optimization passes (catalog 1-80 minus implemented) | stub | Row-by-row enablement with golden tests per pass. |
| PLK-014 | Interprocedural passes: inlining, IPCP, devirtualization (catalog 120-138) | stub | Requires serialized IR or whole-program mode. |
| PLK-015 | NLL region-based borrow lifetimes | stub | Replaces the statement-scoped conservative model. |
| PLK-016 | Vector IR and auto-vectorization (design Stage 5, catalog 100-119) | stub | Vector types, SCEV, cost model. |
| PLK-017 | Vectorization follow-on: masked paths, SLP, reductions | stub | Depends on PLK-016. |
| PLK-018 | Direct ELF emission and DWARF debug info | stub | Removes the system assembler dependency; adds source mapping. |
| PLK-019 | Target abstraction layer and `#[repr(...)]` layout variants | partial | One target (x86-64 System V); hooks isolated in planck_codegen. |
| PLK-020 | Speculative JIT layer: profiling, inline caching, OSR, deopt (design Part 6, catalog 159-177) | stub | Additive layer over the AOT baseline per design 6.1. |
| PLK-021 | LTO: SIR emission, ThinLTO indexing, ICF (design Part 7, catalog 193-212) | stub | Requires serialized IR. |
| PLK-022 | Hot/cold directory split and first CEP-0 classified module | stub | Requires Law 4 measurement infrastructure for hot code. |
| PLK-023 | Fuzzing harness and differential testing | stub | Chapter 61 crucible protocol. |
| PLK-024 | Defined shift-count semantics | stub | Shifts are hardware-masked in v0.1; design owes a ruling. |
| PLK-025 | System V aggregate argument classification | stub | Full by-value struct passing per the ABI. |
| PLK-026 | Float stack arguments beyond eight | stub | Rejected with a comment today; needs stack-class floats. |
| PLK-027 | Windows (PE/COFF) target | stub | Design Stage 7 names it; runtime needs a Windows port. |

## Rules for this file

1. A `CEP:TICKET:` reference in source must resolve to a row here.
2. Closing a ticket requires the conformance matrix row in `doc/conformance.md`
   to move to complete in the same change.
3. Tickets never expire silently: scope changes update the row in the same
   commit as the code change.
