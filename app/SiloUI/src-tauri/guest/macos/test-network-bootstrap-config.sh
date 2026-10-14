#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

"$script_dir/build-network-bootstrap.sh" "$tmp_dir/silo-network-bootstrap"

cat >"$tmp_dir/valid.json" <<'JSON'
{
  "version": 1,
  "computerId": "test-computer",
  "expectedMac": "02:00:00:00:00:02",
  "address": "172.30.0.2",
  "prefixLength": 30,
  "router": "172.30.0.1",
  "dnsServer": "172.30.0.1",
  "caFingerprint": "0000000000000000000000000000000000000000000000000000000000000000",
  "probeURL": "https://github.com/"
}
JSON

"$tmp_dir/silo-network-bootstrap" --config "$tmp_dir/valid.json" --validate-only >/dev/null

sed 's/"prefixLength": 30/"prefixLength": 24/' "$tmp_dir/valid.json" >"$tmp_dir/invalid.json"
if "$tmp_dir/silo-network-bootstrap" --config "$tmp_dir/invalid.json" --validate-only >/dev/null 2>&1; then
	printf '%s\n' 'invalid /24 config unexpectedly passed validation' >&2
	exit 1
fi

sh "$script_dir/test-remove-inherited-proxy-ca.sh"

printf '%s\n' 'network bootstrap config checks passed'
