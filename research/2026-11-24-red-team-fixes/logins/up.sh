#!/bin/bash
# Start the fake providers (127.0.0.1:8443) and the boundary (0.0.0.0:443) for an output folder.
# usage: up.sh <out> [vault|pass] ; ACCESS_TTL for the providers
H=$(cd $(dirname $0); pwd); O=$1; C=${CERTS:-/tmp/claude-0/w/certs}
mkdir -p $O; cp $C/silo-ca.crt $O/ca.crt; chmod 755 $O; chmod 644 $O/ca.crt
OUT=$O PORT=8443 TLS_CERT=$C/up-chain.crt TLS_KEY=$C/up-leaf.key python3 $H/providers.py 2>>$O/prov.err & echo $! > $O/pp
REFRESH_MARGIN=${REFRESH_MARGIN:-4} MODE=${2:-vault} CERT=$C/silo-chain.crt KEY=$C/silo-leaf.key UPSTREAM_CA=$C/up-ca.crt COMPUTERS=10.77.1.2=c1,10.77.2.2=c2 RECORD=$O/record.jsonl STATE=$O/vault-state.json python3 $H/vault.py 2>>$O/vault.err & echo $! > $O/vp
sleep 1
