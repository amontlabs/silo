#!/usr/bin/env bash
# Prepares a Linux host for linux.sh: stages the bundled Git (pinned and
# checksummed) that the host-push tests drive. The MicroSandbox runtime is not
# needed. Needs Node and rustc on PATH; run node scripts/preflight.mjs first.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$APP" || exit 2
target=$(rustc --print host-tuple) || die "rustc is not on PATH"
node --input-type=module -e '
import { stageGitRuntime } from "./scripts/git-runtime.mjs";
import { fetchStream } from "./scripts/build-input.mjs";
await stageGitRuntime({ appRoot: process.cwd(), targetTriple: process.argv[1], fetchStream })' "$target" \
  || die "staging the bundled Git runtime failed"
echo "prepared: bundled Git for $target"
