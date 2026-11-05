# Codegen (x86-64 System V)

## Abstract

Code generation lowers verified IR to AT&T-syntax x86-64 assembly. Its inputs are verified programs; its outputs are deterministic assembly text that the system assembler turns into objects. Every IR opcode has exactly one lowering rule in this module.

## Formal Definitions

The value model is frame-resident: every vreg and local has an rbp-relative stack slot; `%rax` holds the most recently computed value (the forwarding invariant), and narrow stores re-widen rax so forwarded values are always canonical. Frame layout assigns slots in declaration order with natural alignment; the total frame is 16-byte aligned.

Call ABI: integer-class arguments in rdi, rsi, rdx, rcx, r8, r9; float class in xmm0-7; overflow on the stack with 16-byte call alignment; hidden sret pointer as the first integer argument. Aggregate parameters arrive as pointers to caller copies and are memcpy'd into callee slots.

## Algorithm

1. Compute the frame (slots, vregs, sret save).
2. Prologue: push rbp, sub rsp frame, spill parameters at their true widths (an 8-byte spill into a 4-byte slot would corrupt the packed neighbor).
3. Per block: label, instructions, terminator. `%rax` forwarding elides reloads; calls and traps invalidate it.
4. Trap paths: overflow (jo/jc by signedness, with range compares for narrow multiplies), bounds, division by zero, and the MinInt/-1 signed-division guard branch to shared stubs that call `planck_rt_panic` (exit 101).
5. Epilogue: scalar returns in rax/xmm0; aggregate returns memcpy the sret buffer and return the pointer.

Emission is O(instructions), one pass, no scheduling.

## Invariants

1. Output is byte-deterministic: same IR, same assembly.
2. Bounds and overflow checks are emitted unconditionally; there is no mode that removes them except the documented release-mode overflow wrapping.
3. Narrow slot stores and spills never write beyond the slot's width.
4. All trap targets referenced by a function have stubs in that function.

## Integration Points

- Depends on: `planck_ir` (verified programs), `planck_support`.
- Depended on by: `planckc` (pipes the assembly to `as` via stdin).

Signature: `pub fn emit_assembly(program: &Program) -> String`.

## Known Limitations

- Frame-resident values trade peak performance for correctness; register allocation is implicit (PLK-012).
- No instruction scheduling (PLK-020).
- `u64`-to-float above 2^63 and float-to-integer casts use hardware semantics with documented deviations (PLK-023-adjacent notes in the source).
- More than eight float arguments are unsupported (PLK-026).
