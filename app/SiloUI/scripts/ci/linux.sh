#!/usr/bin/env bash
# The Linux engine job: the engine, the daemon, the CLI and client crates, and
# silo-host with and without its desktop feature, plus a check that silod does
# not depend on GTK. Run linux-prepare.sh first.
#
#   linux.sh          every part
#   linux.sh engine   the engine tests
#   linux.sh daemon   the GTK check, then the daemon, CLI and client tests
#   linux.sh host     silo-host with and without its desktop feature
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

part=${1:-all}
[ $# -le 1 ] || die "takes at most one part"
case "$part" in all | engine | daemon | host) ;; *) die "unknown part '$part' (all, engine, daemon or host)" ;; esac

no_gtk() {
  local tree
  tree=$(cargo tree -p silod -e normal --locked --prefix none) || { echo "cargo tree failed for silod" >&2; return 1; }
  if grep -E '^(gtk|gdk|gio|glib|webkit2gtk)(-sys)? v' <<<"$tree"; then
    echo "silod depends on the GTK stack" >&2
    return 1
  fi
}

engine() {
  step engine cargo nextest run "${SILO_NEXTEST_FLAGS[@]}" -p silo-engine
}

daemon() {
  # The headless daemon must not need GTK: silo-host's desktop feature stays off.
  step silod-without-gtk no_gtk
  # One run for the daemon side, so silo-host builds without its desktop feature as in silod.
  step daemon cargo nextest run "${SILO_NEXTEST_FLAGS[@]}" -p silod -p silo-cli -p silo-client -p silo-api -p silo-container-shim
}

host() {
  step host cargo nextest run "${SILO_NEXTEST_FLAGS[@]}" -p silo-host
  step host-without-desktop cargo nextest run "${SILO_NEXTEST_FLAGS[@]}" -p silo-host --no-default-features
}

cd "$APP" || exit 2
case "$part" in
  all) engine; daemon; host ;;
  *) "$part" ;;
esac
finish
