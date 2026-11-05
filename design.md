# THE COMPLETE PLANCK LANGUAGE AND COMPILER SPECIFICATION

This document represents the exhaustive, uncompressed architectural and syntactic specification for **Planck**. It encompasses the language semantics, the 7-stage compiler pipeline, the 212-pass optimization engine, the speculative JIT architecture, and the minimalist standard library philosophy.

---

## PART 1: CORE IDENTITY & PHILOSOPHY

*   **Paradigm:** Systems programming language.
*   **Core Philosophy:** Explicitness, readability, and unambiguous semantics. 
*   **Zero Hidden Magic:** No implicit conversions, no hidden control flow, no syntactic sugar that obscures memory or type behavior, and no hidden allocations.
*   **Safety Model:** Rust-level compile-time verification (ownership/borrowing) with explicit `unsafe` boundaries for low-level operations.
*   **Execution Model:** A correct-by-construction AOT baseline, augmented by a transparent, speculative JIT layer for dynamic performance tuning.

---

## PART 2: SYNTAX & SEMANTICS SPECIFICATION

### 2.1 General Structure
*   **File Extension:** `.plk`
*   **Entry Point:** `fn main() -> i32` (Returns an exit code).
*   **Statements:** Every statement ends with a semicolon `;`.
*   **Blocks:** Enclosed in curly braces `{}`.
*   **Comments:** `//` for line comments, `/* */` for block comments.

### 2.2 Variables & Mutability
Mutability is explicit. Variables are immutable by default.
```planck
let x: i32 = 10;          // Immutable
let mut y: i32 = 20;      // Mutable
y = 30;                   // OK
// x = 15;                // Error: cannot assign to immutable variable
```

### 2.3 Types & Primitives
Types are explicit. No type inference for function signatures or public APIs.
*   **Integers:** `i8`, `i16`, `i32`, `i64`, `i128`, `u8`, `u16`, `u32`, `u64`, `u128`.
*   **Floats:** `f32`, `f64`.
*   **Booleans:** `bool` (`true`, `false`).
*   **Characters:** `char` (Unicode scalar value).
*   **Pointers:** `*const T`, `*mut T` (Raw pointers, `unsafe` only).
*   **References:** `&T` (Immutable borrow), `&mut T` (Mutable borrow).
*   **Unit Type:** `()` (Used instead of `void` to maintain a uniform, first-class type system).

### 2.4 Functions
Arguments and return types must be explicitly typed.
```planck
fn add(a: i32, b: i32) -> i32 {
    return a + b;
}

// No-argument function returning the Unit type
fn greet() -> () {
    print("Hello");
}
```

### 2.5 Control Flow
No implicit truthiness. Conditions must evaluate to `bool`.

**If/Else:**
```planck
if x > 10 {
    print("Large");
} else if x == 10 {
    print("Equal");
} else {
    print("Small");
}
```

**Match (Exhaustive Pattern Matching):**
```planck
match value {
    1 => print("One"),
    2 => print("Two"),
    _ => print("Other"), // Wildcard required for exhaustiveness
}
```

**Loops:**
```planck
// Infinite loop
loop { break; }

// While loop
while condition { /* ... */ }

// For loop (explicit iterator)
// 0..10 is exclusive (0 through 9)
// 0..=10 is inclusive (0 through 10)
for i in 0..10 {
    print(i);
}
```

### 2.6 Structs & Enums
```planck
struct Point {
    x: f64,
    y: f64,
}

enum Option<T> {
    Some(T),
    None,
}
```

### 2.7 Traits & Implementations
Traits define shared behavior. Static dispatch (monomorphization) is the default. Dynamic dispatch requires explicit opt-in via `dyn Trait` (fat pointers).
```planck
trait Drawable {
    fn draw(&self) -> ();
}

impl Drawable for Point {
    fn draw(&self) -> () {
        print("Drawing point");
    }
}
```
*   **Auto-Traits:** Marker traits like `Send` and `Sync` are automatically implemented by the compiler based on type structure to guarantee safe concurrency.

