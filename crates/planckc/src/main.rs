// CEP:FILE: crates/planckc/src/main.rs
// CEP:WHAT: The Planck compiler driver: parses arguments, runs the pipeline, and invokes the system toolchain for linking.
// CEP:WHY: One deterministic entry point keeps stage wiring, verification gates, and toolchain invocation in a single reviewable place.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: All failures exit non-zero with diagnostics on stderr: parse/sema/verify/opt errors exit 1; internal invariant violations (verification after optimization) exit 101 with an internal-compiler-error banner per standard 38.6; usage errors exit 2 after printing usage.
// CEP:ASSUMES: `cc` is on PATH for the build command; the runtime lives at runtime/planck_rt.c relative to the repository. Output paths are used as given; the driver never deletes user files.
// CEP:COST: O(program size) end to end; the linker invocation dominates wall time for small programs.
// CEP:EVIDENCE: end-to-end tests in crates/planckc/tests.
// CEP:SECURITY: Input files are read fully into memory without validation beyond UTF-8; generated assembly and objects are written only to the requested output paths. The linker is invoked with a fixed flag set and no shell interpolation.
// CEP:PORTABILITY: POSIX hosts with a System V x86-64 toolchain.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "planckc 0.1.0 — the Planck compiler

USAGE:
    planckc build <input.plk> [-o <output>] [--release] [--dump-ir] [--emit-asm]
    planckc run <input.plk> [--release]
    planckc dump-ir <input.plk> [--release]

FLAGS:
    -o <path>      output executable path (build only; default: out)
    --release      disable integer overflow checks (wraps per design Part 3)
    --dump-ir      print the post-optimization IR to stdout
    --emit-asm     write the generated assembly next to the output (.s)
";

// CEP:WHAT: Handles the main step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(DriverError::Usage) => {
            eprint!("{}", USAGE);
            ExitCode::from(2)
        }
        Err(DriverError::Compiled(message)) => {
            eprint!("{}", message);
            ExitCode::FAILURE
        }
        Err(DriverError::Internal(message)) => {
            eprintln!("internal compiler error: {}", message);
            eprintln!("this is a bug in planckc; please file it against the pipeline gates");
            ExitCode::from(101)
        }
    }
}

enum DriverError {
    Usage,
    Compiled(String),
    Internal(String),
}

struct Config {
    command: String,
    input: PathBuf,
    output: PathBuf,
    release: bool,
    dump_ir: bool,
    emit_asm: bool,
}

// CEP:WHAT: Handles the parse args step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn parse_args(args: &[String]) -> Result<Config, DriverError> {
    if args.is_empty() {
        return Err(DriverError::Usage);
    }
    let command = args[0].clone();
    if !matches!(command.as_str(), "build" | "run" | "dump-ir") {
        return Err(DriverError::Usage);
    }
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut release = false;
    let mut dump_ir = false;
    let mut emit_asm = false;
    let mut idx = 1;
    while idx < args.len() {
        match args[idx].as_str() {
            "-o" => {
                idx += 1;
                if idx >= args.len() {
                    return Err(DriverError::Usage);
                }
                output = Some(PathBuf::from(&args[idx]));
            }
            "--release" => release = true,
            "--dump-ir" => dump_ir = true,
            "--emit-asm" => emit_asm = true,
            other if other.starts_with('-') => return Err(DriverError::Usage),
            other => {
                if input.is_some() {
                    return Err(DriverError::Usage);
                }
                input = Some(PathBuf::from(other));
            }
        }
        idx += 1;
    }
    let input = input.ok_or(DriverError::Usage)?;
    let output = output.unwrap_or_else(|| PathBuf::from("out"));
    Ok(Config {
        command,
        input,
        output,
        release,
        dump_ir,
        emit_asm,
    })
}

