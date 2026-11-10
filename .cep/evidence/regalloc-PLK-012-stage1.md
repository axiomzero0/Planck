# Codegen evidence: PLK-012 stage 1 — register allocation

## What

`crates/planck_codegen/src/alloc.rs`: per-function liveness analysis (backward
dataflow over the CFG), call-crossing detection at instruction granularity,
natural-loop nesting depth via CHK immediate dominators, and a deterministic
selection of hot vregs into registers no emission rule uses as scratch:
callee-saved `%rbx`/`%r12`-`%r15` for integers (prologue pushes, epilogue
pops, odd counts padded to keep call sites 16-byte aligned) and
`%xmm8`-`%xmm12` for floats not live across any call (xmm0-7 are the ABI
argument window). Every other value keeps the frame-slot path unchanged, so
the spill fallback is the pre-existing emitter.

## Why sound

A vreg is written only through a defining instruction's `dst` and is never
address-taken (addresses point at frame slots, not vregs), so one register
per vreg for the whole function is consistent at every use: every def writes
the register, every use reads it, calls preserve callee-saved registers, and
floats live across calls are excluded from caching. The uniform assignment
survives block joins and trap stubs by the same argument.

## Measured effect (benches/, checksum-cross-validated)

Same machine, same suite, before vs after (checked configuration, median of 7):

| benchmark | before (ms) | after (ms) | speedup |
|---|---|---|---|
| fib | 78.2 | 70.0 | 1.12x |
| primes | 14.3 | 12.8 | 1.12x |
| sort | 96.5 | 83.3 | 1.16x |
| mandelbrot | 37.5 | 27.2 | 1.38x |
| nqueens | 31.2 | 22.6 | 1.38x |
| matmul | 220.5 | 192.7 | 1.14x |

Geometric mean vs gcc -O2: 5.90x -> 4.99x (1.18x faster overall). Every
`.text` section shrank (register moves replaced load/store pairs). Full
numbers: `doc/BENCHMARKS.md`, raw data `benches/results.json`.

## Verification

- `cargo test` all green (128 tests incl. the new allocator unit tests and
  the `shifts_compute_at_all_widths` e2e regression the work surfaced).
- `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt` clean.
- `tools/cep_lint` 0 failing.
- Deterministic-build gate: two `planckc build --emit-asm` runs of the demo
  byte-identical.

## Bug caught during validation

The bench suite rejected `shlq %al` (byte register with quad suffix) on the
first run against the new shift lowering: the count/value register pair must
match the instruction width. Fixed to use the width-matching pair and pinned
by `shifts_compute_at_all_widths`, which compiles and executes shifts at
i64/i32 widths through cached and frame-resident operands.

## Remaining scope (PLK-012)

Machine IR representation, DP table-driven instruction selection, iterated
register coalescing with live-range splitting (the SSA-destruction copies at
loop joins still move through memory slots today), and scheduling.
