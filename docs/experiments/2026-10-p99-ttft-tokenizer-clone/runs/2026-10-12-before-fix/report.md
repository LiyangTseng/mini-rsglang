# Frontend Benchmark Report

## Summary

No summary lines.

## s1_cancel

| Arm | OK | Failed |
|---|---|---|
| rust | 5 | 0 |

**Headline metrics:**

| Arm | Metric | Mean | 95% CI |
|---|---|---|---|
| rust | cancelled | 562.6000 | [560.7172, 564.4828] |
| rust | rps | 13.4440 | [13.4149, 13.4730] |
| rust | ttft_p50_ms | 69.2214 | [68.6986, 69.7442] |
| rust | ttft_p99_ms | 4867.6854 | [4492.0699, 5243.3009] |

**P99 TTFT and RPS per arm:**

| Arm | P99 TTFT (ms) | RPS |
|---|---|---|
| rust | 4867.69 | 13.44 |

**GC pauses:**

| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |
|---|---|---|---|---|---|
| rust | Scheduler | 9.00 | 1.40 | 0.32 | 0.72 |
| rust | Launcher | 0.00 | -0.00 | n/a | n/a |
| rust | RustFrontend | N/A (Rust frontend has no garbage collector) | | | |
| rust | Other | 0.00 | -0.00 | n/a | n/a |

**Memory:**

| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |
|---|---|---|---|
| rust | Frontend | 8180721254 | 8198736691 |
| rust | Launcher | 688265626 | 537294438 |
| rust | Scheduler | 2497721139 | 2346987520 |
| rust | Other | 15690138 | 10443162 |
| rust | tree | 11382000845 | 11093023539 |

**GC/P99 co-occurrence:**

| Arm | Group | Spike overlap (mean) | Non-spike overlap (mean) |
|---|---|---|---|
| rust | Frontend | N/A (Rust frontend has no garbage collector) | |
| rust | Scheduler | 100.0% | 3.4% |

## s2_saturation

| Arm | OK | Failed |
|---|---|---|
| rust | 5 | 0 |

**Headline metrics:**

| Arm | Metric | Mean | 95% CI |
|---|---|---|---|
| rust | peak_rps | 72.4805 | [72.2847, 72.6762] |

**Saturation curve:**

| Level | rust achieved RPS | rust P99 TTFT (ms) |
|---|---|---|
| rate=5 | 4.86 | 75.17 |
| rate=10 | 10.27 | 78.25 |
| rate=20 | 19.76 | 80.91 |
| rate=40 | 36.45 | 227.66 |
| rate=60 | 61.02 | 671.69 |
| rate=80 | 72.48 | 282.34 |

**GC pauses:**

| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |
|---|---|---|---|---|---|
| rust | Scheduler | 2.83 | 15.04 | 17.74 | 88.98 |
| rust | Launcher | 0.00 | -0.00 | n/a | n/a |
| rust | RustFrontend | N/A (Rust frontend has no garbage collector) | | | |
| rust | Other | 0.00 | -0.00 | n/a | n/a |

**Memory:**

| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |
|---|---|---|---|
| rust | Frontend | 1492276019 | 1493418667 |
| rust | Launcher | 688611738 | 539258470 |
| rust | Scheduler | 2497804698 | 2348676983 |
| rust | Other | 15657370 | 10422477 |
| rust | tree | 4694270498 | 4391694404 |

**GC/P99 co-occurrence:**

| Arm | Group | Spike overlap (mean) | Non-spike overlap (mean) |
|---|---|---|---|
| rust | Frontend | N/A (Rust frontend has no garbage collector) | |
| rust | Scheduler | 6.7% | 0.8% |

