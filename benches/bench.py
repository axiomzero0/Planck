#!/usr/bin/env python3
"""Benchmark harness for the Planck compiler.

Measures, per benchmark program (benches/*.plk with a C mirror in benches/c/):
  * compile time   -- median wall time of the full toolchain invocation
                      (planckc build incl. as+cc link; gcc compile+link)
  * run time       -- median wall time of N executions of the built binary
  * text size      -- .text section size of the built binary
  * correctness    -- every config's stdout must match the gcc -O2 reference

Configurations:
  c_o0            gcc -O0                     (no-optimization baseline)
  c_o2            gcc -O2                     (production C baseline)
  planck          planckc build               (opt pipeline ON, overflow checks ON)
  planck_release  planckc build --release     (opt pipeline ON, checks OFF)

Usage:  cargo build --release && python3 benches/bench.py [--runs N] [--quick]
Output: benches/results.json plus a markdown summary on stdout.
"""

import argparse
import json
import pathlib
import platform
import statistics
import subprocess
import sys
import tempfile
import time

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parent
PLANCKC = REPO / "target" / "release" / "planckc"

BENCHMARKS = ["fib", "primes", "sort", "mandelbrot", "nqueens", "matmul"]


def run_cmd(cmd, **kwargs):
    return subprocess.run(cmd, check=True, **kwargs)


def timed_compile(cmd, out_path, repeats):
    """Compiles `repeats` times to distinct throwaway paths; returns median ms."""
    times = []
    for i in range(repeats):
        target = out_path.with_name(f"{out_path.name}_{i}")
        t0 = time.perf_counter()
        run_cmd(cmd + ["-o", str(target)])
        times.append((time.perf_counter() - t0) * 1000.0)
        target.unlink(missing_ok=True)
    return statistics.median(times[1:]) if repeats > 1 else times[0]


def timed_run(binary, expect_stdout, repeats):
    """Runs the binary `repeats` times; returns (median_ms, outputs_match)."""
    times = []
    ok = True
    for _ in range(repeats):
        t0 = time.perf_counter()
        proc = subprocess.run([str(binary)], capture_output=True, text=True)
        times.append((time.perf_counter() - t0) * 1000.0)
        if proc.returncode != 0 or proc.stdout != expect_stdout:
            ok = False
    return statistics.median(times), ok


def text_size(binary):
    try:
        out = subprocess.run(
            ["size", str(binary)], capture_output=True, text=True, check=True
        ).stdout.strip().splitlines()
        fields = out[-1].split()
        return int(fields[0])  # text column
    except Exception:
        return None


def gcc_version():
    out = subprocess.run(
        ["cc", "--version"], capture_output=True, text=True
    ).stdout.splitlines()
    return out[0].strip() if out else "unknown"


def cpu_model():
    try:
        for line in pathlib.Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=7, help="timed executions per config")
    ap.add_argument("--compiles", type=int, default=6, help="timed compiles per config")
    ap.add_argument("--quick", action="store_true", help="1 run / 2 compiles")
    args = ap.parse_args()
    runs = 1 if args.quick else args.runs
    compiles = 2 if args.quick else args.compiles

    if not PLANCKC.exists():
        sys.exit("planckc not built: run `cargo build --release` first")

    workdir = pathlib.Path(tempfile.mkdtemp(prefix="planck-bench-"))
    results = {
        "meta": {
            "date": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
            "cpu": cpu_model(),
            "kernel": platform.release(),
            "gcc": gcc_version(),
            "pipeline": "planck-opt-0.2.0",
            "timed_runs": runs,
            "timed_compiles": compiles,
        },
        "benchmarks": {},
    }

    print(f"host: {results['meta']['cpu']}")
    print(f"gcc:  {results['meta']['gcc']}")
    print(f"runs: {runs} per config, compiles: {compiles} per config\n")

    for bench in BENCHMARKS:
        plk = HERE / f"{bench}.plk"
        csrc = HERE / "c" / f"{bench}.c"
        entry = {"configs": {}}

        configs = {
            "c_o0": ["gcc", "-O0", str(csrc)],
            "c_o2": ["gcc", "-O2", str(csrc)],
            "planck": [str(PLANCKC), "build", str(plk)],
            "planck_release": [str(PLANCKC), "build", str(plk), "--release"],
        }

        # Reference output: gcc -O2.
        ref_bin = workdir / f"ref_{bench}"
        run_cmd(configs["c_o2"] + ["-o", str(ref_bin)])
        ref_out = subprocess.run(
            [str(ref_bin)], capture_output=True, text=True, check=True
        ).stdout
        entry["checksum_stdout"] = ref_out.strip()

        for name, cmd in configs.items():
            bin_path = workdir / f"{bench}_{name}"
            compile_ms = timed_compile(cmd, bin_path, compiles)
            final_bin = workdir / f"final_{bench}_{name}"
            run_cmd(cmd + ["-o", str(final_bin)])
            run_ms, ok = timed_run(final_bin, ref_out, runs)
            entry["configs"][name] = {
                "compile_ms": round(compile_ms, 1),
                "run_ms": round(run_ms, 2),
                "text_bytes": text_size(final_bin),
                "stdout_matches_reference": ok,
            }
            print(
                f"  {bench:>10s} {name:>15s}: compile {compile_ms:7.1f} ms"
                f"  run {run_ms:8.2f} ms  text {entry['configs'][name]['text_bytes']} B"
                f"  {'OK' if ok else 'MISMATCH'}"
            )
        results["benchmarks"][bench] = entry
        print()

    (HERE / "results.json").write_text(json.dumps(results, indent=2) + "\n")

    # Markdown summary.
    lines = [
        "| benchmark | c -O0 | c -O2 | planck (checked) | planck --release |",
        "|---|---|---|---|---|",
    ]
    geo = {name: 1.0 for name in configs}
    n = 0
    for bench in BENCHMARKS:
        cfg = results["benchmarks"][bench]["configs"]
        row = [bench]
        for name in ["c_o0", "c_o2", "planck", "planck_release"]:
            row.append(f"{cfg[name]['run_ms']:.1f} ms")
        lines.append("| " + " | ".join(row) + " |")
        base = cfg["c_o2"]["run_ms"]
        for name in configs:
            geo[name] *= cfg[name]["run_ms"] / base
        n += 1
    print("\n### Run time (median)\n")
    print("\n".join(lines))
    print("\n### Geometric mean run time relative to gcc -O2 = 1.00\n")
    for name in ["c_o0", "c_o2", "planck", "planck_release"]:
        print(f"  {name:>15s}: {geo[name] ** (1.0 / n):.2f}x")
    print(f"\nresults written to {HERE / 'results.json'}")
    print(f"workdir: {workdir}")


if __name__ == "__main__":
    main()
