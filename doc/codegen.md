# Codegen (x86-64 System V)

## Abstract

Code generation lowers verified IR to AT&T-syntax x86-64 assembly. Its inputs are verified programs; its outputs are deterministic assembly text that the system assembler turns into objects. Every IR opcode has exactly one lowering rule in this module.

## Formal Definitions

The value model is frame-resident with stage-1 register caching: every vreg
and local has an rbp-relative stack slot; selected hot vregs additionally
live in a dedicated register for their whole lifetime (`alloc.rs` chooses
callee-saved `rbx`/`r12`-`r15` for integers and `xmm8`-`xmm12` for floats
not live across a call, ranked by use count weighted by loop-nesting depth).
A cached vreg's uses read its register and its definitions re-widen `%rax`
then copy into the register, so the canonical-widened invariant holds in
either location; all other values follow the original slot path, with `%rax`
holding the most recently computed value (the forwarding invariant), and
narrow stores re-widening rax so forwarded values are always canonical.
Vregs are never aliased in memory (only `dst` writes them; addresses point
at slots, not vregs), which is what makes one uniform register per vreg
sound across blocks, calls, and trap stubs. Used callee-saved registers are
pushed after the frame `subq` (with one slot of padding when the count is
odd, keeping calls 16-byte aligned) and popped before `leave`. Frame layout
assigns slots in declaration order with natural alignment; the total frame
is 16-byte aligned.

Call ABI: integer-class arguments in rdi, rsi, rdx, rcx, r8, r9; float class in xmm0-7; overflow on the stack with 16-byte call alignment; hidden sret pointer as the first integer argument. Aggregate parameters arrive as pointers to caller copies and are memcpy'd into callee slots.

## Algorithm

1. Compute the frame (slots, vregs, sret save) and the register assignment
   (liveness, loop-depth weighting, deterministic ranking in `alloc.rs`).
2. Prologue: push rbp, sub rsp frame (plus odd-save padding), push used
   callee-saved cache registers, save the hidden sret pointer, spill
   parameters at their true widths (an 8-byte spill into a 4-byte slot would
   corrupt the packed neighbor).
3. Per block: label, instructions, terminator. `%rax` forwarding elides
   reloads; cached vregs move register-to-register; calls and traps
   invalidate the forwarding register, and calls cannot disturb cache
   registers (callee-saved, or XMM outside the ABI argument window and not
   live across the call).
4. Trap paths: overflow (jo/jc by signedness, with range compares for narrow
   multiplies), bounds, division by zero, and the MinInt/-1 signed-division
   guard branch to shared stubs that call `planck_rt_panic` (exit 101).
5. Epilogue: pop cache registers in reverse push order; scalar returns in
   rax/xmm0; aggregate returns memcpy the sret buffer and return the
   pointer.

Emission is O(instructions); allocation is O(instructions) plus dominator
computation per function.

## Invariants

1. Output is byte-deterministic: same IR, same assembly (allocation ranks
   by weighted score with vreg-index tiebreaks over ordered containers).
2. Bounds and overflow checks are emitted unconditionally; there is no mode
   that removes them except the documented release-mode overflow wrapping.
3. Narrow slot stores and spills never write beyond the slot's width, and
   cached registers always hold the canonical widened value.
4. All trap targets referenced by a function have stubs in that function.
5. A vreg has at most one cache register for its whole lifetime; the emit
   paths and the allocator read the same assignment, and no emission rule
   uses a cache register as scratch.

## Integration Points

- Depends on: `planck_ir` (verified programs), `planck_support`.
- Depended on by: `planckc` (pipes the assembly to `as` via stdin).

Signature: `pub fn emit_assembly(program: &Program) -> String`.

## Known Limitations

- Stage-1 allocation caches five integer and five float vregs per function;
  SSA-destruction copies at loop joins still move through memory slots, and
  there is no live-range splitting or coalescing. The Machine IR with
  instruction selection and iterated register coalescing replaces this
  stage (PLK-012, remaining scope).
- No instruction scheduling (PLK-020).
- `u64`-to-float above 2^63 and float-to-integer casts use hardware semantics with documented deviations (PLK-023-adjacent notes in the source).
- More than eight float arguments are unsupported (PLK-026).
