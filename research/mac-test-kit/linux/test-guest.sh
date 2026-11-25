#!/bin/bash
# The Linux runs of the guest tests (02 09 11 12 14 16 21 22): each test runs with KIT_FAKE=1 in a
# temporary folder, with the fake silovm of linux/fake-silovm-guest, and must end on the outcome
# its Linux analogue is built to reach. Called by linux/selftest.sh. Changes nothing outside the
# temporary folder (test 21 listens on 127.0.0.1:18443, test 09 on 127.0.0.1:18022 for a moment).
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
export KIT_ROOT
if [ "$(uname -s)" = Darwin ]; then echo "on a Mac: the Linux runs of the guest tests are skipped"; exit 0; fi
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export KIT_WORK_ROOT="$tmp/root" KIT_FAKE=1 KIT_YES=1 KIT_STOP_ROUNDS=4 KIT_IDLE_HOURS=0.003
fails=0

# run ID EXPECTED_OUTCOME [MEASURE=VALUE]
run() {
  local id="$1" want="$2" f out got
  f="$(ls "$KIT_ROOT"/tests/"$id"-*.sh)"
  out="$tmp/$id.jsonl"; : > "$out"
  mkdir -p "$tmp/work/$id"
  KIT_ID="$id" KIT_WORK="$tmp/work/$id" KIT_OUT="$out" bash "$f" > "$tmp/$id.log" 2>&1 < /dev/null
  got="$(python3 -c 'import json,sys
o=[json.loads(l) for l in open(sys.argv[1]) if l.strip()]
k=[e["key"] for e in o if e["kind"]=="outcome"]
err=[e["reason"] for e in o if e["kind"] in ("error","skip")]
print(k[-1] if k else "none:" + ";".join(err))' "$out")"
  if [ "$got" != "$want" ]; then
    echo "FAIL $id: outcome $got, expected $want"; tail -15 "$tmp/$id.log"; fails=$((fails + 1)); return
  fi
  if [ -n "${3:-}" ]; then
    if ! python3 -c 'import json,sys
n,v=sys.argv[2].split("=")
m=[e for e in map(json.loads,open(sys.argv[1])) if e["kind"]=="measure" and e["name"]==n]
sys.exit(0 if m and str(m[-1]["value"])==v else 1)' "$out" "$3"; then
      echo "FAIL $id: measure $3 not found"; fails=$((fails + 1)); return
    fi
  fi
  echo "ok   $id: $got ${3:-}"
}

run 02 lasts-in-files
run 09 sudo-not-allowed
run 11 open
run 12 place-leaks
run 14 keychain-locked-over-ssh
run 16 quiet
run 21 both-trust-keychain linux_cases_as_expected=1
run 22 inconclusive linux_parsers_ok=1
exit $((fails > 0))
