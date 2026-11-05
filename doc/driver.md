# Driver (planckc)

## Abstract

The driver ties the pipeline together and invokes the host toolchain. Its inputs are command-line options and a `.plk` source file; its outputs are executables, IR dumps, and diagnostics with fixed exit codes.

## Formal Definitions

```text
planckc build <input.plk> [-o <output>] [--release] [--dump-ir] [--emit-asm]
planckc run <input.plk> [--release]
planckc dump-ir <input.plk> [--release]
```

Exit codes: 0 success; 1 compilation errors (diagnostics on stderr); 101 internal compiler error (verification or optimization gate violation); 2 usage error.

## Algorithm

1. Read the source (UTF-8), lex, parse; render syntax diagnostics and stop on errors.
2. Analyze (check-and-lower); render semantic diagnostics; stop on errors.
3. Verify the lowered IR; an ICE report (exit 101) fires on findings.
4. Optimize with gates; ICE on post-pass findings.
5. Command dispatch: `dump-ir` prints the post-optimization IR; `build` assembles via `as` (assembly piped through stdin so no input file name reaches the symbol table), compiles the runtime with `cc -c`, and links with `cc -Wl,--build-id=none`; `run` builds to a temp path, executes, and exits with the program's code.

## Invariants

1. No code generation occurs when any error diagnostic exists.
2. Compilation is byte-deterministic: same source, same flags, same binary (no timestamps, no build IDs, no input names embedded).
3. The driver never deletes user files; scratch paths live under the system temp directory.

## Integration Points

- Depends on: all pipeline crates.
- Depended on by: end users, tests (`cargo test -p planckc`), benchmarks.

## Known Limitations

- The runtime is located relative to the working directory; a packaged install path is future work.
- `run` builds to a fixed temp name; concurrent `planckc run` invocations could race.
