#!/bin/bash
# Run Codex once in computer $1 with CODEX_HOME $2/.codex: one model turn with a shell call, then an answer.
N=$1; HM=$2; CA=$3
mkdir -p /tmp/cw/site; chown cuser /tmp/cw/site; cd /tmp/cw/site
ip netns exec $N runuser -u cuser -- env -i PATH=/opt/codex:/usr/bin:/bin HOME=$HM CODEX_HOME=$HM/.codex SSL_CERT_FILE=$CA CODEX_CA_CERTIFICATE=$CA \
  timeout 60 codex exec --json --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check 'Do the task.' < /dev/null > $HM/out.jsonl 2> $HM/err.txt
echo "exit=$? result=$(grep -o '"text":"[^"]*"' $HM/out.jsonl | tail -1) $(grep -oiE '(unauthorized|401|refresh[^"]{0,60}|sign in again[^"]{0,40})' $HM/err.txt $HM/out.jsonl | sort -u | head -2 | tr '\n' ' ')"
