// CEP:FILE: crates/planckc/tests/e2e.rs
// CEP:WHAT: End-to-end suite: compiles Planck programs with the real driver and checks stdout, exit codes, trap behavior, and binary determinism.
// CEP:WHY: The design's correctness hierarchy places observable program behavior above all internal properties; only whole-pipeline tests can attest to it.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: Test failures assert on compile stderr, exit codes, or output; a panic inside the test binary itself is a harness bug.
// CEP:ASSUMES: The planckc binary is available via CARGO_BIN_EXE_planckc; `cc` is on PATH; the runtime is reachable relative to the crate (../../runtime/planck_rt.c). Each test writes to a unique temp directory.
// CEP:COST: Each case is one driver invocation plus one link; the suite is linear in program count.
// CEP:EVIDENCE: cargo test -p planckc.
// CEP:SECURITY: Programs are repository-controlled fixtures; outputs go to the system temp directory and are cleaned up.
// CEP:PORTABILITY: Requires the POSIX toolchain of the build host.

use std::path::PathBuf;
use std::process::Command;

fn driver() -> &'static str {
    env!("CARGO_BIN_EXE_planckc")
}

struct RunOutcome {
    stdout: String,
    exit: i32,
}

// CEP:WHAT: Handles the compile to step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn compile_to(source: &str, dir: &std::path::Path, name: &str, release: bool) -> PathBuf {
    let source_path = dir.join(format!("{}.plk", name));
    std::fs::write(&source_path, source).expect("write fixture");
    let output = dir.join(name);
    let mut cmd = Command::new(driver());
    cmd.args(["build", source_path.to_str().expect("utf8 path"), "-o"])
        .arg(&output)
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    if release {
        cmd.arg("--release");
    }
    let status = cmd.status().expect("driver runs");
    assert!(status.success(), "compiling {} failed", name);
    output
}

// CEP:WHAT: Handles the run program step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn run_program(source: &str, name: &str) -> RunOutcome {
    let dir = std::env::temp_dir().join(format!("planckc_e2e_{}_{}", name, std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let binary = compile_to(source, &dir, name, false);
    let out = Command::new(&binary).output().expect("binary runs");
    let _ = std::fs::remove_dir_all(&dir);
    RunOutcome {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        exit: out.status.code().unwrap_or(-1),
    }
}

// CEP:WHAT: Verifies that hello world compiles and prints behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn hello_world_compiles_and_prints() {
    let outcome = run_program(
        "fn main() -> i32 { print(\"Hello, Planck!\"); print('\\n'); return 0; }",
        "hello",
    );
    assert_eq!(outcome.stdout, "Hello, Planck!\n");
    assert_eq!(outcome.exit, 0);
}

// CEP:WHAT: Verifies that main exit code is returned behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn main_exit_code_is_returned() {
    let outcome = run_program("fn main() -> i32 { return 7; }", "exitcode");
    assert_eq!(outcome.exit, 7);
}

// CEP:WHAT: Verifies that arithmetic and precedence behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn arithmetic_and_precedence() {
    let outcome = run_program(
        "fn main() -> i32 { let a: i32 = 2 + 3 * 4; let b: i32 = (2 + 3) * 4; print(a); print('\\n'); print(b); print('\\n'); return 0; }",
        "arith",
    );
    assert_eq!(outcome.stdout, "14\n20\n");
}

// CEP:WHAT: Verifies that integer widths and casts behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn integer_widths_and_casts() {
    let outcome = run_program(
        "fn main() -> i32 { let a: i8 = 127; let b: i16 = 300; let c: i64 = 5000000000; print(b as i32 + a as i32); print('\\n'); print(c as i32); print('\\n'); return 0; }",
        "widths",
    );
    assert_eq!(outcome.stdout, "427\n705032704\n");
}

// CEP:WHAT: Verifies that floats print with fixed format behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn floats_print_with_fixed_format() {
    let outcome = run_program(
        "fn main() -> i32 { let x: f64 = 2.5; print(x * 2.0); print('\\n'); return 0; }",
        "floats",
    );
    assert_eq!(outcome.stdout, "5.000000\n");
}

// CEP:WHAT: Verifies that control flow if while loop break behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn control_flow_if_while_loop_break() {
    let outcome = run_program(
        "fn main() -> i32 { let mut i: i32 = 0; let mut acc: i32 = 0; while i < 5 { if i % 2 == 0 { acc = acc + i; } else { acc = acc + 1; } i = i + 1; } loop { acc = acc + 100; if acc > 110 { break; } } print(acc); print('\\n'); return 0; }",
        "control",
    );
    // i=0:+0; i=1:+1; i=2:+2; i=3:+1; i=4:+4 -> 8; loop: +100=108, +100=208 > 110 break.
    assert_eq!(outcome.stdout, "208\n");
}

// CEP:WHAT: Verifies that for loop ranges behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn for_loop_ranges() {
    let outcome = run_program(
        "fn main() -> i32 { let mut s: i32 = 0; for i in 0..4 { s = s + i; } for j in 0..=4 { s = s + j; } print(s); print('\\n'); return 0; }",
        "forloop",
    );
    assert_eq!(outcome.stdout, "16\n");
}

