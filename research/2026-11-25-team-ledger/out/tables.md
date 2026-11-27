### One Mac, $3, stand-in speed (500 tokens/s): the simulator against #374

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| finish | 1.41 | +49.8% | +28.0% | 24 | 0 | 0.0 | 0.0 | 0.00 |
| finish_tps | 1.22 | +35.0% | +10.7% | 20 | 0 | 0.0 | 0.0 | 0.00 |
| admit_input | 1.22 | +35.0% | +10.7% | 20 | 0 | 0.0 | 0.0 | 0.00 |
| reserve_typical | 0.96 | -1.8% | -6.4% | 15 | 0 | 0.0 | 0.0 | 0.00 |
| reserve_max | 0.17 | -78.9% | -85.3% | 3 | 0 | 0.0 | 0.0 | 0.00 |

### Team of 5 (2 heavy, 1 medium, 1 light, 1 idle), $15, finish_tps, 500 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact: one boundary sees all | 1.10 | +14.3% | +6.4% | 101 | 0 | 0.0 | 0.0 | 0.00 |
| central ledger, 1 ms (a Linux host on the LAN) | 1.10 | +14.3% | +6.4% | 101 | 1 | 0.0 | 0.0 | 3.44 |
| central ledger, 30 ms (a host over a VPN, or one region) | 1.10 | +13.0% | +6.7% | 102 | 30 | 0.0 | 0.0 | 3.43 |
| central ledger, 150 ms (another continent) | 1.10 | +14.4% | +5.4% | 103 | 150 | 0.1 | 0.0 | 3.43 |
| central on a shared Linux host: 2 members' computers on it, 3 Macs at 30 ms | 1.10 | +13.4% | +7.7% | 101 | 30 | 0.0 | 0.0 | 3.44 |
| leases, 30 ms | 1.07 | +10.5% | +1.0% | 98 | 840 | 0.8 | 0.0 | 2.76 |
| leases, 30 ms, no recall | 1.06 | +11.4% | -3.0% | 98 | 30 | 0.0 | 0.0 | 0.18 |
| leases, 2 s (a Git push as the ledger) | 1.08 | +12.6% | +0.7% | 98 | 8000 | 8.0 | 0.0 | 2.08 |
| gossip every 1 s | 1.10 | +14.3% | +6.9% | 102 | 0 | 0.0 | 0.0 | 29.27 |
| gossip every 10 s | 1.14 | +20.1% | +9.8% | 106 | 0 | 0.0 | 0.0 | 2.88 |
| static fifths | 0.90 | -6.4% | -13.5% | 81 | 0 | 0.0 | 0.0 | 0.00 |

### Team of 5 (2 heavy, 1 medium, 1 light, 1 idle), $15, reserve_typical, 500 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact: one boundary sees all | 0.99 | -0.0% | -1.5% | 91 | 0 | 0.0 | 0.0 | 0.00 |
| central ledger, 1 ms (a Linux host on the LAN) | 0.99 | -0.0% | -1.5% | 91 | 1 | 0.0 | 0.0 | 3.48 |
| central ledger, 30 ms (a host over a VPN, or one region) | 0.99 | +0.1% | -1.5% | 90 | 30 | 0.0 | 0.0 | 3.49 |
| central ledger, 150 ms (another continent) | 0.99 | -0.1% | -1.5% | 91 | 150 | 0.1 | 0.0 | 3.48 |
| central on a shared Linux host: 2 members' computers on it, 3 Macs at 30 ms | 0.99 | -0.3% | -1.5% | 90 | 30 | 0.0 | 0.0 | 3.49 |
| leases, 30 ms | 0.99 | -0.0% | -1.9% | 91 | 180 | 0.2 | 0.0 | 5.91 |
| leases, 30 ms, no recall | 0.95 | -0.8% | -15.5% | 87 | 30 | 0.0 | 0.0 | 0.27 |
| leases, 2 s (a Git push as the ledger) | 0.99 | +0.0% | -4.1% | 90 | 44000 | 44.0 | 2.7 | 215.89 |
| gossip every 1 s | 0.97 | -0.5% | -4.5% | 88 | 0 | 0.0 | 0.0 | 34.44 |
| gossip every 10 s | 0.98 | +0.3% | -4.6% | 89 | 0 | 0.0 | 0.0 | 3.24 |
| static fifths | 0.76 | -22.0% | -24.9% | 67 | 0 | 0.0 | 0.0 | 0.00 |

