#!/bin/bash
# LM Studio's headless server in the computer: which routes it serves, and what it calls outside.
L=${LMS_DIR:?the unpacked llmster folder}
$L/llmster > $HOME/llmster.log 2>&1 & D=$!
sleep 8
$L/.bundle/lms server start --port 1235 2>&1 | tail -2
sleep 2
for r in "GET /v1/models" "POST /v1/chat/completions" "POST /v1/responses" "POST /v1/messages" "POST /api/v1/chat" "GET /api/v0/models" "GET /api/v1/models"; do
  set -- $r; code=$(curl -s -o /tmp/lmsbody -w '%{http_code}' -X $1 -H 'content-type: application/json' -d '{"model":"none","messages":[{"role":"user","content":"hi"}],"input":"hi","max_tokens":5}' http://127.0.0.1:1235$2); echo "$r -> $code $(head -c 160 /tmp/lmsbody)"; done
$L/.bundle/lms ls 2>&1 | head -3
ls -la $HOME/.lmstudio 2>&1 | head; kill $D
