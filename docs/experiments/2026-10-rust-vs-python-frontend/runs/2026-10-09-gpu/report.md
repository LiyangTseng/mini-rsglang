# Frontend Benchmark Report

## Summary

- D-10: Python's default --num-tokenizer is 0; this sweep's best-performing candidate is 2.
- REGRESSION: Rust frontend standard throughput is 1.56% lower than python-best (95% CI [-0.63%, 3.74%]); ±2% is a reference target, not a gate.
- REGRESSION: Rust frontend standard throughput is 0.34% lower than python-default (95% CI [-2.54%, 3.23%]); ±2% is a reference target, not a gate.

## num_tokenizer_sweep

| Arm | OK | Failed |
|---|---|---|
| python-nt0 | 1 | 0 |
| python-nt1 | 1 | 0 |
| python-nt2 | 1 | 0 |
| python-nt4 | 1 | 0 |

**Headline metrics:**

| Arm | Metric | Mean | 95% CI |
|---|---|---|---|
| python-nt0 | peak_rps | 72.6160 | n/a (n<2) |
| python-nt1 | peak_rps | 72.5754 | n/a (n<2) |
| python-nt2 | peak_rps | 72.6201 | n/a (n<2) |
| python-nt4 | peak_rps | 72.5546 | n/a (n<2) |

**Sweep candidates (D-09):**

| --num-tokenizer | Peak RPS |
|---|---|
| 0 | 72.62 |
| 1 | 72.58 |
| 2 | 72.62 |
| 4 | 72.55 |

**GC pauses:**

| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |
|---|---|---|---|---|---|
| python-nt0 | ApiServer | 87.83 | 48.21 | 1.45 | 84.18 |
| python-nt0 | Tokenizer | 0.17 | 0.01 | 0.05 | 0.05 |
| python-nt0 | Scheduler | 2.67 | 0.62 | 0.65 | 1.45 |
| python-nt0 | Other | 0.00 | -0.00 | n/a | n/a |
| python-nt1 | ApiServer | 90.67 | 45.38 | 1.08 | 78.60 |
| python-nt1 | Tokenizer | 0.33 | 0.08 | 0.24 | 0.45 |
| python-nt1 | Scheduler | 3.00 | 23.20 | 22.90 | 134.44 |
| python-nt1 | Other | 0.00 | -0.00 | n/a | n/a |
| python-nt2 | ApiServer | 90.50 | 45.44 | 13.70 | 76.91 |
| python-nt2 | Tokenizer | 0.17 | 0.08 | 0.46 | 0.46 |
| python-nt2 | Scheduler | 2.67 | 0.47 | 0.48 | 0.98 |
| python-nt2 | Other | 0.00 | -0.00 | n/a | n/a |
| python-nt4 | ApiServer | 90.67 | 43.35 | 0.94 | 72.41 |
| python-nt4 | Tokenizer | 0.00 | -0.00 | n/a | n/a |
| python-nt4 | Scheduler | 2.67 | 0.41 | 0.38 | 0.66 |
| python-nt4 | Other | 0.00 | -0.00 | n/a | n/a |

**Memory:**

| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |
|---|---|---|---|
| python-nt0 | Frontend | 1493576363 | 1086883840 |
| python-nt0 | Launcher | 0 | 0 |
| python-nt0 | Scheduler | 2500242091 | 2300801024 |
| python-nt0 | Other | 100599263232 | 85831174144 |
| python-nt0 | tree | 104593081685 | 89218859008 |
| python-nt1 | Frontend | 2266935979 | 1590834859 |
| python-nt1 | Launcher | 0 | 0 |
| python-nt1 | Scheduler | 2494427136 | 2270100821 |
| python-nt1 | Other | 115100654251 | 94372697771 |
| python-nt1 | tree | 119862017365 | 98233633451 |
| python-nt2 | Frontend | 3043033088 | 2079631701 |
| python-nt2 | Launcher | 0 | 0 |
| python-nt2 | Scheduler | 2492492459 | 2247710037 |
| python-nt2 | Other | 139086165333 | 109391304704 |
| python-nt2 | tree | 144621690880 | 113718646443 |
| python-nt4 | Frontend | 4584016555 | 3024723968 |
| python-nt4 | Launcher | 0 | 0 |
| python-nt4 | Scheduler | 2493442048 | 2232142507 |
| python-nt4 | Other | 186953210539 | 138794927104 |
| python-nt4 | tree | 194030669141 | 144051793579 |