### Team of 5 (2 heavy, 1 medium, 1 light, 1 idle), $15, finish, 500 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact: one boundary sees all | 1.19 | +23.4% | +16.2% | 111 | 0 | 0.0 | 0.0 | 0.00 |
| central ledger, 1 ms (a Linux host on the LAN) | 1.19 | +23.4% | +16.2% | 111 | 1 | 0.0 | 0.0 | 3.40 |
| central ledger, 30 ms (a host over a VPN, or one region) | 1.20 | +22.4% | +17.0% | 112 | 30 | 0.0 | 0.0 | 3.40 |
| central ledger, 150 ms (another continent) | 1.20 | +25.8% | +15.6% | 112 | 150 | 0.1 | 0.0 | 3.39 |
| central on a shared Linux host: 2 members' computers on it, 3 Macs at 30 ms | 1.20 | +22.9% | +16.5% | 111 | 30 | 0.0 | 0.0 | 3.40 |
| leases, 30 ms | 1.13 | +16.4% | +6.7% | 104 | 90 | 0.1 | 0.0 | 2.58 |
| leases, 30 ms, no recall | 1.15 | +21.1% | +4.3% | 107 | 30 | 0.0 | 0.0 | 0.17 |
| leases, 2 s (a Git push as the ledger) | 1.14 | +21.0% | +7.5% | 104 | 8000 | 8.0 | 0.0 | 1.88 |
| gossip every 1 s | 1.20 | +23.4% | +16.2% | 112 | 0 | 0.0 | 0.0 | 27.98 |
| gossip every 10 s | 1.23 | +26.7% | +19.7% | 116 | 0 | 0.0 | 0.0 | 2.76 |
| static fifths | 0.98 | +1.2% | -6.4% | 88 | 0 | 0.0 | 0.0 | 0.00 |

### Team of 5 (2 heavy, 1 medium, 1 light, 1 idle), $15, finish_tps, 50 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact: one boundary sees all | 1.11 | +14.6% | +9.1% | 103 | 0 | 0.0 | 0.0 | 0.00 |
| central ledger, 1 ms (a Linux host on the LAN) | 1.11 | +14.6% | +9.1% | 103 | 1 | 0.0 | 0.0 | 3.43 |
| central ledger, 30 ms (a host over a VPN, or one region) | 1.11 | +14.6% | +8.2% | 103 | 30 | 0.0 | 0.0 | 3.43 |
| central ledger, 150 ms (another continent) | 1.11 | +14.9% | +9.1% | 103 | 150 | 0.1 | 0.0 | 3.43 |
| central on a shared Linux host: 2 members' computers on it, 3 Macs at 30 ms | 1.11 | +14.6% | +9.2% | 103 | 30 | 0.0 | 0.0 | 3.43 |
| leases, 30 ms | 1.07 | +12.3% | +0.5% | 99 | 120 | 0.1 | 0.0 | 2.35 |
| leases, 30 ms, no recall | 1.08 | +13.5% | -3.6% | 99 | 30 | 0.0 | 0.0 | 0.18 |
| leases, 2 s (a Git push as the ledger) | 1.09 | +14.0% | +5.1% | 100 | 8000 | 8.0 | 0.0 | 2.31 |
| gossip every 1 s | 1.11 | +14.6% | +9.1% | 103 | 0 | 0.0 | 0.0 | 242.73 |
| gossip every 10 s | 1.12 | +14.8% | +9.1% | 103 | 0 | 0.0 | 0.0 | 24.28 |
| static fifths | 0.91 | -5.2% | -12.6% | 82 | 0 | 0.0 | 0.0 | 0.00 |

