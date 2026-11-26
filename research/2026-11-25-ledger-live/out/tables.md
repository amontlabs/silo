### A team of five, $15, the stand-in's 500 tokens/s: live against the simulator

| Setting | Runs | Spent / budget | Over, worst | Under, worst | Paid for cut answers | Calls done | Calls that waited over 10 s | Longest wait, s | Messages per call | Cuts with no answer, per run | Sim: spent / budget | Sim: over, worst | Sim: under, worst | Sim: paid for cut answers | Sim: calls done | Sim: waited over 10 s | Sim: messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| one boundary, typical-cut | 4 | 0.990 | -0.6% | -1.4% | $0.00 | 93 | 0.0 | 0.0 | 0.00 |  | 0.991 | -0.0% | -1.5% | $0.00 | 91 | 0.0 | 0.00 |
| central 30 ms, typical-cut | 4 | 0.989 | -0.5% | -1.5% | $0.00 | 92 | 0.0 | 0.1 | 4.48 |  | 0.993 | -0.0% | -1.5% | $0.01 | 90 | 0.0 | 3.49 |
| leases 30 ms, typical-cut | 3 | 0.994 | -0.4% | -0.8% | $0.00 | 92 | 0.0 | 0.6 | 6.74 | 0.0 | 0.990 | -0.4% | -1.9% | $0.02 | 91 | 0.0 | 5.98 |
| leases 30 ms, typical-cut, a recall keeps $1 | 3 | 0.960 | -3.3% | -4.7% | $0.00 | 88 | 0.0 | 0.1 | 2.97 | 0.0 | 0.990 | -0.4% | -1.9% | $0.02 | 91 | 0.0 | 5.98 |
| one boundary, cut | 3 | 1.005 | +0.7% | +0.4% | $1.41 | 87 | 0.0 | 0.0 | 0.00 |  | 1.001 | +0.2% | +0.0% | $1.49 | 84 | 0.0 | 0.00 |
| central 30 ms, cut | 3 | 1.006 | +0.7% | +0.5% | $1.40 | 85 | 0.0 | 0.1 | 4.98 |  | 1.000 | +0.2% | -0.0% | $1.49 | 84 | 0.0 | 3.84 |
| leases 30 ms, cut | 3 | 1.010 | +1.3% | +0.8% | $1.37 | 86 | 0.0 | 0.1 | 3.31 | 0.0 | 1.002 | +0.5% | +0.0% | $1.73 | 82 | 0.0 | 6.56 |
| leases 30 ms, cut, a recall keeps $1 | 3 | 1.011 | +1.3% | +1.0% | $1.72 | 86 | 0.0 | 0.1 | 3.70 | 0.0 | 1.002 | +0.5% | +0.0% | $1.73 | 82 | 0.0 | 6.56 |
| leases 30 ms, cut, #388's recall | 3 | 1.009 | +1.1% | +0.8% | $1.56 | 84 | 0.0 | 0.3 | 2.59 | 2.7 | 1.002 | +0.5% | +0.0% | $1.73 | 82 | 0.0 | 6.56 |

### The ledger killed, Macs cut off from it: live against the simulator (typical-cut)

