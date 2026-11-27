gemini|GEMINI_API_KEY=AIzaFake NODE_USE_SYSTEM_CA=1 GEMINI_CLI_TRUST_WORKSPACE=true|gemini -p "Do the task." --yolo
qwen|OPENAI_API_KEY=sk-fake OPENAI_BASE_URL=https://dashscope-intl.aliyuncs.com/compatible-mode/v1 OPENAI_MODEL=qwen3-coder-plus NODE_USE_SYSTEM_CA=1|qwen -p "Do the task." --yolo
cline|NODE_USE_SYSTEM_CA=1|cline -P anthropic -k sk-ant-api03-fake -m claude-sonnet-5-5 -t 80 "Do the task."
opencode|ANTHROPIC_API_KEY=sk-ant-api03-fake|opencode run -m anthropic/claude-sonnet-5-5 "Do the task."
goose|GOOSE_PROVIDER=anthropic GOOSE_MODEL=claude-sonnet-5-5 ANTHROPIC_API_KEY=sk-ant-api03-fake GOOSE_DISABLE_KEYRING=1 GOOSE_MODE=auto|goose run -t "Do the task."
aider|OPENAI_API_KEY=sk-fake SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt|aider --model gpt-4o --message "Do the task." --yes-always --no-git --no-check-update --no-show-release-notes --no-analytics existing.txt
