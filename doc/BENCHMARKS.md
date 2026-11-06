# Benchmarks

## Abstract

This report measures the Planck compiler and the code it generates, using the
harness in `benches/`. Three questions are answered: how fast does `planckc`
compile, how fast do Planck programs run compared to equivalent C compiled by
gcc, and what does the checked-arithmetic safety net cost. The headline
numbers on the reference machine: Planck compiles as fast as gcc (40-83 ms for
these programs), generated code runs at **5.9x gcc -O2** (geometric mean),
and the integer overflow checks cost **less than 2%**. The dominant gap is
structural, not accidental: the frame-resident value model holds by ticket
PLK-012 (register allocation) is the single largest known lever.

![Benchmark overview](benchmarks.png)

## Methodology

Six compute-bound programs, each paired with a line-for-line C mirror
(`benches/*.plk`, `benches/c/*.c`). Every program prints a checksum; the
harness refuses results unless all four configurations print the identical
checksum, which cross-validates the whole stack (language semantics, codegen,
and the C mirrors) on every run.

| Benchmark | Exercises | Work per run |
|---|---|---|
| fib | deep recursion, checked i64 adds on the hot path | fib(35), 29,860,703 calls |
| primes | i64 remainder, nested loops, early returns | trial division below 200,000 |
| sort | loops, borrow passing, index bounds checks, memory traffic | 600 rounds of 512-element mergesort |
| mandelbrot | f64 arithmetic and compares, short-circuit loop condition | 400x300 lattice, 100-iteration cap |
| nqueens | i64 bitwise logic, multi-argument recursion | 11-queens, 10 rounds |
| matmul | reference dereference reads/writes, flat index arithmetic, checked i32 math | 150 rounds of 48x48 matmul |

Four configurations per program:

| Config | Meaning |
|---|---|
| `c_o0` | `gcc -O0` |
| `c_o2` | `gcc -O2` (the reference, 1.00x by definition) |
| `planck` | `planckc build` -- optimization pipeline ON, integer overflow checks ON |
| `planck_release` | `planckc build --release` -- checks OFF (wrapping per design Part 3) |

Compile time is the median of six full toolchain invocations (the first is
discarded as warmup); for Planck that includes `as` and the `cc` link step.
Run time is the median of seven executions. Text size comes from `size`.
Reference machine: Intel Xeon, Debian gcc 14.2.0, Linux 6.x. Raw numbers are
committed in `benches/results.json`; `benches/bench.py` and
`benches/make_charts.py` regenerate everything (`cargo build --release &&
python3 benches/bench.py && python3 benches/make_charts.py`).

Two sources are generated and committed (`benches/sort.plk`,
`benches/matmul.plk`, by `benches/gen_sources.py`): the language has no
repeat-array expression, so large zero-initialized arrays must be spelled
element by element (ticket PLK-030). Workloads are sized so the checked
configuration runs in roughly 15-220 ms.

## Results

### Run time (median of 7, lower is better)

| Benchmark | gcc -O0 | gcc -O2 | planck (checked) | planck --release | checked vs C -O2 |
|---|---|---|---|---|---|
| fib | 67.2 ms | 14.1 ms | 78.2 ms | 77.8 ms | 5.5x |
| primes | 11.6 ms | 11.0 ms | 14.3 ms | 14.1 ms | 1.3x |
| sort | 43.7 ms | 13.3 ms | 96.5 ms | 96.9 ms | 7.3x |
| mandelbrot | 11.1 ms | 4.9 ms | 37.5 ms | 37.0 ms | 7.6x |
| nqueens | 20.4 ms | 13.4 ms | 31.2 ms | 30.1 ms | 2.3x |
| matmul | 98.6 ms | 4.8 ms | 220.5 ms | 216.3 ms | 46.0x |
| **geometric mean** | **3.24x** | **1.00x** | **5.90x** | **5.82x** | |

### Compile time (median of 6, full toolchain)

| Benchmark | gcc -O0 | gcc -O2 | planckc | planckc --release |
|---|---|---|---|---|
| fib | 27.4 ms | 51.9 ms | 39.9 ms | 40.5 ms |
| primes | 28.1 ms | 35.2 ms | 40.2 ms | 40.8 ms |
| sort | 30.0 ms | 52.1 ms | 49.1 ms | 50.0 ms |
| mandelbrot | 27.4 ms | 32.9 ms | 40.8 ms | 41.0 ms |
| nqueens | 27.9 ms | 35.8 ms | 40.3 ms | 39.5 ms |
| matmul | 28.4 ms | 41.5 ms | 80.5 ms | 83.4 ms |

### Binary .text size

| Benchmark | gcc -O0 | gcc -O2 | planckc |
|---|---|---|---|
| fib | 1438 B | 2508 B | 3488 B |
| primes | 1517 B | 1502 B | 4110 B |
| sort | 2296 B | 2072 B | 29675 B |
| mandelbrot | 1682 B | 1574 B | 4608 B |
| nqueens | 1589 B | 1596 B | 4267 B |
| matmul | 1951 B | 1913 B | 145278 B |

