# Helpers for the fork runs. A computer is a folder C/<name> holding home/, work/ and machine-id.
# Programs run in a private mount namespace where that computer's folders are at /vm/home and
# /vm/work, so a copy of the folder (a fork) sees the same paths, as a fork of a disk does.
# Each computer's programs reach the stand-in on that computer's own port: that is how the
# boundary knows which computer sent a request, without asking the program.
R=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
C=${C:-/tmp/forkruns/C}
CODEX=${CODEX:-codex}
mkdir -p /vm/home /vm/work

new_computer() { # name
  mkdir -p "$C/$1/home" "$C/$1/work"
  python3 -c 'import uuid;print(uuid.uuid4().hex)' > "$C/$1/machine-id"
  mkdir -p "$C/$1/home/.codex"
  printf 'model = "gpt-6.2-sol"\nmodel_provider = "standin"\n[model_providers.standin]\nname = "standin"\nbase_url = "http://127.0.0.1:1/v1"\nenv_key = "OPENAI_API_KEY"\nwire_api = "responses"\n' > "$C/$1/home/.codex/config.toml"
  (cd "$C/$1/work" && git init -q && git -c user.email=a@b -c user.name=a commit -q --allow-empty -m start)
}

# A disk copy. A program's background work (Codex fetching plugins) may delete files while it runs.
copy_disk() { cp -a "$1" "$2" 2>/dev/null || true; }

# fork_computer parent child [new-identity]: copy the disk; with new-identity, a new machine-id
# as `fork-identity` asks.
fork_computer() {
  copy_disk "$C/$1" "$C/$2"
  if [ "${3:-}" = new-identity ]; then python3 -c 'import uuid;print(uuid.uuid4().hex)' > "$C/$2/machine-id"; fi
}

port_of() { python3 -c "import sys;d=dict(p.split('=') for p in sys.argv[1].split(','));print(d[sys.argv[2]])" "$PORTS" "$1"; }

on() { # computer cmd...: run a command on that computer
  local name=$1; shift; local port; port=$(port_of "$name")
  unshare -m --propagation private bash -c '
    mount --bind "$1/home" /vm/home && mount --bind "$1/work" /vm/work && mount --bind "$1/machine-id" /etc/machine-id || exit 9
    shift; cd /vm/work; exec "$@"' _ "$C/$name" env -i PATH="$PATH" HOME=/vm/home TERM=dumb \
      ANTHROPIC_BASE_URL="http://127.0.0.1:$port" ANTHROPIC_API_KEY=sk-ant-api03-standin-key \
      OPENAI_API_KEY=sk-proj-standin-key PORT="$port" \
      IS_SANDBOX=1 DISABLE_AUTOUPDATER=1 DISABLE_TELEMETRY=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
      "$@"
}

CCARGS="--dangerously-skip-permissions --output-format json --model claude-opus-5-5"
cc() { local name=$1; shift; on "$name" claude -p "$@" $CCARGS < /dev/null; }
cx() { local name=$1; shift; local port; port=$(port_of "$name")
  on "$name" "$CODEX" exec --json --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox \
     -c "model_providers.standin.base_url=\"http://127.0.0.1:$port/v1\"" "$@" < /dev/null; }
