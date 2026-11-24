#!/bin/bash
# For each case: a fresh lenient host, a fresh boundary (the #347 spike), one program connection.
# usage: BOUNDARY=<path to the built spike> [STRICT=1] ./run.sh
# The spike is the one of PR #347, kept outside the repository; strict.patch is the red-team change.
R=$(cd $(dirname $0); pwd); B=${BOUNDARY:?set BOUNDARY}; T=$R/tls; O=$R/out; rm -rf $O; mkdir -p $O
(cd $R && python3 mkcases.py > /dev/null)
if [ ! -f $T/ca.crt ]; then mkdir -p $T; (cd $T
  for n in ca upca; do openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $n.key -out $n.crt -subj "/CN=$n" -days 30 2>/dev/null; done
  printf "subjectAltName=DNS:api.anthropic.com,DNS:api.openai.com,DNS:chatgpt.com\n" > san.ext
  for p in leaf:ca up:upca; do l=${p%%:*}; c=${p##*:}; openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $l.key -out $l.csr -subj "/CN=api.anthropic.com" 2>/dev/null
    openssl x509 -req -in $l.csr -CA $c.crt -CAkey $c.key -CAcreateserial -out $l.crt -days 30 -extfile san.ext 2>/dev/null; cat $l.crt $c.crt > $l-chain.crt; done); fi
for f in $R/cases/*.bin; do n=$(basename $f .bin)
  python3 $R/host.py 8443 $T/up-chain.crt $T/up.key $O/$n.host.jsonl & HP=$!
  LISTEN_PORT=9443 RECORD=$O/$n.record.jsonl CERT=$T/leaf-chain.crt KEY=$T/leaf.key UPSTREAM_CA=$T/upca.crt HOLD=0 $B 2>$O/$n.err & BP=$!
  sleep 0.4
  timeout 5 python3 $R/client.py 9443 $T/ca.crt api.anthropic.com $f > $O/$n.client.txt
  sleep 0.2; kill $HP $BP 2>/dev/null; wait $HP $BP 2>/dev/null
  echo "=== $n"
  echo "-- the record (boundary) saw:"; grep '"request"' $O/$n.record.jsonl | python3 -c "import sys,json;[print('  ',{k:d[k] for k in ('line','bytes','body_bytes_decoded','results')}) for d in map(json.loads,sys.stdin)]"
  echo "-- the host parsed:"; python3 -c "import sys,json;[print('  ',d) for d in map(json.loads,open('$O/$n.host.jsonl'))]" 2>/dev/null
  echo "-- the program got:"; grep -o '{"host_parsed_request[^}]*}' $O/$n.client.txt | sed 's/^/   /'
done