| Setting | Runs | Spent / budget | Over, worst | Under, worst | Paid for cut answers | Calls done | Calls that waited over 10 s | Longest wait, s | Messages per call | Cuts with no answer, per run | Sim: spent / budget | Sim: over, worst | Sim: under, worst | Sim: paid for cut answers | Sim: calls done | Sim: waited over 10 s | Sim: messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| central, ledger killed 40-70 s | 2 | 0.994 | -0.2% | -1.0% | $0.00 | 92 | 15.0 | 29.9 | 42.57 |  | 0.991 | -0.1% | -1.3% | $0.00 | 90 | 8.5 | 3.49 |
| leases, ledger killed 40-70 s | 2 | 0.992 | -0.6% | -1.1% | $0.00 | 93 | 5.0 | 29.4 | 10.59 | 0.0 | 0.990 | -0.3% | -1.4% | $0.02 | 91 | 1.6 | 5.16 |
| central, all Macs cut off 40-70 s | 2 | 0.992 | -0.6% | -1.1% | $0.00 | 94 | 15.0 | 29.8 | 8.50 |  | 0.991 | -0.1% | -1.3% | $0.00 | 90 | 8.5 | 3.49 |
| leases, all Macs cut off 40-70 s | 2 | 0.989 | -1.0% | -1.2% | $0.00 | 91 | 4.0 | 29.3 | 6.67 | 0.0 | 0.990 | -0.3% | -1.4% | $0.02 | 91 | 1.6 | 5.16 |
| central, ledger killed 40-70 s, calls refused | 2 | 0.233 | -64.8% | -88.6% | $0.00 | 22 | 0.0 | 0.1 | 11.07 |  | 0.109 | -75.1% | -92.6% | $0.00 | 9 | 0.0 | 5.05 |
| central, ledger killed 40-160 s | 2 | 0.994 | -0.4% | -0.8% | $0.00 | 88 | 6.5 | 120.0 | 197.69 |  | 0.991 | -0.1% | -1.3% | $0.00 | 90 | 8.6 | 3.49 |
| leases, ledger killed 40-160 s | 2 | 0.984 | -1.0% | -2.1% | $0.12 | 94 | 18.5 | 119.3 | 36.44 | 0.0 | 0.989 | -0.4% | -2.5% | $0.01 | 90 | 11.8 | 4.04 |
| leases, 2 Macs cut off 40-100 s | 2 | 0.988 | -1.2% | -1.2% | $0.00 | 92 | 3.5 | 59.5 | 5.57 | 0.0 | n/a | | | | | | |

### A Mac that sleeps holding a slice (typical-cut); the simulator has no model of it

| Setting | Runs | Spent / budget | Over, worst | Under, worst | Paid for cut answers | Calls done | Calls that waited over 10 s | Longest wait, s | Messages per call | Cuts with no answer, per run | Sim: spent / budget | Sim: over, worst | Sim: under, worst | Sim: paid for cut answers | Sim: calls done | Sim: waited over 10 s | Sim: messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| leases, a heavy Mac asleep 30-150 s, no expiry | 2 | 0.987 | -1.1% | -1.4% | $0.00 | 94 | 0.0 | 0.6 | 3.50 | 0.0 | n/a | | | | | | |
| leases, a heavy Mac asleep 30-150 s, 20 s leases | 2 | 0.992 | -0.5% | -1.2% | $0.01 | 93 | 0.0 | 2.5 | 5.04 | 0.5 | n/a | | | | | | |

### A batch held at its bound (typical-cut)

| Setting | Runs | Spent / budget | Over, worst | Under, worst | Paid for cut answers | Calls done | Calls that waited over 10 s | Longest wait, s | Messages per call | Cuts with no answer, per run | Sim: spent / budget | Sim: over, worst | Sim: under, worst | Sim: paid for cut answers | Sim: calls done | Sim: waited over 10 s | Sim: messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| one boundary, typical-cut, a batch (batch admitted 2 of 2; sim 20 of 20) | 2 | 0.828 | -16.7% | -17.6% | $0.00 | 73 | 0.0 | 0.0 | 0.00 |  | 0.829 | -16.3% | -17.8% | $0.00 | 64 | 0.0 | 0.00 |
| central 30 ms, typical-cut, a batch (batch admitted 2 of 2; sim 20 of 20) | 2 | 0.833 | -16.7% | -16.8% | $0.00 | 68 | 0.0 | 0.1 | 4.71 |  | 0.827 | -16.7% | -17.8% | $0.00 | 63 | 0.0 | 3.75 |
| leases 30 ms, typical-cut, a batch (batch admitted 2 of 2; sim 19 of 20) | 2 | 0.824 | -17.3% | -17.8% | $0.00 | 67 | 0.0 | 0.3 | 9.90 | 0.0 | 0.836 | -0.8% | -18.2% | $0.03 | 65 | 0.0 | 6.54 |

Largest gap between what the boundaries counted and what the provider billed, in any run without a batch: 0.37%. In the batch runs the Macs' call totals leave the batch out; each batch was settled at what the provider billed for it, to the cent.

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
