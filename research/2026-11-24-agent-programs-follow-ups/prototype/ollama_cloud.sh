#!/bin/bash
# Ollama's own server in the computer, asked for a cloud model: what leaves for ollama.com.
ollama serve > $HOME/serve.log 2>&1 & S=$!
sleep 2
ollama run gpt-oss:120b-cloud "Say hi." ; echo "run exit=$?"
ls -la $HOME/.ollama; kill $S
