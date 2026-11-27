### Team of 5, $15, `typical-cut`, 50 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, s | Longest wait, s | Calls that waited over 10 s, mean | Writes per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| #388's leases, 2 s round trip, with a recall | 0.99 | -0.1% | -3.0% | 89 | 180.0 | 180.0 | 1.2 | 286.98 |
| Git: expiry 5 min frees what was not reported | 0.99 | +3.0% | -1.9% | 89 | 265.4 | 265.4 | 1.6 | 1.25 |
| Git: check-in every 5 min, a slice stays the Mac's until it gives it back | 0.99 | -0.4% | -1.9% | 89 | 265.4 | 265.4 | 1.6 | 1.25 |
| Git: as above, $2 slices only | 0.98 | -0.5% | -2.6% | 90 | 319.7 | 319.7 | 1.6 | 1.17 |
| Git: as above, pushes 5 s | 0.99 | -0.2% | -2.7% | 90 | 310.2 | 310.2 | 2.4 | 1.10 |

### Team of 12, $36, `typical-cut`, 50 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, s | Longest wait, s | Calls that waited over 10 s, mean | Writes per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| #388's leases, 2 s round trip, with a recall | 1.00 | +0.0% | -1.3% | 219 | 24.0 | 43.8 | 5.2 | 17.03 |
| Git: expiry 5 min frees what was not reported | 1.00 | +3.8% | -1.1% | 220 | 809.1 | 824.9 | 15.2 | 1.64 |
| Git: check-in every 5 min, a slice stays the Mac's until it gives it back | 0.99 | -0.3% | -1.4% | 218 | 281.0 | 366.7 | 10.6 | 1.66 |
| Git: as above, $2 slices only | 0.99 | -0.3% | -1.6% | 218 | 224.1 | 395.5 | 5.6 | 1.75 |
| Git: as above, pushes 5 s | 0.99 | -0.4% | -1.5% | 217 | 256.0 | 629.6 | 21.2 | 1.12 |

### Team of 5, $15, `finish_tps` without a cut, 50 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, s | Longest wait, s | Calls that waited over 10 s, mean | Writes per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| #388's leases, 2 s round trip, with a recall | 1.09 | +14.0% | +5.1% | 100 | 8.0 | 8.0 | 0.0 | 2.31 |
| Git: expiry 5 min frees what was not reported | 1.10 | +14.0% | +6.2% | 101 | 3.2 | 3.4 | 0.0 | 0.77 |
| Git: check-in every 5 min, a slice stays the Mac's until it gives it back | 1.10 | +14.0% | +6.2% | 101 | 3.2 | 3.4 | 0.0 | 0.77 |
| Git: as above, $2 slices only | 1.10 | +13.8% | +8.0% | 103 | 28.4 | 29.3 | 0.1 | 0.63 |
| Git: as above, pushes 5 s | 1.09 | +14.1% | +6.3% | 102 | 59.9 | 60.0 | 0.8 | 0.65 |

### A heavy Mac goes to sleep holding its slice, team of 5, $15, `typical-cut`, 50 tokens/s

Its agents stop where they are and it sends nothing more. Under budget here is what the four awake Macs could not spend.

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, s | Longest wait, s | Calls that waited over 10 s, mean | Writes per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| #388's leases, 30 ms, a heavy Mac asleep from 300 s (its recall reaches no one) | 0.76 | -1.3% | -48.4% | 70 | 0.1 | 0.1 | 0.0 | 3.45 |
| Git: expiry 5 min frees what was not reported, a heavy Mac asleep from 300 s | 1.03 | +13.6% | -0.3% | 98 | 127.1 | 127.1 | 0.4 | 0.87 |
| Git: check-in every 5 min, a slice stays the Mac's until it gives it back, a heavy Mac asleep from 300 s | 0.83 | -6.8% | -49.9% | 78 | 724.3 | 724.3 | 0.6 | 0.91 |
| Git: as above, $2 slices only, a heavy Mac asleep from 300 s | 0.88 | -10.3% | -12.8% | 83 | 878.0 | 878.0 | 0.4 | 0.92 |

### GitHub cannot be reached, team of 5, $15, `typical-cut`, 50 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, s | Longest wait, s | Calls that waited over 10 s, mean | Writes per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| #388's leases, 30 ms, ledger down 1 minute from 300 s | 0.99 | -0.3% | -2.2% | 90 | 47.9 | 47.9 | 0.4 | 5.80 |
| Git: expiry 5 min frees what was not reported, GitHub down 1 minute from 300 s | 0.84 | +10.0% | -89.8% | 79 | 480.8 | 480.8 | 0.7 | 0.48 |
| Git: check-in every 5 min, a slice stays the Mac's until it gives it back, GitHub down 1 minute from 300 s | 0.99 | -0.7% | -2.7% | 90 | 265.4 | 265.4 | 2.2 | 1.15 |
| Git: as above, $2 slices only, GitHub down 1 minute from 300 s | 0.98 | -0.6% | -2.8% | 90 | 319.7 | 319.7 | 2.1 | 1.17 |
| #388's leases, 30 ms, ledger down 10 minutes from 600 s | 0.99 | +0.0% | -7.7% | 89 | 600.1 | 600.1 | 9.2 | 2.27 |
| Git: expiry 5 min frees what was not reported, GitHub down 10 minutes from 600 s | 1.32 | +41.7% | +13.9% | 119 | 645.4 | 645.4 | 10.3 | 0.38 |
| Git: check-in every 5 min, a slice stays the Mac's until it gives it back, GitHub down 10 minutes from 600 s | 0.99 | -0.3% | -2.2% | 90 | 1090.7 | 1090.7 | 5.0 | 0.79 |
| Git: as above, $2 slices only, GitHub down 10 minutes from 600 s | 0.99 | -0.5% | -3.0% | 89 | 637.5 | 637.5 | 2.6 | 0.72 |
