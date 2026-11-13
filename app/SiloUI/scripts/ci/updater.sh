#!/usr/bin/env bash
# The patched updater plugin's own unit tests (transport and atomic
# installation). It is not a workspace member, so it runs on its own.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

cd "$APP" || exit 2
step updater cargo test --manifest-path src-tauri/Cargo.toml --locked -p tauri-plugin-updater --lib
finish