**GC/P99 co-occurrence:**

| Arm | Group | Spike overlap (mean) | Non-spike overlap (mean) |
|---|---|---|---|
| python-nt0 | Frontend | 66.7% | 34.1% |
| python-nt0 | Scheduler | 0.0% | 0.5% |
| python-nt1 | Frontend | 61.1% | 34.4% |
| python-nt1 | Scheduler | 5.6% | 0.6% |
| python-nt2 | Frontend | 63.9% | 34.4% |
| python-nt2 | Scheduler | 0.0% | 0.5% |
| python-nt4 | Frontend | 66.7% | 34.3% |
| python-nt4 | Scheduler | 11.1% | 0.4% |

## s1_cancel

| Arm | OK | Failed |
|---|---|---|
| python-best | 5 | 0 |
| python-default | 5 | 0 |
| rust | 5 | 0 |

**Headline metrics:**

| Arm | Metric | Mean | 95% CI |
|---|---|---|---|
| python-best | cancelled | 578.0000 | [574.0741, 581.9259] |
| python-best | rps | 13.7703 | [13.6431, 13.8974] |
| python-best | ttft_p50_ms | 54.2070 | [53.7245, 54.6895] |
| python-best | ttft_p99_ms | 430.8470 | [188.1666, 673.5274] |
| python-default | cancelled | 576.6000 | [569.2240, 583.9760] |
| python-default | rps | 13.7332 | [13.5737, 13.8927] |
| python-default | ttft_p50_ms | 54.0022 | [52.8843, 55.1201] |
| python-default | ttft_p99_ms | 409.2406 | [255.5889, 562.8923] |
| rust | cancelled | 559.2000 | [551.9931, 566.4069] |
| rust | rps | 13.3364 | [13.1490, 13.5239] |
| rust | ttft_p50_ms | 74.9814 | [73.8224, 76.1404] |
| rust | ttft_p99_ms | 4687.4614 | [4394.9367, 4979.9861] |

**P99 TTFT and RPS per arm:**

| Arm | P99 TTFT (ms) | RPS |
|---|---|---|
| python-best | 430.85 | 13.77 |
| python-default | 409.24 | 13.73 |
| rust | 4687.46 | 13.34 |

**Rust vs Python deltas:**

| Comparison | Metric | Diff | 95% CI | % | % 95% CI |
|---|---|---|---|---|---|
| rust_vs_python-best | cancelled | -18.8000 | [-26.0342, -11.5658] | -3.25% | [-4.50%, -2.00%] |
| rust_vs_python-best | rps | -0.4338 | [-0.6268, -0.2408] | -3.15% | [-4.55%, -1.75%] |
| rust_vs_python-best | ttft_p50_ms | 20.7744 | [19.6117, 21.9371] | 38.32% | [36.18%, 40.47%] |
| rust_vs_python-best | ttft_p99_ms | 4256.6144 | [3932.8029, 4580.4259] | 987.96% | [912.81%, 1063.12%] |
| rust_vs_python-default | cancelled | -17.4000 | [-26.1856, -8.6144] | -3.02% | [-4.54%, -1.49%] |
| rust_vs_python-default | rps | -0.3968 | [-0.6064, -0.1871] | -2.89% | [-4.42%, -1.36%] |
| rust_vs_python-default | ttft_p50_ms | 20.9792 | [19.6073, 22.3511] | 38.85% | [36.31%, 41.39%] |
| rust_vs_python-default | ttft_p99_ms | 4278.2208 | [3986.9579, 4569.4837] | 1045.40% | [974.23%, 1116.58%] |

**GC pauses:**

| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |
|---|---|---|---|---|---|
| python-best | ApiServer | 4177.00 | 1954.51 | 0.82 | 107.18 |
| python-best | Tokenizer | 10.80 | 3.11 | 1.10 | 1.41 |
| python-best | Scheduler | 10.00 | 1.64 | 0.35 | 0.67 |
| python-best | Other | 0.00 | -0.00 | n/a | n/a |
| python-default | ApiServer | 4173.00 | 1950.79 | 0.80 | 115.21 |
| python-default | Tokenizer | 9.60 | 3.34 | 1.36 | 3.19 |
| python-default | Scheduler | 10.00 | 1.56 | 0.30 | 0.44 |
| python-default | Other | 0.00 | -0.00 | n/a | n/a |
| rust | Scheduler | 9.20 | 20.51 | 19.17 | 94.54 |
| rust | Launcher | 0.00 | -0.00 | n/a | n/a |
| rust | RustFrontend | N/A (Rust frontend has no garbage collector) | | | |
| rust | Other | 0.00 | -0.00 | n/a | n/a |

**Memory:**

| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |
|---|---|---|---|
| python-best | Frontend | 3040792576 | 2079087411 |
| python-best | Launcher | 0 | 0 |
| python-best | Scheduler | 2494917018 | 2250084557 |
| python-best | Other | 139075356262 | 109385146368 |
| python-best | tree | 144611065856 | 113714318336 |
| python-default | Frontend | 1496502272 | 1093529190 |
| python-default | Launcher | 0 | 0 |
| python-default | Scheduler | 2491630387 | 2292522803 |
| python-default | Other | 100416714342 | 85748067123 |
| python-default | tree | 104404847002 | 89134119117 |
| rust | Frontend | 31409464934 | n/a (PSS needs Linux) |
| rust | Launcher | 687107277 | n/a (PSS needs Linux) |
| rust | Scheduler | 2494065050 | n/a (PSS needs Linux) |
| rust | Other | 1280859684045 | n/a (PSS needs Linux) |
| rust | tree | 1315449638912 | n/a (PSS needs Linux) |

**GC/P99 co-occurrence:**

| Arm | Group | Spike overlap (mean) | Non-spike overlap (mean) |
|---|---|---|---|
| python-best | Frontend | 100.0% | 95.1% |
| python-best | Scheduler | 100.0% | 3.0% |
| python-default | Frontend | 100.0% | 95.9% |
| python-default | Scheduler | 100.0% | 2.9% |
| rust | Frontend | N/A (Rust frontend has no garbage collector) | |
| rust | Scheduler | 100.0% | 3.9% |

## s2_saturation

| Arm | OK | Failed |
|---|---|---|
| python-best | 5 | 0 |
| python-default | 5 | 0 |
| rust | 5 | 0 |

**Headline metrics:**

| Arm | Metric | Mean | 95% CI |
|---|---|---|---|
| python-best | peak_rps | 72.5632 | [72.4290, 72.6974] |
| python-default | peak_rps | 72.5652 | [72.4889, 72.6415] |
| rust | peak_rps | 72.3437 | [72.2451, 72.4422] |

**Rust vs Python deltas:**

| Comparison | Metric | Diff | 95% CI | % | % 95% CI |
|---|---|---|---|---|---|
| rust_vs_python-best | peak_rps | -0.2195 | [-0.3613, -0.0777] | -0.30% | [-0.50%, -0.11%] |
| rust_vs_python-default | peak_rps | -0.2216 | [-0.3277, -0.1154] | -0.31% | [-0.45%, -0.16%] |

**Saturation curve:**

| Level | python-best achieved RPS | python-best P99 TTFT (ms) | python-default achieved RPS | python-default P99 TTFT (ms) | rust achieved RPS | rust P99 TTFT (ms) |
|---|---|---|---|---|---|---|
| rate=5 | 4.86 | 42.51 | 4.86 | 42.35 | 4.86 | 77.41 |
| rate=10 | 10.27 | 52.11 | 10.27 | 50.95 | 10.26 | 82.26 |
| rate=20 | 19.77 | 53.67 | 19.77 | 62.16 | 19.75 | 87.78 |
| rate=40 | 36.47 | 130.18 | 36.46 | 82.08 | 36.42 | 263.41 |
| rate=60 | 61.07 | 126.07 | 61.15 | 106.87 | 60.91 | 1016.22 |
| rate=80 | 72.56 | 159.32 | 72.57 | 129.96 | 72.34 | 373.52 |

