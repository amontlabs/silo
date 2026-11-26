### A team of five, $15, the stand-in's 500 tokens/s: live against the simulator

| Setting | Runs | Spent / budget | Over, worst | Under, worst | Paid for cut answers | Calls done | Calls that waited over 10 s | Longest wait, s | Messages per call | Sim: spent / budget | Sim: over, worst | Sim: under, worst | Sim: paid for cut answers | Sim: calls done | Sim: waited over 10 s | Sim: messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| one boundary, typical-cut | 4 | 0.990 | -0.6% | -1.4% | $0.00 | 93 | 0.0 | 0.0 | 0.00 | 0.991 | -0.0% | -1.5% | $0.00 | 91 | 0.0 | 0.00 |
| central 30 ms, typical-cut | 4 | 0.989 | -0.5% | -1.5% | $0.00 | 92 | 0.0 | 0.1 | 4.48 | 0.993 | -0.0% | -1.5% | $0.01 | 90 | 0.0 | 3.49 |
| leases 30 ms, typical-cut, a recall keeps $1 | 3 | 0.960 | -3.3% | -4.7% | $0.00 | 88 | 0.0 | 0.1 | 2.97 | 0.990 | -0.4% | -1.9% | $0.02 | 91 | 0.0 | 5.98 |
| one boundary, cut | 3 | 1.005 | +0.7% | +0.4% | $1.41 | 87 | 0.0 | 0.0 | 0.00 | 1.001 | +0.2% | +0.0% | $1.49 | 84 | 0.0 | 0.00 |
| central 30 ms, cut | 3 | 1.006 | +0.7% | +0.5% | $1.40 | 85 | 0.0 | 0.1 | 4.98 | 1.000 | +0.2% | -0.0% | $1.49 | 84 | 0.0 | 3.84 |
| leases 30 ms, cut, a recall keeps $1 | 3 | 1.011 | +1.3% | +1.0% | $1.72 | 86 | 0.0 | 0.1 | 3.70 | 1.002 | +0.5% | +0.0% | $1.73 | 82 | 0.0 | 6.56 |
| leases 30 ms, cut, #388's recall | 2 | 1.008 | +0.8% | +0.8% | $1.48 | 86 | 0.0 | 0.3 | 2.25 | 1.002 | +0.5% | +0.0% | $1.73 | 82 | 0.0 | 6.56 |

Largest gap between what the boundaries counted and what the provider billed, in any run: 0.37%.

### Leases through a Git branch

| Where | Macs | Pace between asks | Asks | Pushes tried | Refused | Ask p50, ms | Ask p90, ms | Ask max, ms | Read p50, ms | Push p50, ms | Refused push p50, ms | Never past the budget |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| git daemon, 0.0 ms | 1 | all at once | 5 | 5 | 0 | 178 | 219 | 219 | 59 | 116 | None | yes |
| git daemon, 50.0 ms | 1 | all at once | 5 | 5 | 0 | 357 | 411 | 411 | 116 | 238 | None | yes |
| git daemon, 150.0 ms | 1 | all at once | 5 | 5 | 0 | 961 | 1115 | 1115 | 319 | 641 | None | yes |
| git daemon, 0.0 ms | 4 | all at once | 10 | 34 | 24 | 188 | 2238 | 2238 | 108 | 124 | 122 | yes |
| git daemon, 50.0 ms | 4 | all at once | 10 | 34 | 24 | 364 | 4133 | 4133 | 177 | 240 | 241 | yes |
| git daemon, 150.0 ms | 4 | all at once | 10 | 34 | 24 | 967 | 10868 | 10868 | 478 | 644 | 641 | yes |
| git daemon, 0.0 ms | 12 | all at once | 19 | 162 | 143 | 2322 | 4180 | 4421 | 114 | 121 | 118 | yes |
| git daemon, 50.0 ms | 12 | all at once | 19 | 162 | 143 | 4081 | 7578 | 7986 | 184 | 244 | 241 | yes |
| git daemon, 150.0 ms | 12 | all at once | 19 | 162 | 143 | 10575 | 19644 | 20791 | 484 | 642 | 640 | yes |
| git daemon, 150.0 ms | 4 | 0 to 10 s | 14 | 22 | 8 | 1139 | 3381 | 4182 | 482 | 646 | 642 | yes |
| git daemon, 150.0 ms | 12 | 0 to 20 s | 32 | 106 | 74 | 3374 | 7508 | 7845 | 479 | 641 | 638 | yes |
| hosted | 1 | all at once | 5 | 5 | 0 | 2009 | 2368 | 2368 | 409 | 1599 | None | yes |
| hosted | 4 | all at once | 10 | 34 | 24 | 2428 | 22864 | 22864 | 534 | 1591 | 1687 | yes |
| hosted | 12 | all at once | 19 | 162 | 143 | 19266 | 40230 | 42448 | 578 | 1569 | 1612 | yes |
| hosted | 4 | 0 to 10 s | 14 | 29 | 15 | 4201 | 7796 | 10663 | 590 | 1563 | 1539 | yes |
| hosted | 12 | 0 to 20 s | 32 | 215 | 183 | 10734 | 34188 | 43568 | 570 | 1558 | 1579 | yes |
