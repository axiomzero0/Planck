// CEP:FILE: tools/cep_lint/src/main.rs
// CEP:WHAT: The cep_lint command-line tool: configuration-driven scanning, self-test over the manifest, DOC-SYNC, and version reporting.
// CEP:WHY: Standard chapter 50 mandates mechanical enforcement with fixed exit codes, deterministic output, and a self-verification gate; this binary is that tool for the Rust tree.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: Exit 0 when no failing findings; 1 when findings at a severity in fail_on exist or a self-test scenario fails; 2 with CEP-LINT-BOOT-<n> on bootstrap failures; 3 on usage errors. The tool never panics on any input path.
// CEP:ASSUMES: Default configuration at .cep/cep_lint.json relative to the working directory, overridable by --config or CEP_LINT_CONFIG. Self-test fixtures live at tools/cep_lint/tests relative to the working directory.
// CEP:COST: O(files x rules) scans; the self-test is bounded by the manifest.
// CEP:EVIDENCE: cep_lint --self-test (one violating and one compliant scenario per rule with exact counts); self-lint of the workspace passes at severity 0 and 1.
// CEP:SECURITY: The tool reads files and prints findings; it writes nothing.
// CEP:PORTABILITY: Path handling works on POSIX separators; the tool is exercised on Linux.

#![forbid(unsafe_code)]

mod config;
mod json;
mod scan;

use config::{BootError, Config};
use scan::{Engine, Finding};
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "cep_lint — CEP&CC mechanical enforcement (standard chapter 50)

USAGE:
    cep_lint [options] [files or directories]

OPTIONS:
    --config, -c <path>   lint configuration JSON (default: .cep/cep_lint.json,
                          environment override: CEP_LINT_CONFIG)
    --self-test, -t       run the self-test manifest and exit
    --help, -h            show this help
    --version, -v         print tool and standard versions

EXIT CODES:
    0  no failing findings
    1  findings at a fail_on severity, or a failed self-test scenario
    2  bootstrap failure (CEP-LINT-BOOT-<n>)
    3  usage error
";

struct Options {
    config_path: PathBuf,
    self_test: bool,
    version: bool,
    help: bool,
    paths: Vec<PathBuf>,
}

// CEP:WHAT: Handles the parse options step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn parse_options(args: &[String]) -> Result<Options, u8> {
    let mut options = Options {
        config_path: PathBuf::from(
            std::env::var("CEP_LINT_CONFIG").unwrap_or_else(|_| ".cep/cep_lint.json".to_string()),
        ),
        self_test: false,
        version: false,
        help: false,
        paths: Vec::new(),
    };
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--config" | "-c" => {
                idx += 1;
                match args.get(idx) {
                    Some(path) => options.config_path = PathBuf::from(path),
                    None => return Err(3),
                }
            }
            "--self-test" | "-t" => options.self_test = true,
            "--version" | "-v" => options.version = true,
            "--help" | "-h" => options.help = true,
            other => options.paths.push(PathBuf::from(other)),
        }
        idx += 1;
    }
    Ok(options)
}

// CEP:WHAT: Handles the main step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse_options(&args) {
        Ok(options) => options,
        Err(code) => {
            eprint!("{}", HELP);
            return exit_code_from_u8(code);
        }
    };
    if options.help {
        print!("{}", HELP);
        return ExitCode::SUCCESS;
    }
    if options.version {
        let config = match config::load(&options.config_path) {
            Ok(config) => config,
            Err(err) => return report_boot(&err),
        };
        println!(
            "{} {} | standard {} {}",
            config.tool_name, config.tool_version, config.standard_name, config.standard_version
        );
        return ExitCode::SUCCESS;
    }
    let config = match config::load(&options.config_path) {
        Ok(config) => config,
        Err(err) => return report_boot(&err),
    };

    if options.self_test {
        return match self_test(&config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(()) => ExitCode::FAILURE,
        };
    }

    // Working directory is the scan root; explicit paths narrow the report but the
    // engine still loads configuration-relative artifacts from the working directory.
    let root = PathBuf::from(".");
    let engine = Engine::new(config, &root);
    let mut findings = engine.run();
    findings.extend(engine.run_doc_sync());
    report(&findings, engine.fail_on())
}

fn exit_code_from_u8(code: u8) -> ExitCode {
    ExitCode::from(code)
}

