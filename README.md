# Planck

Planck is a systems programming language with explicit ownership and borrowing, compiled to native x86-64 code. This repository contains the v0.1 compiler (`planckc`) and the repository's mechanical enforcement tool (`cep_lint`), both in Rust with zero external dependencies.

## Status

v0.1 implements the core language subset completely and honestly: see
[doc/conformance.md](doc/conformance.md) for the per-item mapping to
`design.md` and the CEP&CC standard, and [.cep/tickets.md](.cep/tickets.md)
for everything deferred with tickets.

## Benchmarks

The harness in [benches/](benches/) compiles and runs six Planck programs
against line-for-line C mirrors, cross-validating every configuration's
output. On the reference machine (see
[doc/BENCHMARKS.md](doc/BENCHMARKS.md) for the full report): generated code
runs at 5.9x gcc -O2 (geometric mean; gcc -O0 is 3.2x), compilation takes
40-80 ms -- the same league as gcc -- and the checked-by-default integer
arithmetic costs under 2%. Reproduce with:

```sh
cargo build --release
python3 benches/bench.py
python3 benches/make_charts.py
```

## Quick start

```sh
cargo build --release

# compile and run a Planck program
cat > /tmp/hello.plk <<'EOF'
fn main() -> i32 {
    print("Hello, Planck!");
    print('\n');
    return 0;
}
