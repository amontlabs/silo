#!/usr/bin/env bash
# The Linux engine job: the engine, the daemon, the CLI and client crates, and
# silo-host with and without its desktop feature, plus a check that silod does
# not depend on GTK. Run linux-prepare.sh first.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

no_gtk() {
  if cargo tree -p silod -e normal --locked --prefix none | grep -E '^(gtk|gdk|gio|glib|webkit2gtk)(-sys)? v'; then
    echo "silod depends on the GTK stack" >&2
    return 1
  fi
}

cd "$APP" || exit 2
step engine cargo nextest run "${SILO_NEXTEST_FLAGS[@]}" -p silo-engine
# The headless daemon must not need GTK: silo-host's desktop feature stays off.
step silod-without-gtk no_gtk
# One run for the daemon side, so silo-host builds without its desktop feature as in silod.
step daemon cargo nextest run "${SILO_NEXTEST_FLAGS[@]}" -p silod -p silo-cli -p silo-client -p silo-api -p silo-container-shim
step host cargo nextest run "${SILO_NEXTEST_FLAGS[@]}" -p silo-host
step host-without-desktop cargo nextest run "${SILO_NEXTEST_FLAGS[@]}" -p silo-host --no-default-features
finish