### Team of 5 (2 heavy, 1 medium, 1 light, 1 idle), $15, reserve_typical, 50 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact: one boundary sees all | 0.99 | -0.1% | -1.5% | 90 | 0 | 0.0 | 0.0 | 0.00 |
| central ledger, 1 ms (a Linux host on the LAN) | 0.99 | -0.1% | -1.5% | 90 | 1 | 0.0 | 0.0 | 3.49 |
| central ledger, 30 ms (a host over a VPN, or one region) | 0.99 | -0.1% | -1.5% | 90 | 30 | 0.0 | 0.0 | 3.49 |
| central ledger, 150 ms (another continent) | 0.99 | -0.2% | -1.5% | 90 | 150 | 0.1 | 0.0 | 3.49 |
| central on a shared Linux host: 2 members' computers on it, 3 Macs at 30 ms | 0.99 | -0.1% | -1.5% | 91 | 30 | 0.0 | 0.0 | 3.48 |
| leases, 30 ms | 0.99 | +0.1% | -2.2% | 90 | 180 | 0.2 | 0.0 | 5.83 |
| leases, 30 ms, no recall | 0.95 | -0.1% | -16.3% | 88 | 30 | 0.0 | 0.0 | 0.27 |
| leases, 2 s (a Git push as the ledger) | 0.99 | +0.0% | -3.8% | 90 | 48000 | 48.0 | 0.8 | 62.14 |
| gossip every 1 s | 0.97 | -1.3% | -4.1% | 89 | 0 | 0.0 | 0.0 | 287.80 |
| gossip every 10 s | 0.97 | -1.3% | -4.6% | 89 | 0 | 0.0 | 0.0 | 28.53 |
| static fifths | 0.76 | -22.1% | -25.7% | 67 | 0 | 0.0 | 0.0 | 0.00 |

### Team of 5 (2 heavy, 1 medium, 1 light, 1 idle), $15, finish, 50 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact: one boundary sees all | 1.20 | +23.6% | +18.8% | 112 | 0 | 0.0 | 0.0 | 0.00 |
| central ledger, 1 ms (a Linux host on the LAN) | 1.20 | +23.6% | +18.8% | 112 | 1 | 0.0 | 0.0 | 3.39 |
| central ledger, 30 ms (a host over a VPN, or one region) | 1.21 | +23.6% | +19.0% | 112 | 30 | 0.0 | 0.0 | 3.39 |
| central ledger, 150 ms (another continent) | 1.20 | +22.6% | +18.6% | 112 | 150 | 0.1 | 0.0 | 3.39 |
| central on a shared Linux host: 2 members' computers on it, 3 Macs at 30 ms | 1.20 | +23.6% | +18.3% | 112 | 30 | 0.0 | 0.0 | 3.39 |
| leases, 30 ms | 1.13 | +18.6% | +3.3% | 105 | 90 | 0.3 | 0.0 | 2.20 |
| leases, 30 ms, no recall | 1.16 | +23.6% | +4.6% | 107 | 30 | 0.0 | 0.0 | 0.17 |
| leases, 2 s (a Git push as the ledger) | 1.12 | +18.4% | +6.8% | 104 | 8000 | 12.0 | 0.1 | 2.42 |
| gossip every 1 s | 1.20 | +23.6% | +18.8% | 112 | 0 | 0.0 | 0.0 | 234.02 |
| gossip every 10 s | 1.21 | +23.7% | +18.8% | 113 | 0 | 0.0 | 0.0 | 23.35 |
| static fifths | 0.99 | +1.7% | -4.1% | 90 | 0 | 0.0 | 0.0 | 0.00 |

