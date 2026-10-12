#!/usr/bin/env python3
"""Regenerates the README benchmark charts from an `rsg-bench report` JSON.

Usage: scripts/plot_benchmarks.py REPORT_JSON OUT_DIR

Writes OUT_DIR/s2_saturation_p99.svg (P99 TTFT per offered rate, log scale)
and OUT_DIR/s3_coldstart_memory.svg (frontend RSS and end-to-end cold start).
Standard library only; the SVG style matches the charts first added to the
README in Phase 7.
"""

import json
import math
import sys
from pathlib import Path

FONT = "-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif"
INK = "#1f2937"
ARMS = [
    ("python-default", "Python (default)", "#2563eb"),
    ("python-best", "Python (best)", "#0891b2"),
    ("rust", "Rust", "#ea580c"),
]


def text(x, y, s, size=12, anchor="middle", weight="normal", fill=INK, extra=""):
    s = s.replace("&", "&amp;").replace("<", "&lt;")
    return (
        f"<text{extra} x='{x:.1f}' y='{y:.1f}' font-family='{FONT}' font-size='{size}' "
        f"text-anchor='{anchor}' font-weight='{weight}' fill='{fill}'>{s}</text>"
    )


def legend(y):
    out = []
    for (_, label, color), x in zip(ARMS, (208.5, 366.5, 503.5)):
        out.append(f"<rect x='{x}' y='{y:.1f}' width='14' height='14' fill='{color}' rx='2'/>")
        out.append(text(x + 20, y + 12, label, size=12.5, anchor="start"))
    return out


def s2_chart(report):
    curve = report["scenarios"]["s2_saturation"]["extra"]["curve"]
    labels = [p["label"].removeprefix("rate=") for p in curve]
    series = {
        arm: [p["arms"][arm]["ttft_p99_ms"]["mean"] for p in curve] for arm, _, _ in ARMS
    }
    values = [v for vs in series.values() for v in vs]
    lo, hi = min(30.0, min(values) / 1.2), max(1500.0, max(values) * 1.2)
    top, bottom, left, right = 85, 360, 60, 730

    def y_of(v):
        return bottom - (math.log10(v) - math.log10(lo)) / (math.log10(hi) - math.log10(lo)) * (bottom - top)

    def x_of(i):
        return left + i * (right - left) / (len(labels) - 1)

    out = [
        "<svg xmlns='http://www.w3.org/2000/svg' width='760' height='420' viewBox='0 0 760 420'>",
        "<rect x='0' y='0' width='760' height='420' fill='#ffffff'/>",
        text(380, 24, "Saturation ramp: P99 time-to-first-token (log scale)", size=15, weight="600"),
        *legend(39),
    ]
    for tick in (10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000):
        if lo <= tick <= hi:
            y = y_of(tick)
            out.append(f"<line x1='{left}' y1='{y:.1f}' x2='{right}' y2='{y:.1f}' stroke='#e5e7eb' stroke-width='1'/>")
            out.append(text(50, y + 4, str(tick), anchor="end"))
    out.append(f"<line x1='{left}' y1='{top}' x2='{left}' y2='{bottom}' stroke='#9ca3af' stroke-width='1'/>")
    out.append(f"<line x1='{left}' y1='{bottom}' x2='{right}' y2='{bottom}' stroke='#9ca3af' stroke-width='1'/>")
    for i, label in enumerate(labels):
        out.append(text(x_of(i), 382, label))
    out.append(text(395, 402, "Requests/sec", size=13, weight="500"))
    out.append(text(16, 222.5, "P99 TTFT (ms)", size=13, weight="500", extra=" transform='rotate(-90 16 222.5)'"))
    for arm, _, color in ARMS:
        pts = [(x_of(i), y_of(v)) for i, v in enumerate(series[arm])]
        out.append(
            "<polyline points='" + " ".join(f"{x:.1f},{y:.1f}" for x, y in pts)
            + f"' fill='none' stroke='{color}' stroke-width='2.5'/>"
        )
        out.extend(f"<circle cx='{x:.1f}' cy='{y:.1f}' r='4' fill='{color}'/>" for x, y in pts)
    # Call out Rust's worst point when it is well above Python (best) at that rate.
    rust, best = series["rust"], series["python-best"]
    i = max(range(len(rust)), key=lambda k: rust[k] / best[k])
    if rust[i] > 2 * best[i]:
        out.append(text(
            x_of(i) - 10, y_of(rust[i]) + 20, f"{rust[i]:.0f}ms (~{rust[i] / best[i]:.0f}× Python)",
            anchor="end", weight="600", fill="#ea580c",
        ))
    out.append("</svg>")
    return "\n".join(out) + "\n"


def bar_panel(x0, title, values, fmt, step):
    """One bar panel spanning x0..x0+300, baseline y=330, 4 gridlines of `step`."""
    top_value = step * 4
    while max(values) > top_value:
        step *= 2
        top_value = step * 4
    out = [text(x0 + 150, 84, title, size=13, weight="500")]
    out.append(f"<line x1='{x0}' y1='330' x2='{x0 + 300}' y2='330' stroke='#9ca3af' stroke-width='1'/>")
    for k in range(5):
        y = 330 - k * 60
        out.append(f"<line x1='{x0}' y1='{y:.1f}' x2='{x0 + 300}' y2='{y:.1f}' stroke='#f1f5f9' stroke-width='1'/>")
        out.append(text(x0 - 8, y + 4, f"{step * k:g}", size=11, anchor="end"))
    for i, ((_, _, color), v) in enumerate(zip(ARMS, values)):
        h = v / top_value * 240
        x = x0 + 25 + i * 100
        out.append(f"<rect x='{x:.1f}' y='{330 - h:.1f}' width='50.0' height='{h:.1f}' fill='{color}' rx='3'/>")
        out.append(text(x + 25, 330 - h - 6, fmt(v), size=11.5, weight="600", fill=color))
    return out


def s3_chart(report):
    arms = report["scenarios"]["s3_coldstart"]["arms"]
    rss_mb = [arms[a]["metrics"]["frontend_rss_bytes"]["mean"] / 1e6 for a, _, _ in ARMS]
    ready_s = [arms[a]["metrics"]["e2e_ready_s"]["mean"] for a, _, _ in ARMS]
    out = [
        "<svg xmlns='http://www.w3.org/2000/svg' width='760' height='400' viewBox='0 0 760 400'>",
        "<rect x='0' y='0' width='760' height='400' fill='#ffffff'/>",
        text(380, 24, "Cold start & frontend memory", size=15, weight="600"),
        *legend(41),
        *bar_panel(60, "Frontend RSS (MB, lower is better)", rss_mb, lambda v: f"{v:.0f} MB", 800),
        *bar_panel(420, "End-to-end cold start (s, lower is better)", ready_s, lambda v: f"{v:.2f} s", 3),
        "</svg>",
    ]
    return "\n".join(out) + "\n"


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    report = json.loads(Path(sys.argv[1]).read_text())
    out_dir = Path(sys.argv[2])
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "s2_saturation_p99.svg").write_text(s2_chart(report))
    (out_dir / "s3_coldstart_memory.svg").write_text(s3_chart(report))


if __name__ == "__main__":
    main()
