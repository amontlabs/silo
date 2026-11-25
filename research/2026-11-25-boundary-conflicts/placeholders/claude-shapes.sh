#!/bin/bash
# Which header Claude Code puts a credential in, by the variable it is in and by its shape. Runs
# Claude Code in a network namespace with only a loopback, against headers.py.
# usage: claude-shapes.sh      (as root; claude on PATH)
H=$(cd $(dirname $0); pwd); T=$(mktemp -d)
cat > $T/in.sh <<IN
ip link set lo up
python3 -I $H/headers.py 8080 $T/log & sleep 0.5
one() { : > $T/log; mkdir -p $T/home
  env -i PATH=\$PATH HOME=$T/home ANTHROPIC_BASE_URL=http://127.0.0.1:8080 IS_SANDBOX=1 DISABLE_AUTOUPDATER=1 CLAUDE_CODE_MAX_RETRIES=0 "\$1=\$2" timeout 40 claude -p hi --max-turns 1 > /dev/null 2>&1
  python3 -c "
import json,sys
seen=[json.loads(l) for l in open('$T/log')]
m=[r for r in seen if r['path']=='/v1/messages']
r=m[0] if m else (seen[0] if seen else {})
how='x-api-key' if r.get('x-api-key') else ('authorization: '+r['authorization'].split()[0] if r.get('authorization') else 'none')
print(f'{sys.argv[1]:<24} {sys.argv[2]:<40} {len(seen)} requests, messages via {how}, oauth beta: {\"oauth\" in (r.get(\"beta\") or \"\")}')
" "\$1" "\$2"; }
one ANTHROPIC_API_KEY placeholder-not-a-key
one ANTHROPIC_API_KEY sk-ant-api03-Zx9Qw2Lp0Vb7Nc4Mk1Hj6Gf3Ds8Aa5Ee
one ANTHROPIC_API_KEY sk-ant-oat01-Zx9Qw2Lp0Vb7Nc4Mk1Hj6Gf3Ds8Aa5Ee
one CLAUDE_CODE_OAUTH_TOKEN placeholder-not-a-key
one CLAUDE_CODE_OAUTH_TOKEN sk-ant-oat01-Zx9Qw2Lp0Vb7Nc4Mk1Hj6Gf3Ds8Aa5Ee
one ANTHROPIC_AUTH_TOKEN placeholder-not-a-key
kill %1
IN
unshare -n bash $T/in.sh; echo "claude $(claude --version 2>/dev/null)"; rm -rf "${T:?}"