**GC pauses:**

| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |
|---|---|---|---|---|---|
| python-best | ApiServer | 91.07 | 45.03 | 3.34 | 89.86 |
| python-best | Tokenizer | 0.23 | 0.04 | 0.18 | 0.28 |
| python-best | Scheduler | 2.70 | 0.48 | 0.46 | 0.96 |
| python-best | Other | 0.00 | -0.00 | n/a | n/a |
| python-default | ApiServer | 92.07 | 48.95 | 3.46 | 94.01 |
| python-default | Tokenizer | 0.17 | 0.02 | 0.10 | 0.22 |
| python-default | Scheduler | 2.70 | 3.89 | 5.33 | 101.99 |
| python-default | Other | 0.00 | -0.00 | n/a | n/a |
| rust | Scheduler | 2.80 | 12.75 | 15.61 | 96.79 |
| rust | Launcher | 0.00 | -0.00 | n/a | n/a |
| rust | RustFrontend | N/A (Rust frontend has no garbage collector) | | | |
| rust | Other | 0.00 | -0.00 | n/a | n/a |

**Memory:**

| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |
|---|---|---|---|
| python-best | Frontend | 3037392623 | 2074314513 |
| python-best | Launcher | 0 | 0 |
| python-best | Scheduler | 2493399723 | 2249860540 |
| python-best | Other | 138997629201 | 109303034812 |
| python-best | tree | 144528421547 | 113627208226 |
| python-default | Frontend | 1493114743 | 1093682722 |
| python-default | Launcher | 0 | 0 |
| python-default | Scheduler | 2496773325 | 2292436582 |
| python-default | Other | 100471130385 | 85752791177 |
| python-default | tree | 104461018453 | 89138910481 |
| rust | Frontend | 6084904687 | 6248028023 |
| rust | Launcher | 685214242 | 533902370 |
| rust | Scheduler | 2493513455 | 2342347093 |
| rust | Other | 146599929856 | 145131085995 |
| rust | tree | 155863467759 | 154241458517 |

**GC/P99 co-occurrence:**

| Arm | Group | Spike overlap (mean) | Non-spike overlap (mean) |
|---|---|---|---|
| python-best | Frontend | 63.3% | 34.3% |
| python-best | Scheduler | 0.0% | 0.4% |
| python-default | Frontend | 62.2% | 34.5% |
| python-default | Scheduler | 2.8% | 0.4% |
| rust | Frontend | N/A (Rust frontend has no garbage collector) | |
| rust | Scheduler | 5.6% | 0.9% |

## s3_coldstart

| Arm | OK | Failed |
|---|---|---|
| python-best | 5 | 0 |
| python-default | 5 | 0 |
| rust | 5 | 0 |

**Headline metrics:**

| Arm | Metric | Mean | 95% CI |
|---|---|---|---|
| python-best | e2e_ready_s | 10.7407 | [9.4603, 12.0211] |
| python-best | frontend_pss_bytes | 2062224247.0000 | [2059431741.5971, 2065016752.4029] |
| python-best | frontend_rss_bytes | 3027679914.2000 | [3024239911.0410, 3031119917.3590] |
| python-best | frontend_tail_s | 10.7365 | [9.4577, 12.0153] |
| python-best | tree_pss_bytes | 100445097915.6000 | [100375362124.3992, 100514833706.8008] |
| python-default | e2e_ready_s | 9.6733 | [9.1558, 10.1907] |
| python-default | frontend_pss_bytes | 1080388880.8000 | [1076095206.1998, 1084682555.4002] |
| python-default | frontend_rss_bytes | 1480413729.8000 | [1479721065.6668, 1481106393.9332] |
| python-default | frontend_tail_s | 9.6692 | [9.1521, 10.1864] |
| python-default | tree_pss_bytes | 81801585663.8000 | [81733052442.2364, 81870118885.3636] |
| rust | e2e_ready_s | 9.1306 | [8.6325, 9.6287] |
| rust | frontend_pss_bytes | 485426790.2000 | [483060661.9416, 487792918.4584] |
| rust | frontend_rss_bytes | 490622156.4000 | [487842257.5373, 493402055.2627] |
| rust | frontend_tail_s | 9.1268 | [8.6297, 9.6239] |
| rust | tree_pss_bytes | 75103199777.8000 | [75060138690.0210, 75146260865.5790] |

