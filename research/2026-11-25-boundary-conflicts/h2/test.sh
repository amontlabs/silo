#!/bin/bash
# A gRPC client in each computer calls a Google API through the boundary with its placeholder: one
# unary call and one server stream with trailers. Passes when every call reached the stand-in with
# the real token and returned. gRPC needs HTTP/2: before the boundary speaks it, every call fails.
# usage: test.sh <name>     (as root, after #395's run/tls.sh and builds; BIN3: gvproxy with the
#                           metadata folder's patch; H2BIN: grpc-standin and grpc-client, built from grpc/)
set -u
H=$(cd $(dirname $0); pwd); R=$H/../../2026-11-25-boundary-end-to-end/run
BIN=${BIN3:-/root/e2e/bin3}; H2BIN=${H2BIN:-/root/e2e/h2}; O=/tmp/e2e/h2-$1
BIN=$BIN GVPROXY_ARGS="-no-gateway-api -no-host-loopback -outbound-deny private,host -outbound-handoff $O/c2.handoff.sock -dns-upstream 127.0.0.1:5302" $R/up.sh $O > /dev/null
. $O/env
REAL=$(python3 -c "import json;c=json.load(open('$O/config.json'));print(next(s['value'] for s in c['boundary']['computers'][0]['secrets'] if s['name']=='gcp'))")
$H2BIN/grpc-standin 127.0.0.1:9444 $TLS/up-chain.crt $TLS/up-leaf.key "$REAL" $O/grpc.log 2> $O/grpc.err & S=$!
sleep 0.3; fail=0
for c in c1 c2; do
  PH=$(python3 -c "import json;c=json.load(open('$O/config.json'));print(next(s['placeholder'] for k in c['boundary']['computers'] if k['id']=='$c' for s in k['secrets'] if s['name']=='gcp'))")
  before=$(wc -l < $O/grpc.log 2>/dev/null || echo 0)
  out=$($R/in.sh $O $c $H2BIN/grpc-client pubsub.googleapis.com "$PH" 2>&1); rc=$?
  seen=$(tail -n +$((before+1)) $O/grpc.log 2>/dev/null | python3 -c "import sys,json;print([json.loads(l)['call']+':'+json.loads(l)['cred'] for l in sys.stdin])")
  ok=$([ $rc = 0 ] && [ "$seen" = "['Check:real:gcp', 'Watch:real:gcp']" ] && echo PASS || echo FAIL); [ $ok = PASS ] || fail=1
  echo "$ok  $c gRPC with its placeholder: $(echo $out | tr '\n' ' ' | cut -c1-200) | host saw $seen"
done
# Refusals, on HTTP/2 as on HTTP/1.1: a placeholder bound for a host it may not go to, and a
# placeholder Silo gave the other computer. A gRPC client shows Silo's sentence.
ph() { python3 -c "import json;c=json.load(open('$O/config.json'));print(next(s['placeholder'] for k in c['boundary']['computers'] if k['id']=='$1' for s in k['secrets'] if s['name']=='$2'))"; }
for c in c1 c2; do
  other=$([ $c = c1 ] && echo c2 || echo c1)
  for what in "anthropic:$c:a placeholder bound for another host" "gcp:$other:the other computer's placeholder"; do
    IFS=: read sec owner label <<< "$what"
    before=$(wc -l < $O/grpc.log 2>/dev/null || echo 0)
    out=$($R/in.sh $O $c $H2BIN/grpc-client pubsub.googleapis.com "$(ph $owner $sec)" 2>&1); rc=$?
    seen=$(tail -n +$((before+1)) $O/grpc.log 2>/dev/null | wc -l)
    ok=$([ $rc != 0 ] && [ $seen = 0 ] && echo "$out" | grep -q 'PermissionDenied desc = Silo:' && echo PASS || echo FAIL); [ $ok = PASS ] || fail=1
    echo "$ok  $c gRPC, $label: refused, host saw $seen calls: $(echo "$out" | head -1 | cut -c1-150)"
  done
done
sleep 1
SILOREC=$H/../../2026-11-25-record-format/proto/zig-out/bin/silorec
for c in c1 c2; do echo "record $c: $($SILOREC verify $O/record/$c $c 2>&1 | tail -1)"; done
echo "boundary: $(grep -c '"kind":"request","protocol":"h2"' $O/trace.jsonl) HTTP/2 requests sent, $(grep '"kind":"refused","protocol":"h2"' $O/trace.jsonl | wc -l) refused"
kill $S; $R/down.sh $O; exit $fail
