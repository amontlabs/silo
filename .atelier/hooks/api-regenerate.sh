#!/bin/sh
# Runs both API generators when the merged tree differs from the integration branch under
# app/SiloUI/api/ (a merged change there, or a generated file removed for regeneration).
# Otherwise it changes nothing.
set -eu
cd "$(dirname "$0")/../.."
branch=$(sed -n 's/^integration_branch *= *"\(.*\)".*/\1/p' .atelier/atelier.toml)
branch=${branch:-main}
if [ -z "$(git diff --name-only "refs/heads/$branch" -- app/SiloUI/api/)" ]; then
  echo "api-regenerate: nothing under app/SiloUI/api/ changed"
  exit 0
fi
cd app/SiloUI
cargo run --locked -q -p silo-api --example generate_requests
cargo run --locked -q -p silo-api --example generate_schema