**Rust vs Python deltas:**

| Comparison | Metric | Diff | 95% CI | % | % 95% CI |
|---|---|---|---|---|---|
| rust_vs_python-best | e2e_ready_s | -1.6101 | [-2.8825, -0.3376] | -14.99% | [-26.84%, -3.14%] |
| rust_vs_python-best | frontend_pss_bytes | -1576797456.8000 | [-1579915698.5249, -1573679215.0751] | -76.46% | [-76.61%, -76.31%] |
| rust_vs_python-best | frontend_rss_bytes | -2537057757.8000 | [-2540825770.0827, -2533289745.5173] | -83.80% | [-83.92%, -83.67%] |
| rust_vs_python-best | frontend_tail_s | -1.6097 | [-2.8804, -0.3390] | -14.99% | [-26.83%, -3.16%] |
| rust_vs_python-best | tree_pss_bytes | -25341898137.8000 | [-25414144020.8292, -25269652254.7708] | -25.23% | [-25.30%, -25.16%] |
| rust_vs_python-default | e2e_ready_s | -0.5426 | [-1.1545, 0.0692] | -5.61% | [-11.93%, 0.72%] |
| rust_vs_python-default | frontend_pss_bytes | -594962090.6000 | [-599283539.5175, -590640641.6825] | -55.07% | [-55.47%, -54.67%] |
| rust_vs_python-default | frontend_rss_bytes | -989791573.4000 | [-992656468.0382, -986926678.7618] | -66.86% | [-67.05%, -66.67%] |
| rust_vs_python-default | frontend_tail_s | -0.5425 | [-1.1536, 0.0687] | -5.61% | [-11.93%, 0.71%] |
| rust_vs_python-default | tree_pss_bytes | -6698385886.0000 | [-6769731994.0878, -6627039777.9122] | -8.19% | [-8.28%, -8.10%] |

**Cold start:**

| Arm | End-to-end startup (s) | Frontend cold start tail (s) | Frontend RSS (bytes) | Frontend PSS (bytes) |
|---|---|---|---|---|
| python-best | 10.741 | 10.737 | 3027679914 | 2062224247 |
| python-default | 9.673 | 9.669 | 1480413730 | 1080388881 |
| rust | 9.131 | 9.127 | 490622156 | 485426790 |

**GC pauses:**

| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |
|---|---|---|---|---|---|
| rust | RustFrontend | N/A (Rust frontend has no garbage collector) | | | |

**Memory:**

| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |
|---|---|---|---|
| python-best | Frontend | 3027679915 | 2062224247 |
| python-best | Launcher | 0 | 0 |
| python-best | Scheduler | 2485717675 | 2244002065 |
| python-best | Other | 120059567582 | 96138871603 |
| python-best | tree | 125572965171 | 100445097916 |
| python-default | Frontend | 1480413730 | 1080388881 |
| python-default | Launcher | 0 | 0 |
| python-default | Scheduler | 2486011494 | 2285216085 |
| python-default | Other | 90666989158 | 78435980698 |
| python-default | tree | 94633414383 | 81801585664 |
| rust | Frontend | 490622157 | 485426790 |
| rust | Launcher | 684439689 | 533147511 |
| rust | Scheduler | 2487063347 | 2336927334 |
| rust | Other | 77796959027 | 71747698142 |
| rust | tree | 81459084220 | 75103199778 |

## standard_throughput

| Arm | OK | Failed |
|---|---|---|
| python-best | 5 | 0 |
| python-default | 5 | 0 |
| rust | 5 | 0 |

