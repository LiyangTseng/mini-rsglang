# Frontend Benchmark Report

## Summary

- s3_coldstart: trial 11 (arm python-best) failed: run_trial failed: hyperfine exited Some(1); last stderr:
Error: The conclusion command terminated with a non-zero exit code. Append ' || true' to the command if you are sure that this can be ignored.

## s1_cancel

| Arm | OK | Failed |
|---|---|---|
| python-best | 5 | 0 |
| python-default | 5 | 0 |
| rust | 5 | 0 |

**Headline metrics:**

| Arm | Metric | Mean | 95% CI |
|---|---|---|---|
| python-best | cancelled | 580.4000 | [579.7200, 581.0800] |
| python-best | rps | 13.8715 | [13.8577, 13.8853] |
| python-best | ttft_p50_ms | 53.5478 | [53.0092, 54.0864] |
| python-best | ttft_p99_ms | 314.3670 | [280.8582, 347.8758] |
| python-default | cancelled | 580.8000 | [580.2448, 581.3552] |
| python-default | rps | 13.8694 | [13.8447, 13.8941] |
| python-default | ttft_p50_ms | 53.6630 | [53.1473, 54.1787] |
| python-default | ttft_p99_ms | 315.6470 | [290.1920, 341.1020] |
| rust | cancelled | 581.2000 | [580.6448, 581.7552] |
| rust | rps | 13.8928 | [13.8573, 13.9282] |
| rust | ttft_p50_ms | 50.8982 | [50.1483, 51.6481] |
| rust | ttft_p99_ms | 301.6694 | [274.5259, 328.8129] |

**P99 TTFT and RPS per arm:**

| Arm | P99 TTFT (ms) | RPS |
|---|---|---|
| python-best | 314.37 | 13.87 |
| python-default | 315.65 | 13.87 |
| rust | 301.67 | 13.89 |

**Rust vs Python deltas:**

| Comparison | Metric | Diff | 95% CI | % | % 95% CI |
|---|---|---|---|---|---|
| rust_vs_python-best | cancelled | 0.8000 | [0.0521, 1.5479] | 0.14% | [0.01%, 0.27%] |
| rust_vs_python-best | rps | 0.0213 | [-0.0140, 0.0565] | 0.15% | [-0.10%, 0.41%] |
| rust_vs_python-best | ttft_p50_ms | -2.6496 | [-3.4362, -1.8630] | -4.95% | [-6.42%, -3.48%] |
| rust_vs_python-best | ttft_p99_ms | -12.6976 | [-49.4362, 24.0410] | -4.04% | [-15.73%, 7.65%] |
| rust_vs_python-default | cancelled | 0.4000 | [-0.2522, 1.0522] | 0.07% | [-0.04%, 0.18%] |
| rust_vs_python-default | rps | 0.0234 | [-0.0134, 0.0602] | 0.17% | [-0.10%, 0.43%] |
| rust_vs_python-default | ttft_p50_ms | -2.7648 | [-3.5401, -1.9895] | -5.15% | [-6.60%, -3.71%] |
| rust_vs_python-default | ttft_p99_ms | -13.9776 | [-45.6801, 17.7249] | -4.43% | [-14.47%, 5.62%] |

**GC pauses:**

| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |
|---|---|---|---|---|---|
| python-best | ApiServer | 4210.40 | 1768.69 | 0.71 | 90.65 |
| python-best | Tokenizer | 11.00 | 3.05 | 1.01 | 1.63 |
| python-best | Scheduler | 10.00 | 1.40 | 0.30 | 0.38 |
| python-best | Other | 0.00 | -0.00 | n/a | n/a |
| python-default | ApiServer | 4204.20 | 1790.07 | 0.76 | 82.73 |
| python-default | Tokenizer | 9.80 | 2.83 | 1.01 | 1.77 |
| python-default | Scheduler | 10.40 | 36.97 | 35.81 | 92.51 |
| python-default | Other | 0.00 | -0.00 | n/a | n/a |
| rust | Scheduler | 10.00 | 17.35 | 16.12 | 79.61 |
| rust | Launcher | 0.00 | -0.00 | n/a | n/a |
| rust | RustFrontend | N/A (Rust frontend has no garbage collector) | | | |
| rust | Other | 0.00 | -0.00 | n/a | n/a |

