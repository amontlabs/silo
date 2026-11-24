# One line per program: name|env|command. Keys are fakes. HOME is fresh per run.
claude|IS_SANDBOX=1 ANTHROPIC_API_KEY=sk-ant-api03-fake DISABLE_AUTOUPDATER=1|claude -p "Say hi" --dangerously-skip-permissions
codex|OPENAI_API_KEY=sk-fake|codex exec --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox "Say hi"
gemini|GEMINI_API_KEY=AIzaFake GEMINI_CLI_TRUST_WORKSPACE=true NODE_USE_SYSTEM_CA=1|gemini -p "Say hi" --yolo
opencode-anthropic|ANTHROPIC_API_KEY=sk-ant-api03-fake|opencode run -m anthropic/claude-sonnet-5-5 "Say hi"
opencode-openai|OPENAI_API_KEY=sk-fake|opencode run -m openai/gpt-4o "Say hi"
cline-anthropic||cline -P anthropic -k sk-ant-api03-fake -m claude-sonnet-4-5 -t 40 "Say hi"
cline-openai||cline -P openai-native -k sk-fake -m gpt-5 -t 40 "Say hi"
aider-openai|SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt OPENAI_API_KEY=sk-fake|aider --model gpt-4o --message "Say hi" --yes-always --no-git --no-check-update --no-show-release-notes --no-analytics
aider-anthropic|SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt ANTHROPIC_API_KEY=sk-ant-api03-fake|aider --model anthropic/claude-sonnet-5-5 --message "Say hi" --yes-always --no-git --no-check-update --no-show-release-notes --no-analytics
goose-openai|GOOSE_PROVIDER=openai GOOSE_MODEL=gpt-4o OPENAI_API_KEY=sk-fake GOOSE_DISABLE_KEYRING=1|goose run -t "Say hi"
goose-anthropic|GOOSE_PROVIDER=anthropic GOOSE_MODEL=claude-sonnet-4-5 ANTHROPIC_API_KEY=sk-ant-api03-fake GOOSE_DISABLE_KEYRING=1|goose run -t "Say hi"
qwen|NODE_USE_SYSTEM_CA=1 OPENAI_API_KEY=sk-fake OPENAI_BASE_URL=https://dashscope-intl.aliyuncs.com/compatible-mode/v1 OPENAI_MODEL=qwen3-coder-plus|qwen -p "Say hi" --yolo
copilot|COPILOT_GITHUB_TOKEN=github_pat_fake|copilot -p "Say hi" --allow-all
amp|AMP_API_KEY=sgamp_fake|amp -x "Say hi"
cursor|CURSOR_API_KEY=key_fake|cursor-agent -p "Say hi" --force
