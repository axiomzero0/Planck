# Overview

## Abstract

Planck is a systems programming language with explicit ownership and borrowing, compiled to native x86-64 code by `planckc`. This repository contains the v0.1 compiler: a seven-crate Rust workspace covering lexing, parsing, check-and-lower semantic analysis, a verified CFG intermediate representation, an optimization pipeline, x86-64 System V code generation, and the driver. A data-driven lint tool, `cep_lint`, mechanically enforces the repository standard.

## Repository layout

```text
crates/planck_support   spans, source maps, diagnostics (CEP-1)
crates/planck_lexer     tokenizer (CEP-1)
crates/planck_parser    AST and recursive-descent parser (CEP-1)
crates/planck_sema      type check, mutability, borrow checking, AST-to-IR lowering (CEP-1)
crates/planck_ir        CFG IR, printer, verifier, dominators (CEP-1)
crates/planck_opt       verified pass pipeline (CEP-1)
crates/planck_codegen   x86-64 System V selection and emission (CEP-1)
crates/planckc          driver: build, run, dump-ir (CEP-1)
runtime/planck_rt.c     print builtins, panic handler, entry wrapper (CEP-1)
tools/cep_lint          CEP&CC mechanical enforcement (CEP-2)
tests/                  language-level fixtures (planned growth area)
doc/                    component documentation
.cep/                   lint configuration, evidence, waivers
```

## Pipeline

```text
source .plk
  | planck_lexer   tokens + lexical diagnostics
  | planck_parser  AST + syntax diagnostics
  | planck_sema    check-and-lower -> verified IR
  | planck_ir      verify (gate)
  | planck_opt     const_fold, gvn_cse, licm, peephole, dce (verify after each)
  | planck_ir      verify (gate)
  | planck_codegen x86-64 AT&T assembly
  | as (stdin)     object file
  | cc (link)      executable
```

Every gate is a hard failure: the driver refuses to proceed past verification
or optimization findings, and reports an internal compiler error instead of
emitting code.

## Build and check

```sh
cargo build --release
cargo test                                  # 78 tests across the workspace
cargo clippy --workspace --all-targets -- -D warnings
./target/debug/cep_lint .                   # repository lint gate
./target/debug/cep_lint --self-test         # 30 scenarios
```

Zero external dependencies: the workspace builds offline and every byte of
tool behavior is first-party. The compiler invokes only `as` and `cc` from the
host toolchain.

## Language subset (v0.1)

Implemented completely, with end-to-end tests: `i8`..`i64`, `u8`..`u64`,
`f32`, `f64`, `bool`, `char`, `&str` literals; `let`/`mut`; functions; methods
via `impl`; `if`/`while`/`loop`/`for` over ranges; `break`/`continue`;
structs; enums with single payloads and `match`; references `&T`/`&mut T` with
statement-scoped borrow checking; explicit `as` casts; fixed arrays with
runtime bounds checks; checked integer arithmetic (traps on overflow in the
default mode, wraps under `--release`); `print` builtins.

Not implemented, tracked with tickets in `.cep/tickets.md`: traits, generics,
comptime, `try`, `unsafe`, inline assembly, traits-based dynamic dispatch,
heap types, and the speculative JIT layer.
