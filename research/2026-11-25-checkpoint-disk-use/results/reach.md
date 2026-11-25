| Option | light/mac-512 | medium/mac-512 | heavy/mac-512 | macos-no-trim/mac-512 | medium/mac-1tb |
|---|---|---|---|---|---|
| `baseline` | 17.3 | 4.3 | 1.2 | 2.4 | 14.3 |
| `lz4` | 29.3 | 17.3 | 6.3 | 8.3 | 29.3 |
| `zstd` | 29.3 | 26.3 | 10.3 | 11.3 | 29.3 |
| `zstd-pessimistic` | 29.3 | 10.3 | 2.3 | 5.3 | 29.3 |
| `dedup` | 29.3 | 8.3 | 2.1 | 4.3 | 26.3 |
| `dedup+zstd` | 29.3 | 29.3 | 18.3 | 17.3 | 29.3 |
| `zstd, base a clone` | 29.3 | 13.3 | 4.3 | 6.3 | 29.3 |
| `dedup+zstd, base a clone` | 29.3 | 15.3 | 5.3 | 7.3 | 29.3 |
| `zstd, base a clone, no TRIM` | 23.3 | 6.3 | 1.7 | 3.3 | 20.3 |
| `exclude-redirectable` | 29.3 | 10.3 | 3.3 | 7.3 | 28.3 |
| `exclude-all-rebuildable` | 29.3 | 25.3 | 10.3 | 29.3 | 29.3 |
| `exclude-redirectable+zstd` | 29.3 | 29.3 | 20.3 | 29.3 | 29.3 |
| `exclude-all+zstd` | 29.3 | 29.3 | 29.3 | 29.3 | 29.3 |
| `external-500` | 29.3 | 29.3 | 17.3 | 16.3 | 29.3 |
| `external-500+zstd` | 29.3 | 29.3 | 29.3 | 29.3 | 29.3 |
| `all` | 29.3 | 29.3 | 29.3 | 29.3 | 29.3 |
