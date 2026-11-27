#!/bin/bash
# Runs inside the network namespace: only the loopback, no route, no name server.
# Each line of $RES: case, exit status, seconds, the files the turn left in its folder, the last
# line of Codex's output. Before each case its name goes into $OUT/case, so provider.py tags its
# log lines and keeps the model requests as bodies/<case>-<n>.json.
printf "127.0.0.1 localhost\n::1 localhost\n" > /tmp/hosts.closed && mount --bind /tmp/hosts.closed /etc/hosts
python3 "$H/relay.py" & RELAY=$!
for i in $(seq 50); do [ -s "$READY" ] && break; sleep 0.1; done
P=http://127.0.0.1:3128

case_() { # name, deny (comma-separated path pieces, or -), limit, command... (in a fresh HOME)
  local name=$1 deny=$2 limit=$3; shift 3
  [ -n "$CASES" ] && [[ " $CASES " != *" $name "* ]] && return
  echo "$name" > "$OUT/case"; if [ "$deny" = - ]; then rm -f "$OUT/case.deny"; else echo "$deny" > "$OUT/case.deny"; fi
  echo "{\"case\": \"$name\"}" >> "$OUT/proxy.jsonl"
  local W; W=$(mktemp -d); mkdir -p "$W/codex" "$W/work"
  local t0; t0=$(date +%s.%N)
  out=$(cd "$W/work" && HOME=$W CODEX_HOME=$W/codex HTTPS_PROXY=$P SSL_CERT_FILE=$STORE timeout "$limit" "$@" </dev/null 2>&1); st=$?
  local t1; t1=$(date +%s.%N)
  echo "$out" > "$OUT/$name.out"
  local files; files=$(cd "$W/work" && ls | tr '\n' ' ')
  local last; last=$(echo "$out" | grep -v '^\s*$' | grep -v '^{"type":"\(thread\|turn\)\.started' | tail -1 | cut -c1-200)
  printf '%s\t%s\t%.1f\t%s\t%s\n' "$name" "$st" "$(echo "$t1 - $t0" | bc)" "${files:-(none)}" "$last" >> "$RES"
  rm -rf "$W"
}

GPT="$H/chatgpt.sh"
X="exec --json --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox"
FN="-m gpt-5.5"; CODE="-m gpt-6.2-sol"; META="-c features.executed_tool_call_metadata=true"
KEY="env OPENAI_API_KEY=sk-proj-test"
ASK="Do_the_task."

case_ key-fn          - 120 $KEY $CODEX $X $FN $ASK
case_ key-code        - 120 $KEY $CODEX $X $CODE $ASK
case_ key-code-meta   - 120 $KEY $CODEX $X $CODE $META $ASK
case_ gpt-fn          - 120 $GPT $CODEX $X $FN $ASK
case_ gpt-code        - 120 $GPT $CODEX $X $CODE $ASK
case_ gpt-code-meta   - 120 $GPT $CODEX $X $CODE $META $ASK
case_ gpt-refresh     - 120 env EXPIRED=1 $GPT $CODEX $X $FN $ASK
# Which of the ChatGPT backend's other requests Codex needs: each answered 403 in turn.
for d in accounts/check codex/models ps/plugins ps/mcp analytics-events wham/settings otlp; do
  case_ "gpt-deny-${d//\//-}" "$d" 120 $GPT $CODEX $X $FN $ASK
done
case_ gpt-deny-all-but-model "accounts/check,codex/models,ps/,analytics-events,wham/settings,otlp" 120 $GPT $CODEX $X $FN $ASK
case_ gpt-deny-all-but-model-and-check "codex/models,ps/,analytics-events,wham/settings,otlp" 120 $GPT $CODEX $X $FN $ASK
case_ key-deny-all-but-model "plugins/,otlp" 120 $KEY $CODEX $X $FN $ASK
kill $RELAY
