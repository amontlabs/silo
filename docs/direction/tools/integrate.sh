#!/bin/bash
# Regenerates the API files and runs the focused suites on the integration branch.
cd ~/code/projects/silo-direction/app/SiloUI || exit 1
LOG=$HOME/code/projects/silo-direction/app/SiloUI/target/integration.log
: > "$LOG"
run() { echo "== $*" >> "$LOG"; "$@" >> "$LOG" 2>&1; echo "== exit $? : $*" >> "$LOG"; }
run cargo run -q -p silo-api --example generate_requests
run cargo run -q -p silo-api --example generate_schema
run cargo test -p silo-api --locked
run cargo test -p silod --locked
run cargo test -p silo-cli --locked
run cargo test -p silo-engine --locked --lib -- policy:: audit:: guest_client:: changes:: macos_computers rebuild guest_components recipes network_policy resources checkpoints
run cargo +1.94.0 fmt --manifest-path src-tauri/Cargo.toml --check
echo "== DONE" >> "$LOG"