### 2.8 Attributes
Metadata and compiler directives use the bracketed `#[...]` syntax.
```planck
#[repr(C)]
#[repr(packed)]
struct NetworkHeader {
    id: u16,
    flags: u8,
}

#[no_mangle]
extern "C" fn process_data(ptr: *mut u8, len: usize) -> i32 {
    return 0;
}
```

### 2.9 Unsafe & Inline Assembly
Explicit `unsafe` blocks are required for raw pointer dereferencing, extern calls, and inline assembly. Assembly uses a structured, keyword-based syntax for readability.
```planck
unsafe {
    let ptr: *const i32 = &x;
    let val: i32 = *ptr;

    let mut result: u64;
    asm(
        "mov {0}, rax",
        out(reg) result,
        clobber("rax", "memory")
    );
}
```

### 2.10 Generics & Comptime
**Generics:**
```planck
fn identity<T>(value: T) -> T {
    return value;
}
```

**Comptime (Bounded & Deterministic):**
Comptime executes during compilation. It has full language access but is strictly bounded (no wall-clock time, no randomness, resource limits enforced) to guarantee fast compile times.
```planck
comptime fn generate_sine_table(size: usize) -> [f64; size] {
    let mut table = [0.0; size];
    for i in 0..size {
        table[i] = math::sin(2.0 * PI * i as f64 / size as f64);
    }
    return table;
}

const SINE_TABLE: [f64; 256] = generate_sine_table(256);
```

### 2.11 Error Handling & The `try` Keyword
No exceptions. Explicit `Result<T, E>` and `Option<T>` types. 
To reduce boilerplate without hiding control flow, Planck uses the **prefix `try` keyword** (no postfix `?` operator).
```planck
fn read_config(path: &str) -> Result<Config, IoError> {
    // 'try' explicitly signals early return on Err/None
    let file: File = try File::open(path);
    let content: String = try file.read_to_string();
    return Result::Ok(parse_json(content));
}
```
*Rules for `try`:*
1. Cannot be nested inside complex arithmetic/logical expressions.
2. Requires the enclosing function to return `Result` or `Option`.
3. Requires explicit error conversion traits if error types differ.

### 2.12 Key Explicitness Rules
1.  **No Implicit Casts:** `i32` to `i64` requires `as i64`.
2.  **No Hidden Allocations:** String literals are `&str`. Owned strings require explicit allocation (`String::from()`).
3.  **No Default Operator Overloading:** Must implement specific traits (e.g., `Add`).
4.  **No Garbage Collection:** Memory management is explicit via ownership/borrowing.

---

## PART 3: TYPE SAFETY & MEMORY MODEL

*   **Static & Strong:** All types known at compile-time. No implicit coercions.
*   **Linear/Ownership Types:** Every value has exactly one owner. Moving transfers ownership; copying requires explicit `Clone`.
*   **Borrowing & Lifetimes:** References do not own data. Mutable aliasing is strictly prohibited. Lifetimes are tracked to prevent dangling references.
*   **Null Safety:** No null pointers. Handled via `Option<T>`.
*   **Memory Layout:** Controlled via attributes (`#[repr(C)]`, `#[repr(packed)]`, `#[repr(transparent)]`).
*   **Bounds & Overflow:** Array bounds checked by default (elided in `unsafe`). Integer overflow panics in debug, wraps in release.

---

## PART 4: THE 7-STAGE COMPILER PIPELINE & IRs (FULL DETAIL)

Planck utilizes a highly specialized, multi-stage IR pipeline to separate concerns (memory, vectorization, control flow) and enable aggressive, targeted optimizations.

### Stage 1: Frontend to VSDG (Value State Dependence Graph)
*   **Role:** Entry-level IR. Captures high-level semantics, explicit ownership, borrowing, and comptime execution.
*   **Graph Primitives:** Separates **Value Nodes** (pure computations) and **State Nodes** (side effects, control flow). Edges are strictly divided into **Value Edges** (data) and **State Edges** (execution order).
*   **AST Lowering Rules:** Pure expressions become Value Nodes. Side effects (assignments) become State Nodes. 
*   **Ownership & Borrowing:** The borrow checker operates directly on the graph. Owned values have exactly one consumer. Immutable borrows (`BorrowImm`) are Value Nodes allowing multiple consumers. Mutable borrows (`BorrowMut`) are State Nodes, strictly ordering execution to prevent aliasing. Lifetimes are annotations on Value Edges.
*   **Control Flow & `try`:** `if/else` creates `Branch` and `Merge` State Nodes. The `try` keyword lowers to a `TrySplit` State Node that outputs a success path and an error path that bypasses all subsequent nodes directly to the `FunctionExit`.
*   **Comptime Integration:** Evaluated *during* VSDG construction. The AST lowering pauses, the interpreter runs, and the result is injected as a `Constant` Value Node.