### Team of 12 (4 heavy, 4 medium, 3 light, 1 idle), $36, finish_tps, 50 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact: one boundary sees all | 1.10 | +11.9% | +8.0% | 244 | 0 | 0.0 | 0.0 | 0.00 |
| central ledger, 1 ms (a Linux host on the LAN) | 1.10 | +11.9% | +8.0% | 244 | 1 | 0.0 | 0.0 | 3.42 |
| central ledger, 30 ms (a host over a VPN, or one region) | 1.10 | +11.9% | +7.7% | 244 | 30 | 0.0 | 0.0 | 3.42 |
| central ledger, 150 ms (another continent) | 1.10 | +11.7% | +8.2% | 242 | 150 | 0.1 | 0.0 | 3.42 |
| leases, 30 ms | 1.10 | +11.8% | +6.9% | 242 | 90 | 0.1 | 0.0 | 5.66 |
| leases, 30 ms, no recall | 0.87 | +8.9% | -28.7% | 192 | 30 | 0.0 | 0.0 | 0.21 |
| leases, 2 s (a Git push as the ledger) | 1.10 | +12.0% | +6.9% | 243 | 6000 | 12.0 | 0.1 | 5.97 |
| gossip every 1 s | 1.10 | +12.5% | +8.0% | 244 | 0 | 0.0 | 0.0 | 695.86 |
| gossip every 10 s | 1.11 | +13.1% | +8.1% | 245 | 0 | 0.0 | 0.0 | 69.40 |
| static fifths | 1.02 | +3.1% | -0.2% | 223 | 0 | 0.0 | 0.0 | 0.00 |

### Team of 12 (4 heavy, 4 medium, 3 light, 1 idle), $36, reserve_typical, 50 tokens/s

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact: one boundary sees all | 1.00 | -0.2% | -0.6% | 218 | 0 | 0.0 | 0.0 | 0.00 |
| central ledger, 1 ms (a Linux host on the LAN) | 1.00 | -0.2% | -0.6% | 218 | 1 | 0.0 | 0.0 | 3.47 |
| central ledger, 30 ms (a host over a VPN, or one region) | 1.00 | -0.1% | -0.6% | 219 | 30 | 0.0 | 0.0 | 3.47 |
| central ledger, 150 ms (another continent) | 1.00 | -0.0% | -0.6% | 217 | 150 | 0.1 | 0.0 | 3.47 |
| leases, 30 ms | 1.00 | -0.1% | -0.6% | 218 | 120 | 4.3 | 0.0 | 17.04 |
| leases, 30 ms, no recall | 0.79 | -0.4% | -35.5% | 176 | 30 | 0.0 | 0.0 | 0.31 |
| leases, 2 s (a Git push as the ledger) | 1.00 | -0.1% | -1.0% | 219 | 31373 | 64.0 | 5.0 | 72.01 |
| gossip every 1 s | 0.97 | -2.2% | -4.7% | 211 | 0 | 0.0 | 0.0 | 840.40 |
| gossip every 10 s | 0.96 | -2.6% | -6.2% | 209 | 0 | 0.0 | 0.0 | 83.12 |
| static fifths | 0.87 | -12.1% | -14.1% | 187 | 0 | 0.0 | 0.0 | 0.00 |

### Faults, team of 5, $15, finish_tps, 50 tokens/s: 10 minutes without the ledger, or the team cut in two

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact (no fault possible) | 1.11 | +14.6% | +9.1% | 103 | 0 | 0.0 | 0.0 | 0.00 |
| central, calls wait for the ledger | 1.09 | +13.6% | +6.3% | 100 | 592480 | 592.5 | 15.2 | 3.44 |
| central, calls refused while it is down | 0.52 | -34.7% | -68.9% | 43 | 30 | 0.0 | 0.0 | 4.07 |
| leases, the ledger down | 1.07 | +11.6% | +1.3% | 98 | 595971 | 596.0 | 11.1 | 1.41 |
| gossip every 1 s, team cut in two | 1.31 | +44.9% | +19.1% | 124 | 0 | 0.0 | 0.0 | 169.02 |
| gossip every 1 s, cut, escrow on silence | 1.02 | +2.5% | +0.5% | 93 | 0 | 0.0 | 0.0 | 333.34 |
| central, calls wait, ledger down from 200 s | 1.10 | +14.5% | +7.4% | 101 | 599321 | 599.3 | 6.5 | 3.43 |
| leases, ledger down from 200 s | 1.06 | +11.9% | +1.2% | 97 | 427692 | 428.3 | 7.5 | 1.47 |
| central, calls wait, ledger down 1 minute from 300 s | 1.11 | +14.3% | +7.7% | 103 | 51880 | 59.8 | 2.7 | 3.43 |
| leases, ledger down 1 minute from 300 s | 1.07 | +12.3% | +1.1% | 99 | 32236 | 32.2 | 0.1 | 2.32 |
| central, calls wait, ledger down 2 minutes from 300 s | 1.10 | +13.3% | +6.5% | 102 | 119757 | 119.8 | 6.7 | 3.43 |
| leases, ledger down 2 minutes from 300 s | 1.07 | +12.3% | +0.6% | 99 | 92236 | 92.2 | 0.3 | 2.22 |

