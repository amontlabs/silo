# One line per program: address of its computer | environment | command. Every key is a fake.
# The address is how the boundary tells computers apart: each program's /etc/hosts points the
# providers' hosts at its own computer's address.
NODE_TRUST="NODE_EXTRA_CA_CERTS=$TLS/silo-ca.crt"
CC="IS_SANDBOX=1 DISABLE_AUTOUPDATER=1 DISABLE_TELEMETRY=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 $NODE_TRUST"
CCARGS="--dangerously-skip-permissions --output-format stream-json --verbose --model ${MODEL:-claude-opus-5-5}"
P_cc="127.0.0.2|$CC ANTHROPIC_API_KEY=sk-ant-api03-main-fake|claude -p Do_the_task. $CCARGS"
P_cc2="127.0.0.4|$CC ANTHROPIC_API_KEY=sk-ant-api03-fork-fake|claude -p Do_the_task. $CCARGS"
P_ccplan="127.0.0.5|$CC CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-plan-fake|claude -p Do_the_task. $CCARGS"
P_ccbad="127.0.0.2|$CC ANTHROPIC_API_KEY=sk-ant-api03-from-dotenv|claude -p Do_the_task. $CCARGS"
P_ccnew="127.0.0.2|$CC ANTHROPIC_API_KEY=sk-ant-api03-main-fake|claude -p Do_the_task. --dangerously-skip-permissions --output-format stream-json --verbose --model claude-opus-6-0"
P_cx="127.0.0.3|SSL_CERT_FILE=$TLS/store-with-silo.crt OPENAI_API_KEY=sk-proj-codex-fake|codex exec --json -m gpt-6.2-sol --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check Do_the_task."
P_gem="127.0.0.6|$NODE_TRUST NODE_USE_SYSTEM_CA=1 GEMINI_API_KEY=AIzaFakeGeminiKey000 GEMINI_CLI_TRUST_WORKSPACE=true|gemini -m gemini-3.1-pro-preview -p Do_the_task. --yolo"
P_script="127.0.0.7|SSL_CERT_FILE=$TLS/store-with-silo.crt|python3 $R/client.py"
P_big="127.0.0.7|SSL_CERT_FILE=$TLS/store-with-silo.crt|python3 $R/bigcall.py"