### Stage 2: VSDG to Sea of Nodes (SoN) Lowering
*   **Role:** Core optimization IR. Flattens explicit control flow into an unstructured graph.
*   **Core Philosophy:** Separates pure computations from side effects. Pure nodes float based on data dependencies and are implicitly control-dependent on dominating regions. Control nodes (`If`, `Region`, `Phi`) manage flow. Effect nodes maintain a lightweight "Effect Chain" for side effects.
*   **Translation Rules:** VSDG `Branch` becomes SoN `If`. VSDG `Merge` becomes `Region` and `Phi` nodes. SSA construction inserts `Phi` nodes where value edges converge.
*   **Handling `try`:** The `TrySplit` node becomes a standard `If` node testing the discriminant. The error path produces a control edge connecting directly to the function's `Return` node.
*   **Optimizations Unlocked:** Global Value Numbering (GVN), Sparse Conditional Constant Propagation (SCCP), Global Common Subexpression Elimination (GCSE).

### Stage 3: Sea of Nodes to Memory IR Extraction
*   **Role:** Isolates memory semantics from pure compute.
*   **Core Philosophy:** Breaks the simplified SoN effect chain using deep Alias Analysis. A `Store` is explicitly tied to an Abstract Memory Location. A `Load` only depends on a `Store` if they alias.
*   **Deep Alias Analysis:** Uses Type-Based Alias Analysis (TBAA), Steensgaard (Flow-Insensitive), and Andersen (Flow-Sensitive) to sever false dependencies.
*   **Breaking the Effect Chain:** Enables Load Hoisting, Store Sinking, and precise Mod/Ref Call Site Modeling.
*   **Partial Escape Analysis (PEA):** Tracks `New` nodes. If an allocation doesn't escape, it is sunk or stack-promoted. Partial escapes use Materialization Nodes.
*   **Scalar Replacement of Aggregates (SROA):** Destroys non-escaping structs/arrays into independent scalars, enabling Dead Field Elimination.

### Stage 4: Memory IR to Low Level IR (LLIR) Materialization
*   **Role:** Materializes structure. Converts the floating Memory IR back into a strict, SSA-based Control Flow Graph (CFG).
*   **Basic Block Construction:** Identifies leaders, places nodes into blocks based on dominating control nodes, and terminates blocks with explicit jumps/branches.
*   **Phi Placement & SSA:** Materializes `Phi` nodes at block entries. Converts to Loop-Closed SSA (LCSSA) form to simplify loop optimizations.
*   **Instruction Scheduling:** Topological sorting respecting data/memory edges, applying latency hiding heuristics and register pressure management.
*   **Loop Recognition:** Builds dominance trees, identifies back-edges, and extracts natural loops to enable LICM, Unrolling, etc.

### Stage 5: Low Level IR (LLIR) to Vector IR
*   **Role:** Exploits Data-Level Parallelism (DLP).
*   **Primitives:** Target-independent vector types (`<N x T>`), masks (`<N x bool>`), and operations (`vload`, `vstore`, `vgather`, `vshuffle`).
*   **Loop Vectorization:** Uses Scalar Evolution (SCEV) to prove trip counts and strides. Widens scalar loops and handles epilogues.
*   **Superword Level Parallelism (SLP):** Packs independent, isomorphic scalar instructions in straight-line code into vector nodes.
*   **Masked Vectorization:** Uses masks for loop tails and conditionals to avoid scalar fallbacks.
*   **Vector Cost Modeling:** Evaluates latency, throughput, shuffle overhead, and register pressure to guarantee zero-cost vectorization.

