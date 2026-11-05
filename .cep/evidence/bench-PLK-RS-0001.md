# Bench evidence: PLK-RS-0001 — sum loop, one million iterations

## What

`benches/sum_loop.plk`: a range loop accumulating `0..1000000` into an `i64`
with a checked add per iteration (debug-checked overflow semantics are off in
the release binary; this bench uses the release build of the generated code).
The program prints the total (499999500000) and exits.

## Measured

- machine: Linux x86-64 sandbox (shared vCPU), glibc 2.41, kernel 6.x
- compiler: planckc 0.1.0 (release), rustc 1.99.1, gcc 14.2.0 (as + ld)
- flags: `planckc build benches/sum_loop.plk -o /tmp/sum_loop` (release profile
  of the generated code: `--release` is the generated-code flag; the bench used
  the default checked mode — overflow traps were never hit)
- input: fixed loop bound 1,000,000
- wall time over 7 runs: min 4.20 ms, median 4.24 ms, max 4.46 ms
  (includes process spawn and runtime init)
- expected arithmetic throughput: ~1e9 simple additions/second on the reference
  machine for the register-resident loop body after forwarding

## Interpretation

The frame-resident value model issues one load, one add, one store, and one
compare-branch per iteration from/to stack slots. The measured ~4 ms for 1e6
iterations (~4 ns/iter) is consistent with that memory-traffic-bound loop on
the shared-vCPU reference machine. Register allocation (PLK-012) is the
expected major lever; do not treat this number as a performance claim beyond
"measured on this target on this date".

## Artifacts

- source: `benches/sum_loop.plk`
- binary: rebuilt from source at measurement time (deterministic bytes per
  the determinism gate)
- date: 2026-11-06