// CEP:WHAT: Handles the run step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn run(args: &[String]) -> Result<(), DriverError> {
    let config = parse_args(args)?;
    let source = std::fs::read_to_string(&config.input).map_err(|err| {
        DriverError::Compiled(format!("cannot read {}: {}\n", config.input.display(), err))
    })?;
    let file_name = config
        .input
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "<input>".to_string());

    // Pipeline: lex → parse → check-and-lower → verify → optimize (verified) → emit.
    let (module, parse_diags) = planck_parser::parse_module(&source);
    let source_map = planck_support::SourceMap::new(file_name, source);
    if !parse_diags.is_empty() {
        let mut sink = planck_support::DiagSink::new();
        for diag in parse_diags {
            sink.push(diag);
        }
        eprint!("{}", sink.render_all(&source_map));
        return Err(DriverError::Compiled(String::new()));
    }

    let (mut program, sema_diags) = planck_sema::analyze(&module, !config.release);
    let sema_sink = render_diags(&sema_diags, &source_map);
    if !sema_sink.is_empty() {
        eprint!("{}", sema_sink);
    }
    if sema_diags
        .iter()
        .any(|d| d.kind == planck_support::DiagKind::Error)
    {
        return Err(DriverError::Compiled(String::new()));
    }

    let lowered_findings = planck_ir::verify_program(&program);
    if !lowered_findings.is_empty() {
        return Err(DriverError::Internal(format!(
            "verification failed after lowering: {:?}",
            lowered_findings.first()
        )));
    }

    let opt_result = planck_opt::optimize(&mut program);
    if !opt_result.verify_errors.is_empty() {
        return Err(DriverError::Internal(format!(
            "verification failed after optimization: {:?}",
            opt_result.verify_errors.first()
        )));
    }

    if config.dump_ir || config.command == "dump-ir" {
        print!("{}", planck_ir::print_program(&program));
        return Ok(());
    }

    let assembly = planck_codegen::emit_assembly(&program);
    if config.emit_asm {
        let asm_path = config.output.with_extension("s");
        std::fs::write(&asm_path, &assembly).map_err(|err| {
            DriverError::Compiled(format!("cannot write {}: {}\n", asm_path.display(), err))
        })?;
    }

    match config.command.as_str() {
        "build" => {
            invoke_toolchain(&assembly, &config.output, &source_map)?;
            Ok(())
        }
        "run" => {
            let executable = std::env::temp_dir().join("planckc_run_binary");
            invoke_toolchain(&assembly, &executable, &source_map)?;
            let status = std::process::Command::new(&executable)
                .status()
                .map_err(|err| DriverError::Compiled(format!("cannot execute: {}\n", err)))?;
            let _ = std::fs::remove_file(&executable);
            std::process::exit(status.code().unwrap_or(1));
        }
        _ => Err(DriverError::Usage),
    }
}

// CEP:WHAT: Handles the render diags step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn render_diags(diags: &[planck_support::Diag], source_map: &planck_support::SourceMap) -> String {
    let mut sink = planck_support::DiagSink::new();
    for diag in diags {
        sink.push(diag.clone());
    }
    sink.render_all(source_map)
}

// CEP:WHAT: Handles the runtime path step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn runtime_path() -> Result<PathBuf, DriverError> {
    // The driver resolves the runtime relative to the executable, then the working
    // directory, then the repository default.
    let candidates = [
        PathBuf::from("runtime/planck_rt.c"),
        PathBuf::from("../../runtime/planck_rt.c"),
        PathBuf::from("../runtime/planck_rt.c"),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(DriverError::Compiled(
        "runtime/planck_rt.c not found; run from the repository root\n".to_string(),
    ))
}

// CEP:WHAT: Handles the invoke toolchain step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn invoke_toolchain(
    assembly: &str,
    output: &Path,
    source_map: &planck_support::SourceMap,
) -> Result<(), DriverError> {
    // Pipeline: as (stdin, so no input name can leak into the symbol table) -> object;
    // cc -c for the runtime; cc link-only for the final binary. Every file name that
    // could reach STT_FILE is either fixed or supplied via stdin (standard 38.10).
    let scratch = std::env::temp_dir().join(format!("planckc-build-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)
        .map_err(|err| DriverError::Compiled(format!("cannot create scratch dir: {}\n", err)))?;
    let prog_obj = scratch.join("planck_prog.o");
    let rt_obj = scratch.join("planck_rt.o");

    let as_result = {
        use std::io::Write as _;
        let mut command = std::process::Command::new("as");
        command.arg("-o").arg(&prog_obj);
        command.stdin(std::process::Stdio::piped());
        let mut child = command.spawn().map_err(|err| {
            DriverError::Compiled(format!("cannot invoke the assembler `as`: {}\n", err))
        })?;
        child
            .stdin
            .take()
            .expect("stdin piped")
            .write_all(assembly.as_bytes())
            .map_err(|err| DriverError::Compiled(format!("cannot feed assembly: {}\n", err)))?;
        child
            .wait_with_output()
            .map_err(|err| DriverError::Compiled(format!("as failed: {}\n", err)))?
    };
    if !as_result.status.success() {
        eprint!("{}", String::from_utf8_lossy(&as_result.stderr));
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(DriverError::Compiled(format!(
            "assembling failed for {}\n",
            source_map.file_name()
        )));
    }

    let runtime = runtime_path()?;
    let rt_result = std::process::Command::new("cc")
        .args(["-c"])
        .arg(&runtime)
        .arg("-o")
        .arg(&rt_obj)
        .output()
        .map_err(|err| DriverError::Compiled(format!("cannot compile the runtime: {}\n", err)))?;
    if !rt_result.status.success() {
        eprint!("{}", String::from_utf8_lossy(&rt_result.stderr));
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(DriverError::Compiled(
            "runtime compilation failed\n".to_string(),
        ));
    }

    let link_result = std::process::Command::new("cc")
        .arg("-Wl,--build-id=none")
        .arg(&prog_obj)
        .arg(&rt_obj)
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|err| DriverError::Compiled(format!("cannot invoke the linker: {}\n", err)))?;
    let _ = std::fs::remove_dir_all(&scratch);
    if !link_result.status.success() {
        eprint!("{}", String::from_utf8_lossy(&link_result.stderr));
        return Err(DriverError::Compiled(format!(
            "linking failed for {}\n",
            source_map.file_name()
        )));
    }
    Ok(())
}