### Stage 6: Vector IR to Machine IR
*   **Role:** Target commitment. Maps abstract vectors and virtual registers to physical hardware constraints.
*   **Type Legalization:** Maps abstract types to physical register classes (GPR, FPR, VR128/256/512). Splits or scalarizes unsupported vector types.
*   **Instruction Selection:** Uses **Dynamic Programming (DP) on the DAG** to find the globally optimal minimum-cost covering of the DAG with target-specific machine instructions, natively handling shared subexpressions and complex addressing modes.
*   **Instruction Scheduling:** List scheduling with hazard avoidance and resource-constrained scheduling. Modulo scheduling for software pipelining.
*   **Register Allocation:** Uses **Iterated Register Coalescing (IRC)** to aggressively eliminate `MOV` instructions via graph coloring, with iterative simplification, freezing, and spilling.
*   **Post-RA Finalization:** Eliminates virtual registers, integrates spill code, performs branch relaxation, and applies machine peephole optimizations.

### Stage 7: Machine IR to Assembly/Object Emission
*   **Role:** Deterministic binary generation.
*   **Binary Encoding:** Maps instructions to binary opcodes, ModR/M, SIB, and immediates. Calculates PC-relative offsets and inserts alignment padding.
*   **Relocations & Symbols:** Generates relocation records for external references and builds the symbol table with binding and type metadata.
*   **Object File Generation:** Packages output into ELF (Linux), PE/COFF (Windows), or Mach-O (macOS) formats.
*   **Debug Information:** Generates DWARF or PDB metadata to map optimized machine code back to Planck source lines and variables.

---

## PART 5: THE COMPLETE 212 OPTIMIZATIONS

### 1. Local & Peephole Optimizations (Basic Block Level)
1. Constant Folding | 2. Constant Propagation | 3. Copy Propagation | 4. Local CSE | 5. Local DCE | 6. Strength Reduction | 7. Algebraic Simplification | 8. Peephole Optimization | 9. Instruction Combination | 10. Bitwise Optimization | 11. Boolean Simplification | 12. Shift and Mask Folding | 13. Commutativity/Associativity Reordering | 14. Negation Folding | 15. Two's Complement Simplification | 16. Min/Max Folding | 17. Absolute Value Folding | 18. Sign/Zero Extension Folding | 19. Byte Swap / Rotate Folding | 20. Multiplexing (Select/Ternary) Folding

### 2. Global Data Flow & SSA-Based Optimizations
21. Global Value Numbering (GVN) | 22. Sparse Conditional Constant Propagation (SCCP) | 23. Global Dead Code Elimination | 24. Partial Dead Code Elimination (PDCE) | 25. Global Common Subexpression Elimination (GCSE) | 26. Tail Recursion Elimination | 27. Sibling Call Optimization | 28. Lazy Code Motion (LCM) | 29. Very Busy Expression Analysis | 30. Available Expression Analysis | 31. Anticipable Expression Analysis | 32. Live Variable Analysis | 33. Reaching Definitions | 34. Use-Def Chain Optimization | 35. SSA Construction/Destruction | 36. Dead Argument Elimination | 37. Dead Return Value Elimination | 38. Redundant Phi Elimination | 39. Phi Translation | 40. Loop Closed SSA (LCSSA)

### 3. Control Flow Optimizations
41. Jump Threading | 42. Block Merging | 43. Edge Splitting | 44. Unreachable Code Elimination | 45. Branch Prediction Hints | 46. PGO Block Reordering | 47. If-Conversion | 48. Branch Folding | 49. Tail Duplication | 50. Loop Rotation | 51. Switch Lowering | 52. Switch Clustering | 53. Correlated Value Propagation | 54. Correlated Branch Folding | 55. Critical Edge Splitting | 56. Speculative Control Flow Optimization | 57. Predication | 58. Select Lowering | 59. Guard Hoisting/Sinking | 60. Profile-Guided Switch Reordering

### 4. Memory, Pointer, & Escape Optimizations
61. Alias Analysis (Steensgaard, Andersen, Flow-Sensitive) | 62. Escape Analysis (EA) | 63. Partial Escape Analysis (PEA) | 64. Scalar Replacement of Aggregates (SROA) | 65. Dead Store Elimination (DSE) | 66. Redundant Load Elimination | 67. Allocation Sinking / Stack Promotion | 68. Write/Read Barrier Elision | 69. Type-Based Alias Analysis (TBAA) | 70. Scope-Based Alias Analysis | 71. Mod/Ref Analysis | 72. Memory Dependence Analysis | 73. Load Forwarding | 74. Store Forwarding | 75. Store Merging | 76. Dead Field Elimination | 77. Struct Splitting | 78. Array Bounds Check Elimination | 79. Null Pointer Check Elimination | 80. Alignment Assumption Propagation

