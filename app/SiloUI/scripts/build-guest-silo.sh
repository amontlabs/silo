#!/usr/bin/env bash
# Builds the static Linux `silo` Silo puts into its Linux computers (the same build `npm run
# runtime:prepare` stages) and prints its path. Needs `zig` on PATH (or SILO_ZIG) and the Rust
# musl target. `silo tools enable <computer> --binary <path>` sends it as a development override.
set -euo pipefail
exec node "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/stage-guest-silo.mjs" "$@"