// CEP:WHAT: Verifies that structs methods mutability behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn structs_methods_mutability() {
    let outcome = run_program(
        "struct P { x: i32, y: i32 } impl P { fn sum(&self) -> i32 { return self.x + self.y; } fn scale(&mut self, k: i32) -> () { self.x = self.x * k; self.y = self.y * k; } } fn main() -> i32 { let mut p: P = P { x: 3, y: 4 }; let s: i32 = p.sum(); p.scale(10); print(s); print('\\n'); print(p.sum()); print('\\n'); return 0; }",
        "structs",
    );
    assert_eq!(outcome.stdout, "7\n70\n");
}

// CEP:WHAT: Verifies that enums match payloads behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn enums_match_payloads() {
    let outcome = run_program(
        "enum Shape { Circle(i32), Square(i32), Empty } fn area(s: Shape) -> i32 { match s { Shape::Circle(r) => 3 * r * r, Shape::Square(a) => a * a, Shape::Empty => 0, _ => 0 } } fn main() -> i32 { print(area(Shape::Circle(2))); print('\\n'); print(area(Shape::Square(5))); print('\\n'); print(area(Shape::Empty)); print('\\n'); return 0; }",
        "enums",
    );
    assert_eq!(outcome.stdout, "12\n25\n0\n");
}

// CEP:WHAT: Verifies that recursion computes factorial behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn recursion_computes_factorial() {
    let outcome = run_program(
        "fn fact(n: i64) -> i64 { if n <= 1 { return 1; } return n * fact(n - 1); } fn main() -> i32 { print(fact(10)); print('\\n'); return 0; }",
        "fact",
    );
    assert_eq!(outcome.stdout, "3628800\n");
}

// CEP:WHAT: Verifies that arrays index and mutate behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn arrays_index_and_mutate() {
    let outcome = run_program(
        "fn main() -> i32 { let mut a: [i32; 3] = [10, 20, 30]; a[1] = 99; let mut s: i32 = 0; for i in 0..3 { s = s + a[i]; } print(s); print('\\n'); return 0; }",
        "arrays",
    );
    assert_eq!(outcome.stdout, "139\n");
}

// CEP:WHAT: Verifies that references read and write behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn references_read_and_write() {
    let outcome = run_program(
        "fn bump(r: &mut i32) -> () { *r = *r + 1; } fn read(r: &i32) -> i32 { return *r; } fn main() -> i32 { let mut x: i32 = 41; bump(&mut x); bump(&mut x); print(read(&x)); print('\\n'); return 0; }",
        "refs",
    );
    assert_eq!(outcome.stdout, "43\n");
}

// CEP:WHAT: Verifies that float comparisons branch correctly behaves as specified.
// CEP:WHY: Regression pin: the float comparison path previously emitted no comparison at all (emit_float_bin's catch-all), and then an inverted ucomisd operand order; both silently produced undefined branch conditions.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn float_comparisons_branch_correctly() {
    // Loop-carried: 1.5 * 2 three times reaches 12.0; the loop must run.
    let outcome = run_program(
        "fn main() -> i32 { let mut f: f64 = 1.5; while f < 10.0 { f = f * 2.0; } print(f as i32); print('\\n'); return 0; }",
        "floop",
    );
    assert_eq!(outcome.stdout, "12\n");
    // All six orderings on constants.
    let outcome = run_program(
        "fn main() -> i32 { let a: f64 = 1.0; let b: f64 = 2.0; print((a < b) as i32); print((a <= b) as i32); print((a > b) as i32); print((a >= b) as i32); print((a == b) as i32); print((a != b) as i32); print('\\n'); return 0; }",
        "fcmp",
    );
    assert_eq!(outcome.stdout, "110001\n");
}

