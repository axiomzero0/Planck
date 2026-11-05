# Planck

Planck is a systems programming language with explicit ownership and borrowing, compiled to native x86-64 code. This repository contains the v0.1 compiler (`planckc`) and the repository's mechanical enforcement tool (`cep_lint`), both in Rust with zero external dependencies.

## Status

v0.1 implements the core language subset completely and honestly: see
[doc/conformance.md](doc/conformance.md) for the per-item mapping to
`design.md` and the CEP&CC standard, and [.cep/tickets.md](.cep/tickets.md)
for everything deferred with tickets.

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
