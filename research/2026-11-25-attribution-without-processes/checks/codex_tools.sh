#!/bin/bash
# Which tools Codex offers each model, read from its first request. Output: results/codex-tools.txt.
#   CODEX_BIN=... checks/codex_tools.sh <scratch folder>
set -u
HERE=$(cd "$(dirname "$0")"; pwd); W=$(realpath -m "$1"); mkdir -p "$W"; cd "$W"
python3 "$HERE/capture_request.py" & CAP=$!; sleep 1
for m in gpt-5.5 gpt-6.2-sol gpt-5.2-codex o3; do
  H=$W/h-$m; mkdir -p $H/.codex
  printf 'model = "%s"\nmodel_provider = "b"\n[model_providers.b]\nname = "b"\nbase_url = "http://127.0.0.1:18555/v1"\nwire_api = "responses"\nenv_key = "K"\n' $m > $H/.codex/config.toml
  HOME=$H CODEX_HOME=$H/.codex K=x timeout 40 "$CODEX_BIN" exec --json --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox hi > /dev/null 2>&1
done
kill $CAP
for f in body-*.json; do python3 -c "
import json,sys; f=sys.argv[1]; b=json.load(open(f))
extra=[t['name']+'('+t.get('type','')+')' for i in b.get('input',[]) if i.get('type')=='additional_tools' for ns in i['tools'] for t in ns['tools']]
print(f[5:-5], 'tools:', [t.get('name') or t.get('type') for t in b.get('tools',[])], 'apply_patch type:', [t.get('type') for t in b.get('tools',[]) if t.get('name')=='apply_patch'], '| additional_tools:', extra)" "$f"; done
