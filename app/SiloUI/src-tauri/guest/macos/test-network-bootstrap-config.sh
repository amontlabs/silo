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

# A CA that is present but not trusted must not pass the readiness check. The fixture
# CA and the probe leaf the host signs with it stand in for the guest's staged files; only
# `--trust-ca-as-anchor` makes the evaluator trust the CA, as an installed trust setting does.
openssl=/usr/bin/openssl
cat >"$tmp_dir/ca.cnf" <<'CNF'
[req]
distinguished_name = dn
x509_extensions = v3
prompt = no
[dn]
CN = Silo test CA
[v3]
basicConstraints = critical,CA:TRUE
keyUsage = critical,keyCertSign,cRLSign
subjectKeyIdentifier = hash
CNF
make_leaf_config() {
	cat >"$tmp_dir/$1.cnf" <<CNF
[req]
distinguished_name = dn
prompt = no
[dn]
CN = $2
[v3]
basicConstraints = CA:FALSE
keyUsage = digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = DNS:$2
authorityKeyIdentifier = keyid
CNF
}
sign_leaf() {
	"$openssl" ecparam -name prime256v1 -genkey -noout -out "$tmp_dir/$1-key.pem" 2>/dev/null
	"$openssl" req -new -key "$tmp_dir/$1-key.pem" -config "$tmp_dir/$1.cnf" -out "$tmp_dir/$1.csr"
	"$openssl" x509 -req -in "$tmp_dir/$1.csr" -CA "$tmp_dir/ca.pem" -CAkey "$tmp_dir/ca-key.pem" 		-sha256 -CAcreateserial -days 365 -extfile "$tmp_dir/$1.cnf" -extensions v3 -out "$tmp_dir/$1.pem" 2>/dev/null
}
"$openssl" ecparam -name prime256v1 -genkey -noout -out "$tmp_dir/ca-key.pem" 2>/dev/null
"$openssl" req -new -x509 -sha256 -key "$tmp_dir/ca-key.pem" -days 3650 -config "$tmp_dir/ca.cnf" -out "$tmp_dir/ca.pem"
make_leaf_config probe silo-ca-trust-probe.invalid
sign_leaf probe
make_leaf_config other other.invalid
sign_leaf other
fingerprint=$("$openssl" x509 -in "$tmp_dir/ca.pem" -noout -fingerprint -sha256 | sed 's/^[^=]*=//' | tr -d ':' | tr 'A-F' 'a-f')
evaluate() {
	"$tmp_dir/silo-network-bootstrap" --evaluate-ca-trust "$@" 2>/dev/null || true
}
expect() {
	if [ "$2" != "$1" ]; then
		printf 'CA trust check: expected %s, got %s (%s)\n' "$1" "$2" "$3" >&2
		exit 1
	fi
}
expect untrusted "$(evaluate "$tmp_dir/probe.pem" "$tmp_dir/ca.pem" "$fingerprint")" 'present but untrusted CA'
expect trusted "$(evaluate "$tmp_dir/probe.pem" "$tmp_dir/ca.pem" "$fingerprint" --trust-ca-as-anchor)" 'trusted CA'
expect untrusted "$(evaluate "$tmp_dir/probe.pem" "$tmp_dir/ca.pem" "$(printf '%064d' 0)" --trust-ca-as-anchor)" 'another CA fingerprint'
expect untrusted "$(evaluate "$tmp_dir/other.pem" "$tmp_dir/ca.pem" "$fingerprint" --trust-ca-as-anchor)" 'leaf for another name'
expect untrusted "$(evaluate "$tmp_dir/missing.pem" "$tmp_dir/ca.pem" "$fingerprint" --trust-ca-as-anchor)" 'missing leaf'

sh "$script_dir/test-remove-inherited-proxy-ca.sh"

printf '%s\n' 'network bootstrap config checks passed'