### Faults, team of 5, $15, reserve_typical, 50 tokens/s: 10 minutes without the ledger, or the team cut in two

| Way | Spent / budget, mean | Over, worst | Over, best | Calls done, mean | Wait p99, ms | Longest wait, s | Calls that waited over 10 s, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| exact (no fault possible) | 0.99 | -0.1% | -1.5% | 90 | 0 | 0.0 | 0.0 | 0.00 |
| central, calls wait for the ledger | 0.99 | -0.3% | -1.4% | 90 | 592480 | 592.5 | 14.6 | 3.49 |
| central, calls refused while it is down | 0.52 | -34.7% | -68.9% | 43 | 30 | 0.0 | 0.0 | 4.07 |
| leases, the ledger down | 0.99 | +0.2% | -1.9% | 90 | 600082 | 600.1 | 8.8 | 2.19 |
| gossip every 1 s, team cut in two | 1.27 | +32.5% | +19.1% | 119 | 0 | 0.0 | 0.0 | 172.66 |
| gossip every 1 s, cut, escrow on silence | 0.98 | -1.1% | -3.6% | 89 | 0 | 0.0 | 0.0 | 718.94 |
| central, calls wait, ledger down from 200 s | 0.99 | -0.2% | -1.5% | 90 | 599321 | 599.3 | 6.5 | 3.49 |
| leases, ledger down from 200 s | 0.99 | -0.0% | -1.6% | 92 | 578955 | 579.0 | 4.5 | 2.69 |
| central, calls wait, ledger down 1 minute from 300 s | 0.99 | +0.4% | -1.5% | 90 | 59757 | 59.8 | 2.7 | 3.49 |
| leases, ledger down 1 minute from 300 s | 0.99 | -0.3% | -2.2% | 90 | 51910 | 51.9 | 0.5 | 5.77 |
| central, calls wait, ledger down 2 minutes from 300 s | 0.99 | -0.1% | -1.4% | 90 | 119757 | 119.8 | 6.7 | 3.49 |
| leases, ledger down 2 minutes from 300 s | 0.99 | -0.2% | -2.2% | 90 | 111940 | 111.9 | 1.2 | 5.28 |

### One Mac, $3, stand-in speed, with #385's cut: the simulator against #385

| Way | Spent / budget, mean | Over, worst | Over, best | Paid for cut answers, mean | Calls done, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|
| finish_tps and cut | 1.00 | +0.3% | -0.1% | $0.57 | 13 | 0.00 |
| reserve_typical and cut | 0.96 | -1.8% | -6.4% | $0.00 | 15 | 0.00 |

### Team of 5, $15, 50 tokens/s, #385's `cut`: calls under way are cut once the budget is passed