### 5. Loop Optimizations
81. Loop-Invariant Code Motion (LICM) | 82. Loop Unrolling | 83. Loop Unswitching | 84. Loop Fusion / Jamming | 85. Loop Fission / Distribution | 86. Loop Interchange | 87. Loop Tiling / Strip-Mining | 88. Induction Variable Elimination | 89. Loop Versioning | 90. Speculative Loop Optimization | 91. Loop Unroll and Jam | 92. Loop Collapsing | 93. Loop Peeling | 94. Affine Loop Recognition | 95. Canonical Induction Variable Insertion | 96. Strength Reduction of Loop Invariants | 97. Loop-Aware SLP | 98. Loop Idiom Recognition | 99. Loop Exit Value Optimization

### 6. Vectorization & SIMD
100. Auto-Vectorization (Loop Vectorization) | 101. Superword Level Parallelism (SLP) | 102. Masked Vectorization | 103. SIMD Instruction Selection | 104. Vector Memory Access Optimization | 105. Basic Block Vectorization | 106. Interleaved Memory Access Vectorization | 107. Gather/Scatter Vectorization | 108. Vector Predication | 109. Vector Widening/Narrowing | 110. Vector Shuffling/Permute Optimization | 111. Vector Reduction Optimization | 112. Horizontal Vector Operations | 113. SIMD Register Packing | 114. Vector Type Legalization | 115. Vector Cost Modeling | 116. Vector Function Multiversioning | 117. Auto-Vectorization with Runtime Dispatch | 118. Loop Vectorization with Scalar Evolution | 119. SLP Vectorization with Graph Coloring

### 7. Interprocedural & Call Graph Optimizations
120. Inlining | 121. Partial Inlining | 122. Profile-Guided Inlining | 123. Devirtualization | 124. Dead Virtual Function Elimination | 125. Interprocedural Constant Propagation (IPCP) | 126. Interprocedural DCE (IPDCE) | 127. Clone Specialization / Context-Sensitive Inlining | 128. Bottom-Up Inlining | 129. Top-Down Inlining | 130. Cold Function Outlining | 131. Argument Promotion | 132. Return Value Optimization (RVO) | 133. Named Return Value Optimization (NRVO) | 134. Elision of Temporary Objects | 135. Link-Time Inlining | 136. Cross-Module Devirtualization | 137. Class Hierarchy Analysis (CHA) | 138. Rapid Type Analysis (RTA)

### 8. Backend: Instruction Selection, Scheduling, & Register Allocation
139. Instruction Selection (DP on DAG) | 140. Instruction Scheduling | 141. Modulo Scheduling / Software Pipelining | 142. Register Allocation | 143. Register Coalescing | 144. Live Range Splitting | 145. Spill Code Optimization | 146. Tail Merging | 147. Branch Relaxation | 148. Jump Table Generation | 149. Bottom-Up Rewrite System (BURS) | 150. Graph Coloring Register Allocation | 151. Linear Scan Register Allocation | 152. SSA-Based Register Allocation | 153. Live Range Shortening | 154. Spill Cost Calculation | 155. Rematerialization | 156. Branch Shortening | 157. Hazard Avoidance Scheduling | 158. Resource-Constrained Scheduling

### 9. JIT-Specific & Dynamic Compilation
159. On-Stack Replacement (OSR) | 160. Deoptimization (Uncommon Traps) | 161. Speculative Optimization | 162. Inline Caching | 163. Guard Insertion and Elimination | 164. Runtime Profile Feedback Integration | 165. Hot/Cold Code Splitting | 166. Baseline Compilation | 167. Optimizing Compilation | 168. Super-Optimizing Compilation | 169. Speculative Inlining | 170. Speculative Devirtualization | 171. Speculative Type Profiling | 172. Monomorphic Inline Caching | 173. Polymorphic Inline Caching | 174. Megamorphic Inline Caching | 175. Inline Cache Miss Profiling | 176. Guard Sinking | 177. Uncommon Trap Insertion

