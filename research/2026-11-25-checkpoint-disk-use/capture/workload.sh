#!/bin/bash
# An agent's days on a computer, in steps; a checkpoint after each.
set -u
E=/home/user/ckexp; M=$E/mnt; D=$E/caps
export HOME=$M/home/agent CARGO_HOME=$M/home/agent/.cargo RUSTUP_HOME=/root/.rustup
export PATH=/root/.cargo/bin:/opt/node22/bin:$PATH npm_config_cache=$HOME/.npm PIP_CACHE_DIR=$HOME/.cache/pip
export CARGO_TERM_QUIET=true npm_config_audit=false npm_config_fund=false npm_config_update_notifier=false
G="git -c user.email=agent@example.invalid -c user.name=agent"
snap() { echo "== $1 $(date +%T)"; python3 $E/tools/ckcap.py $D $E/live.img $M "$1"; }
step() { local t=$1; shift; echo "-- $t: $*"; ( set -x; eval "$@" ) >> $E/workload.log 2>&1 || echo "   (exit $?)"; snap $t; }
mkdir -p $HOME/p
step s00-base "mkdir -p $M/system && cp -a /usr/lib /usr/share /usr/bin $M/system/"
step s01-node-install "cd \$HOME/p && git clone -q --depth 200 https://github.com/fastify/fastify && cd fastify && npm install"
step s02-ts-install "cd \$HOME/p && git clone -q --depth 200 https://github.com/sindresorhus/ky && cd ky && npm install && npx tsc -p . || true"
step s03-rust-build "cd \$HOME/p && git clone -q --depth 200 https://github.com/BurntSushi/ripgrep && cd ripgrep && cargo build"
step s04-py-venv "cd \$HOME/p && python3 -m venv .venv && .venv/bin/pip install -q numpy pandas requests"
for c in 1 2 3; do
  step c$c-a-rust-edit "cd \$HOME/p/ripgrep && sed -i '1a // agent edit $c' crates/core/main.rs crates/searcher/src/lib.rs && cargo build && $G commit -qam e$c"
  step c$c-b-node-edit "cd \$HOME/p/fastify && sed -i '1a // agent edit $c' lib/route.js lib/reply.js && npx eslint lib >/dev/null; $G commit -qam e$c"
  step c$c-c-rust-test "cd \$HOME/p/ripgrep && cargo test --no-run"
  step c$c-d-node-reinstall "cd \$HOME/p/fastify && rm -rf node_modules && npm install"
  step c$c-e-ts-build "cd \$HOME/p/ky && sed -i '1a // agent edit $c' source/index.ts && rm -rf distribution && npx tsc -p . ; $G commit -qam e$c"
  if [ $c = 2 ]; then step c$c-f-rust-clean "cd \$HOME/p/ripgrep && cargo clean && cargo build"; fi
  if [ $c = 1 ]; then step c$c-f-rust-release "cd \$HOME/p/ripgrep && cargo build --release"; fi
  if [ $c = 3 ]; then step c$c-f-rust-release "cd \$HOME/p/ripgrep && cargo build --release"; fi
  step c$c-g-py-reinstall "cd \$HOME/p && .venv/bin/pip install -q --force-reinstall pandas requests"
  step c$c-h-git-gc "cd \$HOME/p/fastify && $G gc -q && cd ../ripgrep && $G gc -q"
done
echo DONE
