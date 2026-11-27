#!/bin/bash
# A short "night" on the patched boundary: programs that once worked here (so their TLS clients
# have names), then the same programs without Silo's authority, then what the view makes of it.
# usage: night.sh <out> [seconds of Codex refusing, default 300]       (as root)
set -u
R=$(cd $(dirname $0); pwd); E=$(cd $R/../../2026-11-25-boundary-end-to-end; pwd); O=$1; S=${2:-300}
SILOREC=$(dirname $E)/2026-11-25-record-format/proto/zig-out/bin/silorec
$R/up.sh $O > /dev/null
python3 $R/cases.py $O claude-trusted codex-trusted git-trusted
CASE_LIMIT=$S python3 $R/cases.py $O codex-untrusted-long > $O/codex-long.jsonl &
CX=$!
sleep 5
python3 $R/cases.py $O claude-untrusted node-untrusted git-untrusted git-untrusted git-untrusted pinned-key curl-trusted python-trusted go-trusted
wait $CX; cat $O/codex-long.jsonl
sleep 2
$SILOREC verify $O/record/c1 c1
$SILOREC print $O/record/c1 > $O/c1.jsonl
python3 $R/refusals.py $O/c1.jsonl
python3 - $O/c1.jsonl <<'PY'
import json, sys
ls = [l for l in open(sys.argv[1])]
r = [l for l in ls if '"why":"certificate-refused"' in l or '"why":"closed-after-certificate"' in l or '"why":"no-request"' in l]
o = [l for l in ls if '"what":"opened"' in l]
print(f"record: {len(ls)} lines, {sum(map(len, ls))} bytes; refusal lines {len(r)}, {sum(map(len, r))//max(1,len(r))} bytes each; their opened lines {sum(map(len, o))//max(1,len(o))} bytes each")
PY
$E/run/down.sh $O