**Headline metrics:**

| Arm | Metric | Mean | 95% CI |
|---|---|---|---|
| python-best | throughput_req_s | 0.5486 | [0.5474, 0.5499] |
| python-best | throughput_tok_s | 267.1929 | [266.5821, 267.8037] |
| python-best | ttft_p99_ms | 100664.7039 | [99628.8331, 101700.5747] |
| python-default | throughput_req_s | 0.5420 | [0.5280, 0.5559] |
| python-default | throughput_tok_s | 263.9385 | [257.1372, 270.7398] |
| python-default | ttft_p99_ms | 102672.6837 | [99910.8188, 105434.5486] |
| rust | throughput_req_s | 0.5401 | [0.5281, 0.5520] |
| rust | throughput_tok_s | 263.0310 | [257.2198, 268.8422] |
| rust | ttft_p99_ms | 101945.6259 | [99624.3034, 104266.9483] |

**Rust vs Python deltas:**

| Comparison | Metric | Diff | 95% CI | % | % 95% CI |
|---|---|---|---|---|---|
| rust_vs_python-best | throughput_req_s | -0.0086 | [-0.0206, 0.0034] | -1.56% | [-3.75%, 0.63%] |
| rust_vs_python-best | throughput_tok_s | -4.1619 | [-10.0051, 1.6813] | -1.56% | [-3.74%, 0.63%] |
| rust_vs_python-best | ttft_p99_ms | 1280.9220 | [-1073.3222, 3635.1662] | 1.27% | [-1.07%, 3.61%] |
| rust_vs_python-default | throughput_req_s | -0.0019 | [-0.0175, 0.0138] | -0.35% | [-3.24%, 2.54%] |
| rust_vs_python-default | throughput_tok_s | -0.9075 | [-8.5288, 6.7139] | -0.34% | [-3.23%, 2.54%] |
| rust_vs_python-default | ttft_p99_ms | -727.0578 | [-3800.7313, 2346.6156] | -0.71% | [-3.70%, 2.29%] |

**GC pauses:**

| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |
|---|---|---|---|---|---|
| python-best | ApiServer | 18.00 | 2.36 | 0.84 | 0.97 |
| python-best | Tokenizer | 1.00 | 0.20 | 0.20 | 0.29 |
| python-best | Scheduler | 0.40 | 0.06 | 0.15 | 0.15 |
| python-best | Other | 0.00 | -0.00 | n/a | n/a |
| python-default | ApiServer | 18.40 | 2.36 | 0.85 | 1.09 |
| python-default | Tokenizer | 0.00 | -0.00 | n/a | n/a |
| python-default | Scheduler | 0.00 | -0.00 | n/a | n/a |
| python-default | Other | 0.00 | -0.00 | n/a | n/a |
| rust | Scheduler | 0.00 | -0.00 | n/a | n/a |
| rust | Launcher | 0.00 | -0.00 | n/a | n/a |
| rust | RustFrontend | N/A (Rust frontend has no garbage collector) | | | |
| rust | Other | 0.00 | -0.00 | n/a | n/a |

**Memory:**

| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |
|---|---|---|---|
| python-best | Frontend | 3047106970 | 2036448461 |
| python-best | Launcher | 0 | 0 |
| python-best | Scheduler | 2495787008 | 2242030387 |
| python-best | Other | 139307932877 | 108143028019 |
| python-best | tree | 144850826854 | 112421506867 |
| python-default | Frontend | 1498284851 | 1046046925 |
| python-default | Launcher | 0 | 0 |
| python-default | Scheduler | 2495223398 | 2266530406 |
| python-default | Other | 100547188326 | 83952961331 |
| python-default | tree | 104540696576 | 87265538662 |
| rust | Frontend | 8872778138 | n/a (PSS needs Linux) |
| rust | Launcher | 687338291 | n/a (PSS needs Linux) |
| rust | Scheduler | 2496271155 | n/a (PSS needs Linux) |
| rust | Other | 273715949568 | n/a (PSS needs Linux) |
| rust | tree | 285768278016 | n/a (PSS needs Linux) |

