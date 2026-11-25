# The night runs in a Linux container on the Mac: the morning review's probe is strace, which
# macOS does not have. Build context: the repository's research/ folder.
FROM node:26-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends python3 strace git procps curl ca-certificates unzip \
 && rm -rf /var/lib/apt/lists/*
# The agent programs, pinned: a night is compared with the next only on the same versions.
RUN npm install -g @anthropic-ai/claude-code@2.1.332 @openai/codex@0.172.0 && npm cache clean --force
COPY 2026-11-24-morning-review/proto /silo/proto
COPY 2026-11-25-real-night/kit /silo/kit
ENV SILO_PRICES=/silo/kit/prices.json SILO_CLAIMS=claims@0.3 CODEX_BIN=codex PYTHONDONTWRITEBYTECODE=1
ENTRYPOINT ["/silo/kit/in-container.sh"]
