### A team of five, $15, the stand-in's 500 tokens/s: live against the simulator

| Setting | Runs | Spent / budget | Over, worst | Under, worst | Paid for cut answers | Calls done | Calls that waited over 10 s | Longest wait, s | Messages per call | Sim: spent / budget | Sim: over, worst | Sim: under, worst | Sim: paid for cut answers | Sim: calls done | Sim: waited over 10 s | Sim: messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| one boundary, typical-cut | 1 | 0.994 | -0.6% | -0.6% | $0.00 | 94 | 0.0 | 0.0 | 0.00 | 0.991 | -0.0% | -1.5% | $0.00 | 91 | 0.0 | 0.00 |
| central 30 ms, typical-cut | 1 | 0.985 | -1.5% | -1.5% | $0.00 | 84 | 0.0 | 0.1 | 4.52 | 0.993 | -0.0% | -1.5% | $0.01 | 90 | 0.0 | 3.49 |
| leases 30 ms, typical-cut | 1 | 0.961 | -3.9% | -3.9% | $0.00 | 81 | 0.0 | 0.1 | 2.52 | 0.990 | -0.4% | -1.9% | $0.02 | 91 | 0.0 | 5.98 |

Largest gap between what the boundaries counted and what the provider billed, in any run: 0.00%.

### Leases through a Git branch

| Where | Macs | Pace between asks | Asks | Pushes tried | Refused | Ask p50, ms | Ask p90, ms | Ask max, ms | Read p50, ms | Push p50, ms | Refused push p50, ms | Never past the budget |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| git daemon, 0.0 ms | 1 | all at once | 5 | 5 | 0 | 178 | 219 | 219 | 59 | 116 | None | yes |
| git daemon, 50.0 ms | 1 | all at once | 5 | 5 | 0 | 357 | 411 | 411 | 116 | 238 | None | yes |
| git daemon, 150.0 ms | 1 | all at once | 5 | 5 | 0 | 961 | 1115 | 1115 | 319 | 641 | None | yes |
| git daemon, 0.0 ms | 4 | all at once | 10 | 34 | 24 | 188 | 2238 | 2238 | 108 | 124 | 122 | yes |
| git daemon, 50.0 ms | 4 | all at once | 10 | 34 | 24 | 364 | 4133 | 4133 | 177 | 240 | 241 | yes |
| git daemon, 150.0 ms | 4 | all at once | 10 | 34 | 24 | 967 | 10868 | 10868 | 478 | 644 | 641 | yes |
