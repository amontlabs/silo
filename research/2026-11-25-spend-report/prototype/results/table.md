| Run | Calls billed | Exchanges seen | Events | Matched | Exchanges with no event | Events with no exchange | Flags | Report's state | Billed | Reported | Floor for the rest | Estimate by bytes | Estimate by the conversation | Billed for the rest |
|---|---:|---:|---:|---:|---:|---:|---|---|---:|---:|---:|---:|---:|---:|
| baseline | 7 | 7 | 7 | 7 | 0 | 0 | - | whole | 0.1738 | 0.1738 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| both-none | 7 | 7 | 0 | 0 | 7 | 0 | - | silent: no report at all | 0.1738 | 0.0000 | 0.0094 | - | 0.1668 | 0.1738 |
| disable-telemetry | 7 | 7 | 7 | 7 | 0 | 0 | - | whole | 0.1767 | 0.1767 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| endpoint-away | 7 | 7 | 0 | 0 | 7 | 0 | - | silent: no report at all | 0.1738 | 0.0000 | 0.0094 | - | 0.1668 | 0.1738 |
| env-off | 7 | 7 | 0 | 0 | 7 | 0 | - | silent: no report at all | 0.1738 | 0.0000 | 0.0094 | - | 0.1668 | 0.1738 |
| env-unset | 7 | 7 | 0 | 0 | 7 | 0 | - | silent: no report at all | 0.1738 | 0.0000 | 0.0094 | - | 0.1668 | 0.1738 |
| forged-event | 5 | 5 | 6 | 5 | 0 | 1 | no-exchange | contradicted by what the boundary saw | 0.1545 | 0.4345 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| late-interval | 7 | 7 | 7 | 7 | 0 | 0 | - | whole | 0.1738 | 0.1738 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| late-killed | 18 | 18 | 0 | 0 | 18 | 0 | - | silent: no report at all | 0.3269 | 0.0000 | 0.0273 | - | 0.2803 | 0.3269 |
| logs-none | 7 | 7 | 0 | 0 | 7 | 0 | - | silent: no report at all | 0.1738 | 0.0000 | 0.0094 | - | 0.1668 | 0.1738 |
| long | 31 | 31 | 31 | 31 | 0 | 0 | - | whole | 0.4851 | 0.4851 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| long-visible | 31 | 31 | 31 | 31 | 0 | 0 | - | whole | 0.4851 | 0.4851 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| managed-on-endpoint-away | 7 | 7 | 0 | 0 | 7 | 0 | - | silent: no report at all | 0.1738 | 0.0000 | 0.0094 | - | 0.1668 | 0.1738 |
| managed-on-env-off | 7 | 7 | 7 | 7 | 0 | 0 | - | whole | 0.1738 | 0.1738 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| managed-on-project-off | 7 | 7 | 7 | 7 | 0 | 0 | - | whole | 0.1739 | 0.1739 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| midrun-project-off | 9 | 9 | 9 | 9 | 0 | 0 | - | whole | 0.1885 | 0.1885 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| midrun-user-off | 9 | 9 | 9 | 9 | 0 | 0 | - | whole | 0.1885 | 0.1885 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| nonessential | 7 | 7 | 7 | 7 | 0 | 0 | - | whole | 0.1766 | 0.1766 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| project-off | 7 | 7 | 7 | 7 | 0 | 0 | - | whole | 0.1739 | 0.1739 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| protocol-grpc | 7 | 7 | 0 | 0 | 7 | 0 | - | silent: no report at all | 0.1738 | 0.0000 | 0.0094 | - | 0.1668 | 0.1738 |
| protocol-proto | 7 | 7 | 0 | 0 | 7 | 0 | - | silent: no report at all | 0.1738 | 0.0000 | 0.0094 | - | 0.1668 | 0.1738 |
| sdk-disabled | 7 | 7 | 7 | 7 | 0 | 0 | - | whole | 0.1738 | 0.1738 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| short-shutdown | 7 | 7 | 7 | 7 | 0 | 0 | - | whole | 0.1738 | 0.1738 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| tampered-deflate | 31 | 31 | 31 | 31 | 0 | 0 | input-below-bytes, output-below-bytes | contradicted by what the boundary saw | 0.4851 | 0.0484 | 0.0000 | - | 0.4334 | 0.4851 |
| tampered-output-only | 31 | 31 | 31 | 31 | 0 | 0 | input-below-bytes | contradicted by what the boundary saw | 0.4851 | 0.1860 | 0.0000 | - | 0.4334 | 0.4851 |
| tampered-relabel | 31 | 31 | 31 | 31 | 0 | 0 | cache-read-never-sent | contradicted by what the boundary saw | 0.4851 | 0.2894 | 0.0000 | - | 0.4334 | 0.4851 |
| tampered-silence-half | 31 | 31 | 15 | 15 | 16 | 0 | - | silent since exchange 16 of 31 | 0.4851 | 0.2888 | 0.0312 | 0.4082 | 0.1932 | 0.1964 |
| user-off | 7 | 7 | 0 | 0 | 7 | 0 | - | silent: no report at all | 0.1738 | 0.0000 | 0.0094 | - | 0.1668 | 0.1738 |