### 10. Advanced, Academic, & Research Optimizations
178. Supercompilation | 179. Deforestation / Fusion | 180. List Fusion | 181. List Deforestation | 182. Stream Fusion | 183. Work-Time Optimality | 184. Size-Depth Optimality | 185. Abstract Interpretation Driven Optimizations | 188. Homeomorphic Embedding | 189. Whistle Detection | 190. Configuration Graph Folding | 191. Driving | 192. Generalization

### 11. Link-Time & Whole-Program (LTO)
193. Link-Time Optimization (LTO) | 194. ThinLTO | 195. Whole Program Analysis (WPA) | 196. Cross-Module Inlining | 197. Global Devirtualization | 198. Dead Data Elimination | 199. Section Garbage Collection | 200. Symbol Garbage Collection | 201. Weak Symbol Resolution | 202. COMDAT Folding | 203. Identical Code Folding (ICF) | 204. String Tail Merging | 205. String Pooling / Interning | 206. Debug Info Optimization | 207. Debug Info Stripping | 208. Type Merging | 209. VTable Merging | 210. VTable Optimization | 211. RTTI Optimization | 212. Exception Handling Table Optimization

---

## PART 6: THE JIT AND RUNTIME ARCHITECTURE

The JIT is not a separate compilation paradigm; it is an additive, speculative layer on top of a correct-by-construction AOT/Baseline foundation. The AOT code is the absolute source of truth for correctness. The JIT code is merely a speculative, high-performance projection of the AOT code, compiled incrementally based on runtime behavior.

### 6.1 Core Philosophy: Speculation and Incrementalism
*   **The AOT Baseline:** The program starts executing the AOT-compiled Machine IR. This code is fully safe, un-speculative, and handles all edge cases.
*   **Incremental Recompilation:** The JIT monitors execution, identifies hot methods, and incrementally recompiles *only* those specific methods by injecting speculative assumptions into their existing IR graphs.
*   **Fallback Safety:** Because the AOT baseline is always present and correct, the JIT can make incredibly aggressive, unsafe assumptions. If an assumption fails, the runtime simply discards the speculative JIT code and falls back to the AOT baseline.

### 6.2 Profiling and Inline Caching
*   **Inline Caching (IC):** At dynamic dispatch sites, the AOT code includes an inline cache (Monomorphic, Polymorphic, or Megamorphic).
*   **Edge and Type Profiling:** The AOT baseline includes lightweight counters at branch edges and type checks. When a counter exceeds a threshold, it triggers the JIT compiler to analyze that specific method.

### 6.3 Incremental Speculative Compilation
When a method is flagged as hot, the JIT loads the existing optimized IR and applies speculative transformations:
*   **Speculative Devirtualization:** Replaces virtual calls with direct calls if the inline cache shows a single dominant type.
*   **Speculative Unboxing:** Strips `Option` or `Result` wrappers if proven always `Some`/`Ok`.
*   **Guard Insertion:** Every speculative optimization is protected by a Guard (a runtime check). If the guard fails, execution branches to a deoptimization stub.

### 6.4 On-Stack Replacement (OSR)
OSR allows the runtime to transition from AOT code to JIT code *while the function is already executing* (e.g., in a long-running loop).
1.  **OSR Entry Points:** AOT code includes special entry points at loop headers that check a counter.
2.  **Frame Materialization:** When the counter trips, an OSR Stub reads the physical registers/stack of the AOT code and mathematically reconstructs the JIT's expected virtual state (accounting for inlining, SROA, unboxing).
3.  **Mid-Execution Jump:** The stub jumps directly into the middle of the JIT's Machine IR.

### 6.5 Deoptimization (Uncommon Traps)
Deoptimization occurs when a speculative guard fails.
1.  **The Uncommon Trap:** The failing guard branches to an Uncommon Trap Stub.
2.  **Frame Reconstruction:** The stub captures the JIT state, un-inlines functions, re-boxes enums, and reconstructs the exact AOT memory layout.
3.  **Resuming AOT Execution:** The instruction pointer is set to the exact AOT instruction corresponding to the point of failure. Execution resumes in the safe AOT baseline. The failed JIT code is discarded.