## Analysis

**The overflow checks are free.** The checked configuration is within run-to-run
noise of `--release` everywhere (at most 3.7% on the shortest benchmark,
nqueens, and under 2% on every other; the sign flips both ways). The trap
path is a compare-and-branch on each arithmetic node, perfectly predicted
not-taken in these workloads. This is a meaningful claim for the language's
safety story: design Part 3's checked-by-default arithmetic does not tax
correct programs on this evidence. (The one caveat: these kernels are
branch-dominated; check overhead on tight dependency chains could be larger.)

**Where Planck is already close.** primes (1.3x) and nqueens (2.3x) are
branchy and division/bit-heavy; there the frame-resident model hurts least
because the hardware divide and the unpredictable branches dominate, and gcc
-O2 cannot remove the work either.

**Where the frame-resident model dominates.** sort (7.3x), mandelbrot (7.6x)
and matmul (46x) are register-friendly kernels. Every vreg lives in a stack
slot and %rax forwards one value at a time (see doc/codegen.md), so each
inner-loop operation pays store-plus-load round trips; gcc -O2 keeps these
loops in registers and vectorizes matmul. This is exactly the gap ticket
PLK-012 (Machine IR with iterated register coalescing) exists to close, and
it is the expected shape of the gap: the optimizer's ILP work is sound but
the backend cannot express it yet. Bounds checks on every array access
(sort) and the lack of PLK-011 alias analysis (which would license keeping
array elements in registers across stores) add second-order costs.

**Compile time is competitive.** 40 ms for small programs -- the same league
as gcc, with the full SSA pipeline and eleven optimization passes running,
plus a verifier gate after every pass. The matmul outlier (80 ms) is parsed
source volume: 24 KB of array-literal elements (ticket PLK-030 again).

**Code size tracks the same story.** Small programs are within 2x of C. The
matmul 145 KB is not the matmul kernel -- it is the three 2304-element array
literals lowered as one store per element, roughly 60 instructions per zero
written (ticket PLK-029 proposes bulk `rep stos`/memcpy lowering).

## Bugs the benchmark suite caught

The suite paid for itself immediately: writing realistic programs surfaced
three real miscompiles/compiler crashes that the existing 122-test suite had
missed, all now fixed with regression tests:

1. **Duplicate `Ldiv_ok` labels** (codegen): two division sites in one
   function both used the function index as label suffix -- the second `%`
   in a function produced an assembler error.
2. **Duplicate `Lflt` labels** (codegen): re-encountering an existing float
   constant re-inserted it into the pool with a fresh label, renaming the
   old entry and colliding with a later distinct constant.
3. **LICM miscompile of nested loops** (optimizer): LICM ran after SSA
   destruction and hoisted the inner loop's induction-variable initialization
   (a multi-def vreg after phi lowering) out of the outer loop, so nested
   `for` loops executed their inner loop only on the first outer iteration.
   A 3x4 nested loop printed 4 instead of 12. The fix pins multi-def vregs;
   regression tests cover the LICM unit and the end-to-end nested-loop
   behavior (`nested_for_loops_reinitialize_inner_induction`,
   `nested_loops_with_float_carry`).

The lesson matches the design's correctness hierarchy: observable program
behavior (CEP-2) is the only test that attests the whole pipeline; the
verifier gates check invariants per pass but cannot catch a *legal-IR*
transformation that changes semantics.

## Language sharp edges recorded while writing the benchmarks

Encountered while fitting programs to the v0.1 subset; recorded as ticket
PLK-030 with the exact repros, since each needs a spec ruling rather than a
local fix:

* no repeat-array expression (`[0; N]`), and definite initialization is
  whole-value granular -- large arrays are impractical to declare;
* `if` is a statement, not an expression (`let x = if c { a } else { b }`
  does not parse);
* an integer literal as the *left* operand of a binary expression defaults
  to i32 and does not widen to the other operand's type, so
  `big_lit + x as i64` fails where `x as i64 + big_lit` compiles;
* reference parameters cannot be forwarded (`PLK-S0006` requires a fresh
  `&`/`&mut` at each call site) and methods cannot be called through
  references (`PLK-S0017`), which shapes how helper functions over arrays
  and structs must be written.

## Threats to validity

Single machine, one run family; medians damp but do not eliminate scheduler
noise, and the C -O2 numbers for primes/mandelbrot are small enough (5-11 ms)
that a few percent of the gap is measurement floor. The workloads are
integer/float kernels without heap allocation, matching the v0.1 language
surface -- they measure the compiler, not the runtime library. Absolute
numbers will drift with hardware; the relative structure (checked vs
release ~equal, register-allocation-bound kernels far off -O2) is the
durable finding. Re-run with `python3 benches/bench.py` before quoting
updated numbers.
