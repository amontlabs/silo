#!/bin/bash
# Runs inside the network namespace: only the loopback, no route, no name server.
# Each line of $RES: case, with or without the proxy settings, exit status, seconds, last line of output.
H=$(dirname "$0")
# A computer's own /etc/hosts holds nothing of the providers'.
printf "127.0.0.1 localhost\n::1 localhost\n" > /tmp/hosts.closed && mount --bind /tmp/hosts.closed /etc/hosts
python3 "$H/relay.py" & RELAY=$!
for i in $(seq 50); do [ -s "$READY" ] && break; sleep 0.1; done

export NODE_EXTRA_CA_CERTS=$CA SSL_CERT_FILE=$CA CURL_CA_BUNDLE=$CA \
  REQUESTS_CA_BUNDLE=$CA GIT_SSL_CAINFO=$CA
P=http://127.0.0.1:3128
WITH="HTTP_PROXY=$P HTTPS_PROXY=$P http_proxy=$P https_proxy=$P"
WORK=$(mktemp -d); cd "$WORK"; export HOME=$WORK npm_config_cache=$WORK/npm

run() { # name, env-label, command...
  local name=$1 label=$2; shift 2
  local t0=$(date +%s.%N)
  out=$(timeout ${LIMIT:-120} "$@" </dev/null 2>&1); st=$?
  local t1=$(date +%s.%N)
  printf '%s\t%s\t%s\t%.1f\t%s\n' "$name" "$label" "$st" "$(echo "$t1 - $t0" | bc)" \
    "$( (echo "$out" | grep -i -m1 -E 'error|fail|refus|unreach|resolve' || echo "$out" | grep -v '^\s*$' | tail -1) | cut -c1-160)" >> "$RES"
}

both() { # name, command... : once without the proxy settings, once with them
  local name=$1; shift
  run "$name" without "$@"
  run "$name" with env $WITH "$@"
}

run "name lookup (getent)" - getent hosts api.anthropic.com
run "raw DNS to 8.8.8.8" - python3 -c "import socket;s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.sendto(b'x',('8.8.8.8',53))"
run "TCP to 1.1.1.1:443, proxy skipped" - curl -sS --noproxy '*' https://1.1.1.1/
both "curl https://api.anthropic.com/v1/models" curl -sS -o /dev/null -w '%{http_code}' -H 'x-api-key: sk-ant-test' https://api.anthropic.com/v1/models
both "curl, host not allowed" curl -sS -o /dev/null -w '%{http_code}' https://example.org/
both "git ls-remote over HTTPS" git ls-remote https://github.com/octocat/Hello-World
both "npm view" npm view left-pad version --fetch-retries=0
both "pip download" pip download --no-deps --retries 0 --timeout 5 -d dl six
both "Python urllib" python3 -c "import urllib.request;print(urllib.request.urlopen('https://api.anthropic.com/v1/models').status)"
both "Node 22 fetch" node -e "fetch('https://api.anthropic.com/v1/models').then(r=>console.log(r.status),e=>{console.log(String(e.cause||e));process.exit(1)})"
run "Node 22 fetch" "with + NODE_USE_ENV_PROXY=1" env $WITH NODE_USE_ENV_PROXY=1 node -e "fetch('https://api.anthropic.com/v1/models').then(r=>console.log(r.status),e=>{console.log(String(e.cause||e));process.exit(1)})"
LIMIT=600 both "Claude Code -p" env ANTHROPIC_API_KEY=sk-ant-api03-test DISABLE_AUTOUPDATER=1 claude -p "Say hello." --max-turns 1
kill $RELAY
