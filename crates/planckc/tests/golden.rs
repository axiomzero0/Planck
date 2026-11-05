// CEP:FILE: crates/planckc/tests/golden.rs
// CEP:WHAT: Golden IR test: the post-optimization dump for a fixed program must match the committed golden byte-for-byte.
// CEP:WHY: Standard 38.20 makes pass-order and pass-body changes semantic events; a golden file turns silent drift into a reviewable diff.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: The test fails when the IR dump differs from the golden; update the golden only with a reviewed pipeline change.
// CEP:ASSUMES: The driver binary is available via CARGO_BIN_EXE_planckc; the golden was produced with pipeline planck-opt-0.1.0.
// CEP:COST: One driver invocation; microseconds of IR work.
// CEP:EVIDENCE: cargo test -p planckc --test golden.
// CEP:SECURITY: Repository-controlled fixtures only.
// CEP:PORTABILITY: IR text is target-independent.

use std::path::Path;
use std::process::Command;

// CEP:WHAT: Compiles the golden fixture and compares the dump byte-for-byte.
// CEP:WHY: Pins the pipeline output so pass changes surface as reviewed diffs.
// CEP:STATUS: complete
// CEP:FAILURE: The test fails on any IR drift against the committed golden.
// CEP:ASSUMES: The driver binary is available via CARGO_BIN_EXE_planckc.
// CEP:COST: One driver invocation.
// CEP:EVIDENCE: cargo test -p planckc --test golden.
#[test]
fn optimized_ir_matches_golden() {
    let source = "\
struct Accumulator {
    total: i32,
    steps: i32,
}

impl Accumulator {
    fn record(&mut self, value: i32) -> () {
        self.total = self.total + value;
        self.steps = self.steps + 1;
    }
}

fn main() -> i32 {
    let mut acc: Accumulator = Accumulator { total: 0, steps: 0 };
    for i in 1..=4 {
        acc.record(i);
    }
    print(acc.total);
    print('\\n');
    return acc.total;
}
";
    let dir = std::env::temp_dir().join(format!("planckc_golden_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let source_path = dir.join("golden.plk");
    std::fs::write(&source_path, source).expect("write fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_planckc"))
        .args(["dump-ir", source_path.to_str().expect("utf8 path")])
        .output()
        .expect("driver runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "dump-ir failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let actual = String::from_utf8(out.stdout).expect("utf8 IR");
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/accumulator.ir");
    let golden = std::fs::read_to_string(&golden_path).expect("golden file present");
    assert_eq!(
        actual, golden,
        "IR drift against the committed golden; update it only with a reviewed pipeline change"
    );
}