---

## PART 7: LINK-TIME OPTIMIZATION (LTO) & WHOLE-PROGRAM ANALYSIS

To achieve cross-module optimizations, the compiler emits a Serialized Intermediate Representation (SIR) alongside the object file. The linker consumes this SIR to perform whole-program analysis.

### 7.1 Phase 1: ThinLTO Indexing (Fast Path)
*   The compiler generates a lightweight **Summary Index** for each module (call graph, type hierarchies, inline hints).
*   The linker merges these summaries to build a global view in milliseconds.
*   The linker partitions modules into independent "ThinLTO imports" and dispatches them to multiple compiler threads for parallel, localized cross-module optimization.

### 7.2 Phase 2: Whole-Program Analysis & Global Devirtualization
*   **Rapid Type Analysis (RTA) & Class Hierarchy Analysis (CHA):** Tracks every concrete type instantiated globally. If a trait method is only implemented by one type, the call is globally devirtualized.
*   **Dead Virtual Function Elimination:** Removes machine code for trait implementations that are never instantiated.

### 7.3 Phase 3: Cross-Module Optimization & Inlining
*   **Cross-Module Inlining:** Functions marked `#[inline]` are physically copied into the caller's IR graph across crate boundaries.
*   **Interprocedural Constant Propagation (IPCP):** Specializes functions for constant arguments passed across module boundaries.

### 7.4 Phase 4: Machine Code Deduplication
*   **Identical Code Folding (ICF):** Hashes machine code of all functions. Identical binaries with identical relocation profiles are merged.
*   **COMDAT Folding:** Merges identical data sections (string literals, constant tables).
*   **Dead Data/Code Elimination:** Garbage collects any section or symbol not transitively reachable from the entry point.

---

## PART 8: STANDARD LIBRARY AND RUNTIME PHILOSOPHY

Planck’s standard library is strictly minimal. The core philosophy is **"Pay only for what you use, and explicitly wire what you need."** There are no hidden global states, no implicit allocations, and no mandatory runtime overhead. Maximum customization is achieved by making every runtime component an explicit, swappable interface.

### 8.1 The Three-Tier Architecture

**Tier 1: `core` (The Bare Metal)**
*   **Scope:** The absolute minimum required to write Planck code.
*   **Contents:** Primitive types, basic traits (`Clone`, `Copy`, `Send`, `Sync`), `Option`, `Result`, and compiler intrinsics.
*   **Dependencies:** Zero. No OS knowledge, no heap allocator, no I/O. Strictly `#![no_std]` by nature.

**Tier 2: `std` (The Minimal Standard Library)**
*   **Scope:** Essential abstractions for systems and application programming (`Vec`, `String`, `HashMap`, file I/O, threading, default async executor).
*   **The Explicit Allocator Rule:** `std` does **not** have a hidden, globally available heap allocator. Collections require an explicit `Allocator` trait object or generic parameter. To use the OS default, the user must explicitly instantiate and pass `std::alloc::SystemAllocator`. This ensures zero hidden allocations.

**Tier 3: `rt` (The Runtime & Startup)**
*   **Scope:** Glue code that initializes the environment (stack setup, TLS, default panic handler).

### 8.2 `no_std` Environments and Maximum Customization
To support embedded systems, kernels, or constrained environments, users can completely strip the standard library and runtime.

1.  **Opting Out:** `#![no_std]` at the crate root removes `std` and `rt`, leaving only `core`.
2.  **Custom Entry Point:** The default `_start` wrapper is disabled. The user provides their own:
    ```planck
    #[no_mangle]
    pub extern "C" fn _start() -> ! {
        // Custom hardware initialization
        loop {}
    }
    ```
3.  **Swappable Panic Handler:** Users must implement the panic interface if they want custom logging or recovery instead of a hard abort.
4.  **Swappable Async Executor:** Planck's `async`/`await` lowers to a state machine (generator). The user can pass this state machine to *any* custom executor (embedded event loop, custom work-stealing runtime, WASM host) by implementing the `Future` trait and calling `.poll()`.