**Memory:**

| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |
|---|---|---|---|
| python-best | Frontend | 3043435315 | 2081000243 |
| python-best | Launcher | 0 | 0 |
| python-best | Scheduler | 2496117965 | 2253111296 |
| python-best | Other | 15709798 | 9656115 |
| python-best | tree | 5555263078 | 4343767654 |
| python-default | Frontend | 1500599910 | 1100218163 |
| python-default | Launcher | 0 | 0 |
| python-default | Scheduler | 2498392883 | 2296176435 |
| python-default | Other | 15698330 | 10040115 |
| python-default | tree | 4014691123 | 3406434714 |
| rust | Frontend | 138401382 | 138714522 |
| rust | Launcher | 688125542 | 539251917 |
| rust | Scheduler | 2496201523 | 2347493171 |
| rust | Other | 15669658 | 10417971 |
| rust | tree | 3338398106 | 3035877581 |

**GC/P99 co-occurrence:**

| Arm | Group | Spike overlap (mean) | Non-spike overlap (mean) |
|---|---|---|---|
| python-best | Frontend | 100.0% | 95.6% |
| python-best | Scheduler | 100.0% | 2.8% |
| python-default | Frontend | 100.0% | 95.6% |
| python-default | Scheduler | 100.0% | 3.0% |
| rust | Frontend | N/A (Rust frontend has no garbage collector) | |
| rust | Scheduler | 90.8% | 2.7% |

## s2_saturation

| Arm | OK | Failed |
|---|---|---|
| python-best | 5 | 0 |
| python-default | 5 | 0 |
| rust | 5 | 0 |

**Headline metrics:**

| Arm | Metric | Mean | 95% CI |
|---|---|---|---|
| python-best | peak_rps | 72.7062 | [72.6053, 72.8072] |
| python-default | peak_rps | 72.6594 | [72.5562, 72.7625] |
| rust | peak_rps | 72.6986 | [72.5991, 72.7980] |

**Rust vs Python deltas:**

| Comparison | Metric | Diff | 95% CI | % | % 95% CI |
|---|---|---|---|---|---|
| rust_vs_python-best | peak_rps | -0.0077 | [-0.1284, 0.1131] | -0.01% | [-0.18%, 0.16%] |
| rust_vs_python-default | peak_rps | 0.0392 | [-0.0829, 0.1612] | 0.05% | [-0.11%, 0.22%] |

**Saturation curve:**

| Level | python-best achieved RPS | python-best P99 TTFT (ms) | python-default achieved RPS | python-default P99 TTFT (ms) | rust achieved RPS | rust P99 TTFT (ms) |
|---|---|---|---|---|---|---|
| rate=5 | 4.86 | 38.41 | 4.86 | 38.10 | 4.86 | 53.27 |
| rate=10 | 10.27 | 40.93 | 10.27 | 42.39 | 10.27 | 57.78 |
| rate=20 | 19.77 | 46.34 | 19.77 | 45.43 | 19.78 | 58.25 |
| rate=40 | 36.50 | 50.62 | 36.50 | 51.18 | 36.51 | 59.17 |
| rate=60 | 61.25 | 73.50 | 61.23 | 90.60 | 61.32 | 63.39 |
| rate=80 | 72.71 | 110.26 | 72.66 | 98.46 | 72.70 | 59.12 |

**GC pauses:**

| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |
|---|---|---|---|---|---|
| python-best | ApiServer | 88.73 | 42.21 | 1.08 | 77.56 |
| python-best | Tokenizer | 0.17 | 0.03 | 0.17 | 0.28 |
| python-best | Scheduler | 2.67 | 0.40 | 0.37 | 0.79 |
| python-best | Other | 0.00 | -0.00 | n/a | n/a |
| python-default | ApiServer | 89.63 | 42.54 | 1.22 | 83.59 |
| python-default | Tokenizer | 0.17 | 0.01 | 0.08 | 0.15 |
| python-default | Scheduler | 2.70 | 0.43 | 0.40 | 0.81 |
| python-default | Other | 0.00 | -0.00 | n/a | n/a |
| rust | Scheduler | 2.90 | 14.03 | 15.33 | 85.49 |
| rust | Launcher | 0.00 | -0.00 | n/a | n/a |
| rust | RustFrontend | N/A (Rust frontend has no garbage collector) | | | |
| rust | Other | 0.00 | -0.00 | n/a | n/a |

**Memory:**

| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |
|---|---|---|---|
| python-best | Frontend | 3037086652 | 2073604642 |
| python-best | Launcher | 0 | 0 |
| python-best | Scheduler | 2493958554 | 2248714308 |
| python-best | Other | 15659008 | 9630515 |
| python-best | tree | 5546704213 | 4331949466 |
| python-default | Frontend | 1494223940 | 1091782383 |
| python-default | Launcher | 0 | 0 |
| python-default | Scheduler | 2496301466 | 2295060207 |
| python-default | Other | 15706522 | 10050355 |
| python-default | tree | 4006231927 | 3396892945 |
| rust | Frontend | 128078370 | 128351369 |
| rust | Launcher | 686818645 | 534858274 |
| rust | Scheduler | 2497434419 | 2345724518 |
| rust | Other | 15660646 | 10432717 |
| rust | tree | 3327992081 | 3019366878 |

**GC/P99 co-occurrence:**

| Arm | Group | Spike overlap (mean) | Non-spike overlap (mean) |
|---|---|---|---|
| python-best | Frontend | 61.7% | 33.2% |
| python-best | Scheduler | 1.7% | 0.5% |
| python-default | Frontend | 60.0% | 33.3% |
| python-default | Scheduler | 1.7% | 0.4% |
| rust | Frontend | N/A (Rust frontend has no garbage collector) | |
| rust | Scheduler | 3.3% | 0.6% |

## s3_coldstart

| Arm | OK | Failed |
|---|---|---|
| python-best | 4 | 1 |
| python-default | 5 | 0 |
| rust | 5 | 0 |

**Headline metrics:**

| Arm | Metric | Mean | 95% CI |
|---|---|---|---|
| python-best | e2e_ready_s | 9.2779 | [8.7311, 9.8248] |
| python-best | frontend_pss_bytes | 2060980821.0000 | [2051016471.9594, 2070945170.0406] |
| python-best | frontend_rss_bytes | 3025669802.2500 | [3019619423.4413, 3031720181.0587] |
| python-best | frontend_tail_s | 9.2743 | [8.7276, 9.8210] |
| python-best | tree_pss_bytes | 4316073642.5000 | [4308176665.6906, 4323970619.3094] |
| python-default | e2e_ready_s | 8.8698 | [8.5556, 9.1840] |
| python-default | frontend_pss_bytes | 1079610982.2000 | [1074981352.6720, 1084240611.7280] |
| python-default | frontend_rss_bytes | 1482236996.2000 | [1477944073.9812, 1486529918.4188] |
| python-default | frontend_tail_s | 8.8659 | [8.5520, 9.1799] |
| python-default | tree_pss_bytes | 3378344481.8000 | [3370719181.1928, 3385969782.4072] |
| rust | e2e_ready_s | 8.2206 | [8.1370, 8.3041] |
| rust | frontend_pss_bytes | 121365162.4000 | [120790890.7303, 121939434.0697] |
| rust | frontend_rss_bytes | 122636697.0000 | [122063171.7076, 123210222.2924] |
| rust | frontend_tail_s | 8.2168 | [8.1336, 8.3000] |
| rust | tree_pss_bytes | 3004572467.0000 | [2999623093.9443, 3009521840.0557] |

