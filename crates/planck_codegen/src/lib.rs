// CEP:FILE: crates/planck_codegen/src/lib.rs
// CEP:WHAT: x86-64 System V code generation: frame layout, instruction selection, and AT&T assembly emission.
// CEP:WHY: The design's Stage 7 commitment for this milestone: deterministic, verifiable assembly text that the system assembler and linker turn into executables.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: The emitter is total over verifier-valid IR and never panics; trap paths call the runtime panic handler, which exits with code 101 after printing the trap kind.
// CEP:ASSUMES: Input IR passed planck_ir::verify_program. The value model is frame-resident with %rax forwarding for the most recent definition to elide reloads; the stage-1 register allocator (alloc.rs) additionally caches selected hot vregs in registers the emitter never uses as scratch: callee-saved rbx/r12-r15 for integers and xmm8-12 for floats not live across a call. Calls may clobber rax, rcx, rdx, rsi, rdi, r8-r11, and xmm0-15, which is safe because every other value lives in frame slots and the cache registers are preserved by the ABI or excluded from call-crossing ranges. Floating-point printing uses the runtime's fixed %.6f format (deviation ticket PLK-016); u64-to-float and float-to-int conversions use hardware semantics with documented deviations (ticket PLK-023); shift counts are hardware-masked (ticket PLK-024). Struct passing uses hidden-pointer sret returns and by-reference caller copies (documented ABI deviation, ticket PLK-025).
// CEP:COST: O(instructions) emission; one pass, no scheduling in this milestone (ticket PLK-020 for list scheduling). Frame-resident values trade peak performance for correctness; the single-register forwarding recovers the dominant reload cost.
// CEP:EVIDENCE: cargo test -p planck_codegen; disassembly artifacts in .cep/evidence/.
// CEP:SECURITY: Generated code always performs the emitted bounds and overflow checks; no check elision exists in this milestone by construction.
// CEP:PORTABILITY: Emits for x86-64 with the System V ABI (Linux/macOS); the target/ abstraction layer (design Stage 6 target hooks) is ticket PLK-019.

#![forbid(unsafe_code)]

mod alloc;
mod frame;
mod x64;

pub use x64::emit_assembly;
