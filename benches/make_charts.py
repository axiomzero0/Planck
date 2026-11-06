#!/usr/bin/env python3
"""Renders doc/benchmarks.png from benches/results.json (produced by bench.py).

Three panels: run time relative to gcc -O2 (log), absolute compile time,
and binary text size. Colors follow the charts style guide: blue family for
the Planck configurations, gray family for the C baselines.
"""

import json
import pathlib

import matplotlib
import matplotlib.pyplot as plt
import numpy as np

HERE = pathlib.Path(__file__).resolve().parent
RESULTS = json.loads((HERE / "results.json").read_text())

BENCHES = ["fib", "primes", "sort", "mandelbrot", "nqueens", "matmul"]
CONFIGS = ["c_o0", "c_o2", "planck", "planck_release"]
LABELS = {
    "c_o0": "gcc -O0",
    "c_o2": "gcc -O2",
    "planck": "planckc (checked)",
    "planck_release": "planckc --release",
}
COLORS = {
    "c_o0": "#D1D5DB",
    "c_o2": "#6B7280",
    "planck": "#3B82F6",
    "planck_release": "#93C5FD",
}

plt.rcParams.update({
    "font.sans-serif": ["DejaVu Sans"],
    "axes.unicode_minus": False,
    "figure.facecolor": "#FFFFFF",
    "axes.facecolor": "#FFFFFF",
    "axes.edgecolor": "#E5E7EB",
    "axes.linewidth": 0.8,
    "axes.spines.top": False,
    "axes.spines.right": False,
    "axes.grid": False,
    "xtick.major.size": 0,
    "ytick.major.size": 0,
    "xtick.labelsize": 9,
    "ytick.labelsize": 9,
    "legend.frameon": False,
    "legend.fontsize": 9,
    "figure.dpi": 200,
    "savefig.dpi": 200,
    "savefig.facecolor": "#FFFFFF",
})

G300, G400, G900 = "#D1D5DB", "#9CA3AF", "#111827"


def clean_axis(ax):
    ax.spines["top"].set_visible(False)
    ax.spines["right"].set_visible(False)
    ax.yaxis.grid(True, alpha=0.08, color=G300)
    ax.set_axisbelow(True)


def grouped(ax, values_by_config, title, ylabel, log=False):
    x = np.arange(len(BENCHES))
    n = len(CONFIGS)
    width = 0.7 / n
    all_max = 0.0
    for i, cfg in enumerate(CONFIGS):
        vals = [values_by_config[cfg][b] for b in BENCHES]
        offset = (i - n / 2 + 0.5) * width
        ax.bar(
            x + offset, vals, width=width * 0.85, color=COLORS[cfg],
            label=LABELS[cfg], zorder=3, edgecolor="white", linewidth=0.3,
        )
        all_max = max(all_max, max(vals))
    ax.set_title(title, loc="left", fontsize=12.5, fontweight="bold")
    ax.set_xticks(x)
    ax.set_xticklabels(BENCHES)
    ax.set_ylabel(ylabel, fontsize=10)
    if log:
        ax.set_yscale("log")
        ax.set_ylim(0.5, all_max * 1.6)
    else:
        ax.set_ylim(0, all_max * 1.18)
    clean_axis(ax)


def main():
    ratios = {c: {} for c in CONFIGS}
    compiles = {c: {} for c in CONFIGS}
    texts = {c: {} for c in CONFIGS}
    for bench in BENCHES:
        cfg = RESULTS["benchmarks"][bench]["configs"]
        base = cfg["c_o2"]["run_ms"]
        for c in CONFIGS:
            ratios[c][bench] = cfg[c]["run_ms"] / base
            compiles[c][bench] = cfg[c]["compile_ms"]
            texts[c][bench] = cfg[c]["text_bytes"] / 1024.0

    fig, axes = plt.subplots(1, 3, figsize=(17, 5.4), constrained_layout=True)

    grouped(
        axes[0], ratios,
        "Run time, relative to gcc -O2 (log scale)",
        "run time / gcc -O2", log=True,
    )
    axes[0].axhline(1.0, color=G400, linewidth=1.0, linestyle="--", zorder=2)

    grouped(
        axes[1], compiles,
        "Compile time (ms)",
        "compile time (ms)",
    )

    grouped(
        axes[2], texts,
        "Binary .text size, KB (log scale)",
        "text size (KB)", log=True,
    )

    handles, labels = axes[0].get_legend_handles_labels()
    fig.legend(
        handles, labels, loc="upper center", bbox_to_anchor=(0.5, 1.08),
        ncol=4, frameon=False,
    )
    out = HERE.parent / "doc" / "benchmarks.png"
    fig.savefig(out, dpi=200, facecolor="white", bbox_inches="tight")
    plt.close(fig)
    print(f"saved {out}")


if __name__ == "__main__":
    main()
