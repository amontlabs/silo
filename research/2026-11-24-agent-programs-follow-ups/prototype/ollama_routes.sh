#!/bin/bash
# Ollama's own server: which routes it serves (no model is loaded, so a route that exists answers 404 "model not found" or 400, a missing one 404 "page not found").
ollama serve > $HOME/serve.log 2>&1 & S=$!; sleep 2
for r in /api/chat /api/generate /v1/chat/completions /v1/responses /v1/messages /v1/nothing; do
  echo "$r -> $(curl -s -w ' %{http_code}' -X POST -H 'content-type: application/json' -d '{"model":"none","messages":[{"role":"user","content":"hi"}],"input":"hi","max_tokens":5}' http://$OLLAMA_HOST$r | head -c 200)"; done
kill $S