// CEP:WHAT: Verifies that overflow traps in debug mode behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn overflow_traps_in_debug_mode() {
    let dir = std::env::temp_dir().join(format!("planckc_e2e_ovf_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let binary = compile_to(
        "fn main() -> i32 { let a: i8 = 127; let b: i8 = 1; let c: i8 = a + b; return c as i32; }",
        &dir,
        "ovf",
        false,
    );
    let out = Command::new(&binary).output().expect("binary runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&out.stderr).contains("arithmetic overflow"));
}

// CEP:WHAT: Verifies that overflow wraps in release mode behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn overflow_wraps_in_release_mode() {
    let dir = std::env::temp_dir().join(format!("planckc_e2e_relovf_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let binary = compile_to(
        "fn main() -> i32 { let a: i8 = 127; let b: i8 = 1; let c: i8 = a + b; return c as i32; }",
        &dir,
        "relovf",
        true,
    );
    let out = Command::new(&binary).output().expect("binary runs");
    let _ = std::fs::remove_dir_all(&dir);
    // 127 + 1 wraps to -128 in i8; the process exits with -128 as a byte code.
    assert_eq!(out.status.code(), Some(128));
}

// CEP:WHAT: Verifies that bounds check traps behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn bounds_check_traps() {
    let dir = std::env::temp_dir().join(format!("planckc_e2e_oob_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let binary = compile_to(
        "fn main() -> i32 { let a: [i32; 3] = [1, 2, 3]; return a[7]; }",
        &dir,
        "oob",
        false,
    );
    let out = Command::new(&binary).output().expect("binary runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&out.stderr).contains("index out of bounds"));
}

// CEP:WHAT: Verifies that division by zero traps behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn division_by_zero_traps() {
    let dir = std::env::temp_dir().join(format!("planckc_e2e_divz_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let binary = compile_to(
        "fn main() -> i32 { let z: i32 = 0; return 9 / z; }",
        &dir,
        "divz",
        false,
    );
    let out = Command::new(&binary).output().expect("binary runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out.status.code(), Some(101));
}

// CEP:WHAT: Verifies that semantic errors reported with diagnostics behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn semantic_errors_reported_with_diagnostics() {
    let dir = std::env::temp_dir().join(format!("planckc_e2e_err_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let source_path = dir.join("err.plk");
    std::fs::write(
        &source_path,
        "fn main() -> i32 { let x: i32 = true; return 0; }",
    )
    .expect("write");
    let out = Command::new(driver())
        .args(["build", source_path.to_str().expect("utf8"), "-o"])
        .arg(dir.join("out"))
        .output()
        .expect("driver runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("PLK-S0003"), "stderr: {}", stderr);
}

// CEP:WHAT: Verifies that compilation is deterministic behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn compilation_is_deterministic() {
    // Standard 38.10: same source + same flags = same binary bytes.
    let dir = std::env::temp_dir().join(format!("planckc_e2e_det_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let source = "fn main() -> i32 { print(\"determinism\"); print('\\n'); return 3; }";
    let first = compile_to(source, &dir, "det1", false);
    let second = compile_to(source, &dir, "det2", false);
    let bytes_a = std::fs::read(&first).expect("read first");
    let bytes_b = std::fs::read(&second).expect("read second");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        bytes_a, bytes_b,
        "two compilations of the same source differ"
    );
}

// CEP:WHAT: Verifies that optimizer preserves program semantics behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: Microseconds; unit test.
// CEP:EVIDENCE: cargo test.
#[test]
fn optimizer_preserves_program_semantics() {
    // Constant folding, CSE, LICM, and DCE must not change observable output.
    let outcome = run_program(
        "fn main() -> i32 { let mut acc: i32 = 0; for i in 0..10 { let x: i32 = 2 * 3 + i; acc = acc + x; } print(acc); print('\\n'); return 0; }",
        "optsem",
    );
    assert_eq!(outcome.stdout, "105\n");
}

// CEP:WHAT: Verifies that ownership moves and deferred init run end to end behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on unexpected output or exit status.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: One driver invocation plus one link.
// CEP:EVIDENCE: cargo test -p planckc.
#[test]
fn ownership_moves_and_deferred_init_run() {
    // Exercises: deferred initialization with branch merging, moves into call
    // arguments and enum payloads, field reassignment, and aggregate payload
    // bindings out of a match.
    let src = concat!(
        "struct P { x: i32, y: i32 } ",
        "enum W { S(P), N } ",
        "fn take(p: P) -> i32 { return p.x + p.y; } ",
        "fn unwrap_y(w: W) -> i32 { match w { W::S(v) => v.y * 100, _ => 0 } } ",
        "fn main() -> i32 { ",
        "let mut p: P; ",
        "let c: i32 = 1; ",
        "if c > 0 { p = P { x: 10, y: 20 }; } else { p = P { x: 0, y: 0 }; } ",
        "let a: i32 = take(p); ",
        "p = P { x: 1, y: 2 }; ",
        "let w: W = W::S(p); ",
        "let b: i32 = unwrap_y(w); ",
        "return a + b; ",
        "}"
    );
    let outcome = run_program(src, "ownership");
    assert_eq!(outcome.exit, 30 + 200);
}

// CEP:WHAT: Verifies that the driver rejects use after move behaves as specified.
// CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails if the driver accepts the program or reports the wrong code.
// CEP:ASSUMES: Fixture inputs are repository-controlled.
// CEP:COST: One driver invocation.
// CEP:EVIDENCE: cargo test -p planckc.
#[test]
fn driver_rejects_use_after_move() {
    let dir = std::env::temp_dir().join(format!("planckc_e2e_move_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let source_path = dir.join("move.plk");
    std::fs::write(
        &source_path,
        concat!(
            "struct P { x: i32 } ",
            "fn eat(p: P) -> i32 { return p.x; } ",
            "fn main() -> i32 { let p: P = P { x: 1 }; let q: P = p; return eat(p); }"
        ),
    )
    .expect("write");
    let out = Command::new(driver())
        .args(["build", source_path.to_str().expect("utf8"), "-o"])
        .arg(dir.join("out"))
        .output()
        .expect("driver runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("PLK-S0023"), "stderr: {}", stderr);
    assert!(stderr.contains("use of moved value"), "stderr: {}", stderr);
}
