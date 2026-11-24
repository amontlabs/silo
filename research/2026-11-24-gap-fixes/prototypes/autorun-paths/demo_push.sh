#!/bin/bash
# End to end: a computer pushes to a remote through the stand-in boundary.
# usage: demo_push.sh <dir>
set -u
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1  # the machine's own Git settings stay out of the demo
P=$(cd $(dirname $0); pwd); D=$(mkdir -p "$1"; cd "$1"; pwd); rm -rf "$D"/*
bash $P/make_worm_repo.sh "$D/computer" > "$D/ids"; . <(sed 's/ /=/' "$D/ids")
git init -q --bare "$D/up/demo.git"; git -C "$D/computer" push -q "$D/up/demo.git" $base:refs/heads/main
PORT=$((9100 + RANDOM % 500))
python3 -I $P/push_boundary.py "$D/up/demo.git" "$D/boundary" $PORT & BP=$!
sleep 1
cd "$D/computer"; git remote add origin http://127.0.0.1:$PORT/demo.git
step() { echo; echo "=== $1"; git push origin $2 2>&1 | sed 's/^/    /'; echo "    upstream main is now: $(git -C $D/up/demo.git rev-parse --short main)"; }
step "push the benign commit" $benign:refs/heads/main
step "push the worm commit" $worm:refs/heads/main
step "push the worm on a new branch" $worm:refs/heads/fix-login
step "push benign + worm + payload in one push, as a second branch from base" $payload:refs/heads/two
# A hook added in one commit and removed in the next: the tip runs nothing, a checkout of the
# middle commit (git bisect, git checkout <id>) runs it.
git checkout -q -b hide $benign; cp .claude/settings.json "$D/keep.json"
git show $worm:.claude/settings.json > .claude/settings.json; git commit -qam "add hook"
cp "$D/keep.json" .claude/settings.json; rm "$D/keep.json"; git commit -qam "remove hook"
step "push a hook added then removed" hide:refs/heads/hide

kill $BP
echo; echo "=== the record, one line per push (decision, ref updates, what was found)"
python3 -c '
import json,sys
for l in open(sys.argv[1]):
    e=json.loads(l)
    print(e["decision"], [ (u["ref"], u["old"][:7], u["new"][:7]) for u in e.get("updates",[])], "pack", e.get("pack_bytes"), "bytes")
    for r in e.get("findings",[]):
        for f in r["findings"]:
            print("   ", "HOLD" if f["hold"] else "list", f["path"], "|", "; ".join(f["details"]))
' "$D/boundary/record.jsonl"
echo; echo "=== the first pkt-line of the last push, as the boundary read it"
tail -1 "$D/boundary/record.jsonl" | python3 -c 'import json,sys; print(repr(json.load(sys.stdin)["head"][:170]))'