| Way | Spent / budget, mean | Over, worst | Over, best | Paid for cut answers, mean | Calls done, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|
| exact: one boundary sees all | 1.00 | +0.0% | +0.0% | $1.43 | 84 | 0.00 |
| central ledger, 1 ms (a Linux host on the LAN) | 1.00 | +0.0% | +0.0% | $1.43 | 84 | 3.82 |
| central ledger, 30 ms (a host over a VPN, or one region) | 1.00 | +0.0% | +0.0% | $1.44 | 84 | 3.82 |
| central ledger, 150 ms (another continent) | 1.00 | +0.0% | +0.0% | $1.47 | 84 | 3.82 |
| central on a shared Linux host: 2 members' computers on it, 3 Macs at 30 ms | 1.00 | +0.1% | +0.0% | $1.41 | 84 | 3.81 |
| leases, 30 ms | 1.00 | +0.0% | +0.0% | $1.85 | 82 | 25.29 |
| leases, 2 s (a Git push as the ledger) | 1.00 | +0.2% | -10.1% | $1.89 | 81 | 1.72 |
| gossip every 1 s | 1.00 | +0.1% | +0.0% | $1.44 | 84 | 249.69 |
| gossip every 10 s | 1.00 | +0.6% | +0.2% | $1.45 | 84 | 24.95 |
| static fifths | 0.80 | -19.9% | -20.0% | $1.40 | 63 | 0.00 |
| gossip every 1 s, team cut in two | 1.21 | +35.3% | +8.3% | $1.60 | 105 | 162.64 |
| gossip every 1 s, cut in two, escrow on silence | 1.00 | +0.0% | +0.0% | $1.56 | 83 | 453.57 |
| central, ledger down 10 minutes from 600 s, calls wait | 1.00 | +0.0% | +0.0% | $1.18 | 85 | 3.76 |
| leases, ledger down 10 minutes from 600 s | 1.00 | +0.0% | +0.0% | $1.52 | 83 | 37.57 |

### Team of 5, $15, 50 tokens/s, #385's `typical-cut`: calls under way are cut once the budget is passed

| Way | Spent / budget, mean | Over, worst | Over, best | Paid for cut answers, mean | Calls done, mean | Messages per call |
|---|---:|---:|---:|---:|---:|---:|
| exact: one boundary sees all | 0.99 | -0.1% | -1.5% | $0.00 | 90 | 0.00 |
| central ledger, 1 ms (a Linux host on the LAN) | 0.99 | -0.1% | -1.5% | $0.00 | 90 | 3.49 |
| central ledger, 30 ms (a host over a VPN, or one region) | 0.99 | -0.1% | -1.5% | $0.00 | 90 | 3.49 |
| central ledger, 150 ms (another continent) | 0.99 | -0.2% | -1.5% | $0.00 | 90 | 3.49 |
| central on a shared Linux host: 2 members' computers on it, 3 Macs at 30 ms | 0.99 | -0.1% | -1.5% | $0.00 | 91 | 3.48 |
| leases, 30 ms | 0.99 | +0.0% | -2.2% | $0.09 | 90 | 5.69 |
| leases, 2 s (a Git push as the ledger) | 0.99 | -0.1% | -3.0% | $0.20 | 89 | 286.98 |
| gossip every 1 s | 0.97 | -1.3% | -4.1% | $0.03 | 88 | 289.36 |
| gossip every 10 s | 0.97 | -1.3% | -4.5% | $0.07 | 88 | 28.76 |
| static fifths | 0.76 | -22.1% | -25.7% | $0.00 | 67 | 0.00 |
| gossip every 1 s, team cut in two | 1.19 | +24.5% | +8.3% | $1.39 | 104 | 164.28 |
| gossip every 1 s, cut in two, escrow on silence | 0.98 | -1.1% | -3.6% | $0.01 | 89 | 719.25 |
| central, ledger down 10 minutes from 600 s, calls wait | 0.99 | -0.3% | -1.4% | $0.00 | 90 | 3.49 |
| leases, ledger down 10 minutes from 600 s | 0.99 | +0.0% | -7.7% | $0.12 | 89 | 2.27 |

### Team of 5, $15, `typical-cut`, and one batch with a $4 bound held until its results (#385)

| Way | Batch admitted, share of runs | Spent / budget, mean | Over, worst | Over, best | Calls done, mean |
|---|---:|---:|---:|---:|---:|
| one boundary | 100% | 0.82 | -16.8% | -18.1% | 63 |
| central ledger, 30 ms | 100% | 0.82 | -16.8% | -18.1% | 63 |
| leases, 30 ms | 100% | 0.82 | -16.9% | -18.2% | 62 |
| gossip every 1 s | 100% | 0.80 | -18.4% | -20.9% | 61 |
| fixed fifths ($3 each) | 0% | 0.76 | -22.1% | -25.7% | 67 |
