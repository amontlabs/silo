#!/bin/bash
# Runs inside the network namespace: only the loopback, no route, no name server.
# Each line of $RES: case, exit status, seconds, the last line that says what happened.
# Before each case a {"case": ...} line goes into both logs, so they can be cut by case.
printf "127.0.0.1 localhost\n::1 localhost\n" > /tmp/hosts.closed && mount --bind /tmp/hosts.closed /etc/hosts
python3 "$H/relay.py" & RELAY=$!
for i in $(seq 50); do [ -s "$READY" ] && break; sleep 0.1; done
P=http://127.0.0.1:3128

case_() { # name, limit, command... (runs in a fresh HOME)
  local name=$1 limit=$2; shift 2
  [ -n "$CASES" ] && [[ " $CASES " != *" $name "* ]] && return
  for f in "$OUT/proxy.jsonl" "$OUT/provider.jsonl"; do echo "{\"case\": \"$name\"}" >> "$f"; done
  local W; W=$(mktemp -d); mkdir -p "$W/codex"
  local t0; t0=$(date +%s.%N)
  out=$(cd "$W" && HOME=$W CODEX_HOME=$W/codex timeout "$limit" "$@" </dev/null 2>&1); st=$?
  local t1; t1=$(date +%s.%N)
  echo "$out" > "$OUT/$name.out"
  local last; last=$(echo "$out" | grep -v '^\s*$' | grep -v '^{"type":"\(thread\|turn\)\.started' | tail -1 | cut -c1-200)
  printf '%s\t%s\t%.1f\t%s\n' "$name" "$st" "$(echo "$t1 - $t0" | bc)" "$last" >> "$RES"
  rm -rf "$W"
}

CC="$CLAUDE -p Say_hello. --max-turns 1"
CCK="ANTHROPIC_API_KEY=sk-ant-api03-test"
case_ cc-none                 400 env $CCK NODE_EXTRA_CA_CERTS=$CA $CC
case_ cc-proxy-node-ca        120 env $CCK HTTPS_PROXY=$P NODE_EXTRA_CA_CERTS=$CA $CC
case_ cc-proxy-lower-node-ca  120 env $CCK https_proxy=$P NODE_EXTRA_CA_CERTS=$CA $CC
case_ cc-proxy-no-ca          120 env $CCK HTTPS_PROXY=$P $CC
case_ cc-proxy-ssl-cert-file  120 env $CCK HTTPS_PROXY=$P SSL_CERT_FILE=$STORE $CC
case_ cc-proxy-store          120 $H/store.sh env $CCK HTTPS_PROXY=$P $CC
case_ cc-proxy-store-use-sys  120 $H/store.sh env $CCK HTTPS_PROXY=$P NODE_USE_SYSTEM_CA=1 $CC
case_ cc-proxy-no-updater     120 env $CCK HTTPS_PROXY=$P NODE_EXTRA_CA_CERTS=$CA DISABLE_AUTOUPDATER=1 $CC
case_ cc-proxy-no-traffic     120 env $CCK HTTPS_PROXY=$P NODE_EXTRA_CA_CERTS=$CA CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 $CC

CX="$CODEX exec --json --skip-git-repo-check -m gpt-5.5 Say_hello."
CXK="OPENAI_API_KEY=sk-proj-test"
case_ cx-none                 400 env $CXK SSL_CERT_FILE=$STORE $CX
case_ cx-proxy                400 env $CXK HTTPS_PROXY=$P SSL_CERT_FILE=$STORE $CX
case_ cx-proxy-lower          400 env $CXK https_proxy=$P SSL_CERT_FILE=$STORE $CX
case_ cx-proxy-store          400 $H/store.sh env $CXK HTTPS_PROXY=$P $CX
case_ cx-proxy-no-ca          400 env $CXK HTTPS_PROXY=$P $CX


# Signed in with a plan instead of a key. Claude Code: a long-lived token, as `claude setup-token`
# makes. Codex: a ChatGPT sign-in, written as the auth.json Codex keeps (made-up tokens; the
# id_token is an unsigned JWT with the claims Codex reads).
case_ cc-plan-proxy           120 env CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-test HTTPS_PROXY=$P NODE_EXTRA_CA_CERTS=$CA $CC
chatgpt() { # writes $CODEX_HOME/auth.json, then runs the command
  python3 - "$CODEX_HOME/auth.json" <<'PY'
import base64, json, sys, time, datetime
b = lambda o: base64.urlsafe_b64encode(json.dumps(o).encode()).rstrip(b"=").decode()
claims = {"email": "night@example.org", "exp": int(time.time()) + 86400, "iat": int(time.time()),
          "https://api.openai.com/auth": {"chatgpt_plan_type": "pro", "chatgpt_account_id": "acct-test", "chatgpt_user_id": "user-test"}}
jwt = b({"alg": "none", "typ": "JWT"}) + "." + b(claims) + ".sig"
json.dump({"OPENAI_API_KEY": None, "tokens": {"id_token": jwt, "access_token": jwt, "refresh_token": "rt-test", "account_id": "acct-test"},
           "last_refresh": datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")}, open(sys.argv[1], "w"))
PY
  exec "$@"
}
case_ cx-chatgpt-proxy        400 env HTTPS_PROXY=$P SSL_CERT_FILE=$STORE bash -c "$(declare -f chatgpt); chatgpt \"\$@\"" - $CX

# A command the agent runs: Claude Code's Bash tool, asked by the stand-in (TOOL in run.sh), with
# the proxy settings in Claude Code's environment only.
case_ cc-tool-command         120 env $CCK HTTPS_PROXY=$P NODE_EXTRA_CA_CERTS=$CA SSL_CERT_FILE=$STORE $CLAUDE -p Say_hello. --max-turns 3 --allowedTools Bash
# The native build updating itself, which a night of main may do: through the proxy or not?
case_ cc-update               120 env HTTPS_PROXY=$P NODE_EXTRA_CA_CERTS=$CA $CLAUDE update
kill $RELAY