**Rust vs Python deltas:**

| Comparison | Metric | Diff | 95% CI | % | % 95% CI |
|---|---|---|---|---|---|
| rust_vs_python-best | e2e_ready_s | -1.0574 | [-1.6125, -0.5022] | -11.40% | [-17.38%, -5.41%] |
| rust_vs_python-best | frontend_pss_bytes | -1939615658.6000 | [-1949601726.8607, -1929629590.3393] | -94.11% | [-94.60%, -93.63%] |
| rust_vs_python-best | frontend_rss_bytes | -2903033105.2500 | [-2909119094.5394, -2896947115.9606] | -95.95% | [-96.15%, -95.75%] |
| rust_vs_python-best | frontend_tail_s | -1.0575 | [-1.6125, -0.5025] | -11.40% | [-17.39%, -5.42%] |
| rust_vs_python-best | tree_pss_bytes | -1311501175.5000 | [-1319357650.7536, -1303644700.2464] | -30.39% | [-30.57%, -30.20%] |
| rust_vs_python-default | e2e_ready_s | -0.6492 | [-0.9743, -0.3241] | -7.32% | [-10.98%, -3.65%] |
| rust_vs_python-default | frontend_pss_bytes | -958245819.8000 | [-962910930.4651, -953580709.1349] | -88.76% | [-89.19%, -88.33%] |
| rust_vs_python-default | frontend_rss_bytes | -1359600299.2000 | [-1363931362.8612, -1355269235.5388] | -91.73% | [-92.02%, -91.43%] |
| rust_vs_python-default | frontend_tail_s | -0.6491 | [-0.9740, -0.3243] | -7.32% | [-10.99%, -3.66%] |
| rust_vs_python-default | tree_pss_bytes | -373772014.8000 | [-381785355.5599, -365758674.0401] | -11.06% | [-11.30%, -10.83%] |

**Cold start:**

| Arm | End-to-end startup (s) | Frontend cold start tail (s) | Frontend RSS (bytes) | Frontend PSS (bytes) |
|---|---|---|---|---|
| python-best | 9.278 | 9.274 | 3025669802 | 2060980821 |
| python-default | 8.870 | 8.866 | 1482236996 | 1079610982 |
| rust | 8.221 | 8.217 | 122636697 | 121365162 |

**GC pauses:**

| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |
|---|---|---|---|---|---|
| rust | RustFrontend | N/A (Rust frontend has no garbage collector) | | | |

**Memory:**

| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |
|---|---|---|---|
| python-best | Frontend | 3025669803 | 2060980821 |
| python-best | Launcher | 0 | 0 |
| python-best | Scheduler | 2487143424 | 2245456640 |
| python-best | Other | 15691776 | 9636181 |
| python-best | tree | 5528505003 | 4316073643 |
| python-default | Frontend | 1482236996 | 1079610982 |
| python-default | Launcher | 0 | 0 |
| python-default | Scheduler | 2487681843 | 2288699392 |
| python-default | Other | 15648631 | 10034108 |
| python-default | tree | 3985567471 | 3378344482 |
| rust | Frontend | 122636698 | 121365163 |
| rust | Launcher | 684446242 | 534997197 |
| rust | Scheduler | 2485824444 | 2337793980 |
| rust | Other | 15687953 | 10416128 |
| rust | tree | 3308595337 | 3004572467 |

**Failed trials:**

- arm python-best, trial 11: run_trial failed: hyperfine exited Some(1); last stderr:
Error: The conclusion command terminated with a non-zero exit code. Append ' || true' to the command if you are sure that this can be ignored.