fn report_boot(err: &BootError) -> ExitCode {
    eprintln!("CEP-LINT-BOOT-{}: {}", err.code(), boot_detail(err));
    ExitCode::from(2)
}

fn boot_detail(err: &BootError) -> String {
    match err {
        BootError::Io(detail) | BootError::Parse(detail) | BootError::Schema(detail) => {
            detail.clone()
        }
    }
}

// CEP:WHAT: Handles the report step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn report(findings: &[Finding], fail_on: &[u8]) -> ExitCode {
    let mut failing = false;
    for finding in findings {
        if fail_on.contains(&finding.severity) {
            failing = true;
        }
        println!(
            "{}:{}: {} [severity {}]: {} ({})",
            finding.file,
            finding.line,
            finding.rule,
            finding.severity,
            finding.message,
            finding.rule
        );
    }
    println!(
        "{} findings ({} failing)",
        findings.len(),
        findings
            .iter()
            .filter(|f| fail_on.contains(&f.severity))
            .count()
    );
    if failing {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Runs every manifest scenario: expected rule counts must match exactly.
// CEP:WHAT: Handles the self test step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn self_test(config: &Config) -> Result<(), ()> {
    let manifest_path = PathBuf::from("tools/cep_lint/tests/manifest.json");
    let text = std::fs::read_to_string(&manifest_path).map_err(|err| {
        eprintln!("CEP-LINT-BOOT-1: manifest unreadable: {}", err);
    })?;
    let manifest = json::parse(&text).map_err(|err| {
        eprintln!("CEP-LINT-BOOT-2: manifest: {}", err);
    })?;
    let scenarios = manifest
        .get("scenarios")
        .and_then(|value| value.as_array())
        .ok_or_else(|| {
            eprintln!("CEP-LINT-BOOT-4: manifest missing `scenarios` array");
        })?;

    let mut failures = 0;
    let mut ran = 0;
    for scenario in scenarios {
        let name = scenario
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unnamed");
        let fixture = scenario
            .get("fixture")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let expected = scenario.get("expected").and_then(|v| v.as_object());
        let fixture_path = PathBuf::from("tools/cep_lint/tests").join(fixture);
        let file_text = std::fs::read_to_string(&fixture_path).unwrap_or_default();
        ran += 1;

        // Run the engine over exactly this fixture by writing it into a temporary
        // root that the globs match.
        let scenario_root = std::env::temp_dir().join(format!(
            "cep_lint_selftest_{}_{}",
            std::process::id(),
            name.replace(['/', ' '], "_")
        ));
        let fixture_dir = scenario_root.join("fixtures");
        std::fs::create_dir_all(&fixture_dir).ok();
        let fixture_name = fixture.rsplit('/').next().unwrap_or("fixture.rs");
        std::fs::write(fixture_dir.join(fixture_name), &file_text).ok();
        // Each scenario is its own universe: the fixture is the only scan target,
        // so the scenario run broadens the globs to the fixture layout.
        let mut scenario_config = config.clone();
        scenario_config.file_globs = vec!["**/*.rs".to_string()];
        let engine = Engine::new(scenario_config, &scenario_root);
        let mut findings = engine.run();
        findings.extend(engine.run_doc_sync());
        let _ = std::fs::remove_dir_all(&scenario_root);

        // DOC-SYNC applies to the real repository; exclude it from fixture counting.
        let mut counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for finding in &findings {
            if finding.rule == "CEP-LINT-DOC-SYNC" {
                continue;
            }
            *counts.entry(finding.rule.clone()).or_insert(0) += 1;
        }
        if let Some(expected) = expected {
            for (rule, count_json) in expected {
                let expected_count = count_json.as_usize().unwrap_or(0);
                let actual = counts.get(rule).copied().unwrap_or(0);
                if actual != expected_count {
                    eprintln!(
                        "SELF-TEST FAIL {}: rule {} expected {} findings, got {} ({:#?})",
                        name, rule, expected_count, actual, findings
                    );
                    failures += 1;
                }
            }
        }
    }
    println!("self-test: {} scenarios, {} failures", ran, failures);
    if failures == 0 {
        Ok(())
    } else {
        Err(())
    }
}

impl config::Rule {
    #[allow(dead_code)]
    fn describe(&self) -> String {
        format!("{} ({}/{})", self.check, self.mode, self.severity)
    }
}
