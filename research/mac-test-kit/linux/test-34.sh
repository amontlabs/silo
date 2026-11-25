#!/bin/bash
# The Linux run of test 34: the boundary between stand-ins for Claude Code and Codex and fake
# providers, through the guest's own night script. The stand-ins send gzip bodies, streams, a
# stream cut half way, an error inside a stream, a 429 and a 529, a refused WebSocket and its
# fallback to plain requests, then a compressed WebSocket; and planted sign-ins. Checks the outcome,
# the facts, that no planted value reaches a file, that the search finds one that does, and that
# the analysis sees a split conversation and missing headers when they are there. Changes nothing
# outside a temporary folder; listens on 127.0.0.1 only.
set -u
[ "$(uname -s)" = Linux ] || { echo "Linux only: skipped"; exit 0; }
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
tmp="$(mktemp -d /tmp/kit34.XXXXXX)"
trap 'rm -rf "$tmp"' EXIT
export KIT_WORK_ROOT="$tmp/home" KIT_ID=34 KIT_WORK="$tmp/home/work/34" KIT_OUT="$tmp/results/results.jsonl"
export KIT_FAKE=1 KIT_YES=1
mkdir -p "$KIT_WORK" "$tmp/results"
: > "$KIT_OUT"
bash "$KIT_ROOT/tests/34-night-through-the-boundary.sh" < /dev/null > "$tmp/run.log" 2>&1
rc=$?
[ "$rc" = 0 ] || { echo "test 34 exited $rc"; tail -30 "$tmp/run.log"; exit 1; }
G="$KIT_WORK/guest-home"
B="$KIT_ROOT/py/34-boundary.py"
fails=0
bad() { echo "FAIL $*"; fails=$((fails + 1)); }

python3 - "$KIT_OUT" <<'PY' || fails=$((fails + 1))
import json, sys
facts, outcome = {}, None
for line in open(sys.argv[1]):
    e = json.loads(line)
    if e["kind"] == "fact": facts[e["name"]] = e["value"]
    if e["kind"] == "outcome": outcome = e
    if e["kind"] in ("error", "skip"): print("FAIL", e)
bad = []
nn = facts.get("not_normal_replies", "")
for k in ("anthropic status-429", "anthropic status-529", "anthropic cut", "anthropic stream-overloaded_error",
          "openai status-426", "openai status-429"):
    if k not in nn: bad.append("not_normal_replies lacks %r: %r" % (k, nn))
if not any(n.startswith("not_normal.") for n in facts): bad.append("no reply that is not normal is its own fact")
cc = facts.get("claude.conversations", "")
for k in ("calls with no id 0", "ids disagreeing in one call 0", "the program's ids not seen 0"):
    if k not in cc: bad.append("claude.conversations: %r" % cc)
cx = facts.get("codex.conversations", "")
if "calls with no id 0" not in cx or "ids disagreeing in one call 0" not in cx: bad.append("codex.conversations: %r" % cx)
tr = facts.get("codex.transport", "")
if "refused 1" not in tr or "compressed 0" in tr or "naming the previous reply 0" in tr or "warm-ups 0" in tr:
    bad.append("codex.transport: %r" % tr)
if "anthropic-ratelimit-unified-5h-utilization" not in facts.get("claude.plan_headers", ""): bad.append("claude.plan_headers %r" % facts.get("claude.plan_headers"))
if "x-codex-primary-used-percent" not in facts.get("codex.plan_headers", "") or "events on the socket: 0" in facts.get("codex.plan_headers", ""):
    bad.append("codex.plan_headers %r" % facts.get("codex.plan_headers"))
if "compressed requests 0" in facts.get("claude.model_call_timing", "compressed requests 0"): bad.append("gzip bodies not seen: %r" % facts.get("claude.model_call_timing"))
if '"found": []' not in facts.get("secrets_search", "") or facts.get("secrets_search", "").count('"found": []') != 2:
    bad.append("secrets_search %r" % facts.get("secrets_search"))
if facts.get("boundary_redactions") != "0": bad.append("boundary_redactions %r" % facts.get("boundary_redactions"))
if not outcome or outcome["key"] != "night-clean": bad.append("outcome %r" % outcome)
for b in bad: print("FAIL", b)
print("outcome:", outcome and outcome["key"], "|", outcome and outcome["text"][:700])
sys.exit(1 if bad else 0)
PY

# No planted value in any file outside the planted sign-ins themselves.
vals="$(python3 -c 'import json,sys
c = json.load(open(sys.argv[1])); a = json.load(open(sys.argv[2]))["tokens"]
v = [c["claudeAiOauth"]["accessToken"], c["claudeAiOauth"]["refreshToken"], c["silokitCookie"],
     a["access_token"], a["refresh_token"], a["id_token"], a["account_id"], a["access_token"].split(".")[2]]
print("\n".join(v))' "$G/.claude/.credentials.json" "$G/.codex/auth.json")"
[ "$(echo "$vals" | wc -l)" = 8 ] || bad "could not read the planted values"
hits="$(grep -rlF -f <(echo "$vals") "$tmp" | grep -v "^$G/.claude/\|^$G/.codex/" || true)"
[ -z "$hits" ] && echo "ok   no planted value in any file" || bad "planted values in: $hits"
[ "$(wc -l < "$KIT_WORK/boundary/calls.jsonl")" -gt 10 ] || bad "calls.jsonl has too few lines"

# The search finds a value that did reach a file, gzip or not, and names no value.
mkdir -p "$tmp/leak"
echo "x $(echo "$vals" | sed -n 2p) y" > "$tmp/leak/plain.txt"
echo "$(echo "$vals" | sed -n 5p)" | gzip > "$tmp/leak/packed.gz"
got="$(cat "$G/.claude/.credentials.json" <(echo ---silokit---) "$G/.codex/auth.json" | python3 -I "$B" scan "$tmp/leak")"
case "$got" in *plain.txt*packed.gz*|*packed.gz*plain.txt*) echo "ok   the search finds planted copies" ;; *) bad "scan: $got" ;; esac
echo "$got" | grep -qF -f <(echo "$vals") && bad "scan printed a value"

# The analysis sees a split conversation and missing headers.
python3 - "$KIT_WORK/boundary/calls.jsonl" "$tmp/split.jsonl" <<'PY'
import json, sys
out = open(sys.argv[2], "w")
done = False
for line in open(sys.argv[1]):
    c = json.loads(line)
    c["resp_headers"] = {k: v for k, v in c.get("resp_headers", {}).items() if not k.startswith("anthropic-ratelimit-")}
    if not done and c.get("provider") == "anthropic" and c.get("model_call") and c.get("tools"):
        c["ids"] = {}
        done = True
    out.write(json.dumps(c) + "\n")
PY
v="$(python3 -I "$B" analyze --calls "$tmp/split.jsonl" --conns "$KIT_WORK/boundary/conns.jsonl" --guest "$KIT_WORK/guest" \
  --night-hours 0.0015 --control-hours 0.0015 | sed -n 's/^verdict\t//p')"
python3 -c 'import json,sys
v = json.loads(sys.argv[1])
ok = any("claude: 1 model calls with tools carried no id" in s for s in v["split"]) and v["headers_missing"]
print(("ok   " if ok else "FAIL ") + "analysis of a split conversation and missing headers: %s / %s" % (v["split"], v["headers_missing"]))
sys.exit(0 if ok else 1)' "$v" || fails=$((fails + 1))

rm -rf "$KIT_ROOT/py/__pycache__" "$KIT_ROOT/linux/__pycache__"
[ "$fails" = 0 ] || { tail -40 "$tmp/run.log"; }
exit $((fails > 0))
