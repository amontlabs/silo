#!/usr/bin/env bash
# Silo v1 phase 2 VM check, Linux half: diffs of three live agent tasks and four keeps, against the Dev engine. Run:
#   bash docs/v1/checks/phase2_linux_check.sh 2>&1 | tee docs/v1/evidence/phase-2/linux-check.txt
# Only `silo` (incl. `silo exec` in e2e-p2-* computers, to build main's fixture and to observe read-only),
# `claude -p` for the live tasks, and read-only host commands. Env: KEEP=1 skips cleanup, REUSE=1 reuses an
# existing e2e-p2-main whose stack is already set up (development of this script only).
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
BUNDLE="$REPO/app/SiloUI/target/debug/bundle/macos/Silo Dev.app"
EVID="$REPO/docs/v1/evidence/phase-2/linux-check-files"
MAIN=e2e-p2-main; T1=e2e-p2-t1; T2=e2e-p2-t2; T3=e2e-p2-t3; TD=e2e-p2-data
MODEL=${CLAUDE_MODEL:-sonnet}
PROJ=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p2-proj.XXXXXX")
mkdir -p "$EVID"
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo bash -c "$*"; }
# put COMPUTER GUESTPATH [MODE]: write stdin to a file in the guest
put() { local b; b=$(base64 | tr -d '\n'); x "$1" "echo $b | base64 -d > $2 && chmod ${3:-644} $2"; }
now() { date +%s; }
ALL="$TD $T3 $T2 $T1 $MAIN"
cleanup() {
  if [ "${KEEP:-0}" = 1 ]; then say "KEEP=1: leaving computers and $PROJ"; return; fi
  say "cleanup"
  for c in $ALL; do "$SILO" stop "$c" >/dev/null 2>&1; done
  for c in $ALL; do for i in $(seq 30); do "$SILO" drop "$c" --force >/dev/null 2>&1 && break; "$SILO" status "$c" >/dev/null 2>&1 || break; sleep 3; done; done
  rm -rf "$PROJ"; say "silo list after cleanup:"; "$SILO" list
}
trap cleanup EXIT

echo "== header"
echo "engine/CLI commit: $(git -C "$REPO" rev-parse HEAD); tracked changes under app/ (git status --porcelain app): [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "bundled msb: $("$BUNDLE/Contents/MacOS/msb" --version 2>&1 | head -1); silo: $("$SILO" --version); host $(sw_vers -productVersion) $(uname -m); date $(date -u +%FT%TZ)"
echo "claude: $(claude --version 2>&1 | head -1); model requested: --model $MODEL (actual model(s) from the usage record are printed after each task)"

mainstate() { # a fingerprint of main's repo, database, packages and settings (no pids)
  x $MAIN 'cd /home/silo/app && G="git -c safe.directory=/home/silo/app" && echo head=$($G rev-parse HEAD) && echo status=[$($G status --porcelain | tr "\n" " ")] && echo files=$($G ls-files -z | xargs -0 sha256sum | sha256sum | cut -c1-16) && echo env=$(sha256sum .env | cut -c1-16) && echo schema=$(sudo -u postgres pg_dump -s appdb | grep -av "restrict" | sha256sum | cut -c1-16) && echo data=$(sudo -u postgres pg_dump -a appdb | grep -av "restrict" | sha256sum | cut -c1-16) && echo dpkg=$(dpkg-query -W | sha256sum | cut -c1-16) && echo bashrc=$(sha256sum /home/silo/.bashrc | cut -c1-16) && echo cfg=$(ls -A /home/silo/.config 2>/dev/null | tr "\n" ",")'
}
webpid() { x $1 'pidof -s web || echo 0'; }
stack_up() { # web answers and postgres answers
  x $1 'pg_isready -q -h 127.0.0.1 && curl -s -m 5 http://127.0.0.1:3000/items | head -c 200 | grep -q "^\["'
}

if [ "${REUSE:-0}" != 1 ]; then
echo "== main $MAIN: create, install the stack, boot persistence"
t=$(now); "$SILO" create $MAIN --linux --cpus 2 --memory 3 --disk 10 >/dev/null 2>&1; check "create $MAIN" $? "$(( $(now)-t )) s"
"$SILO" start $MAIN >/dev/null 2>&1; check "start $MAIN" $? ""
echo "guest OS: $(x $MAIN '. /etc/os-release; echo $PRETTY_NAME kernel $(uname -r)')"
x $MAIN 'rm -f /root/apt.rc; export DEBIAN_FRONTEND=noninteractive; nohup bash -c "apt-get update -qq; apt-get install -y -qq postgresql nodejs npm git curl; echo \$? > /root/apt.rc" >/root/apt.log 2>&1 </dev/null &'
say "installing postgresql, nodejs, npm in the guest (max 600 s)"
for i in $(seq 120); do [ "$(x $MAIN 'cat /root/apt.rc 2>/dev/null')" != "" ] && break; sleep 5; done
echo "apt rc=$(x $MAIN 'cat /root/apt.rc'); node $(x $MAIN 'node -v'); postgres $(x $MAIN 'ls /usr/lib/postgresql')"

put $MAIN /usr/local/bin/silo-p2-setup.sh 755 <<'SETUP'
#!/bin/bash
set -e
service postgresql start || true
for i in $(seq 60); do pg_isready -q -h 127.0.0.1 && break; sleep 1; done
su postgres -c "psql -c \"create role app login password 'secret'\"" || true
su postgres -c "createdb -O app appdb" || true
ln -sf /usr/bin/node /usr/local/bin/web
mkdir -p /home/silo/app/migrations /home/silo/app/test && cd /home/silo/app
cat > package.json <<'EOF'
{
  "name": "p2app",
  "version": "1.0.0",
  "scripts": {
    "migrate": "node migrate.js",
    "start": "node server.js",
    "test": "node --test test/"
  },
  "dependencies": { "pg": "^8.11.0" }
}
EOF
cat > env.js <<'EOF'
const fs = require('fs'), path = require('path');
for (const l of fs.readFileSync(path.join(__dirname, '.env'), 'utf8').split('\n')) {
  const m = l.match(/^([A-Z_]+)=(.*)$/);
  if (m && !(m[1] in process.env)) process.env[m[1]] = m[2];
}
EOF
cat > db.js <<'EOF'
require('./env');
const { Pool } = require('pg');
module.exports = new Pool({ connectionString: process.env.DATABASE_URL });
EOF
cat > server.js <<'EOF'
const http = require('http');
require('./env');
const pool = require('./db');
function send(res, code, obj) { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(obj)); }
http.createServer(async (req, res) => {
  try {
    if (req.method === 'GET' && req.url === '/items') {
      const r = await pool.query('select id, title from items order by id');
      return send(res, 200, r.rows);
    }
    if (req.method === 'POST' && req.url === '/items') {
      let b = ''; for await (const c of req) b += c;
      const { title } = JSON.parse(b);
      const r = await pool.query('insert into items(title) values($1) returning id, title', [title]);
      return send(res, 201, r.rows[0]);
    }
    const del = req.method === 'DELETE' && req.url.match(/^\/items\/(\d+)$/);
    if (del) { await pool.query('delete from items where id = $1', [del[1]]); return send(res, 200, { ok: true }); }
    if (req.url === '/health') return send(res, 200, { ok: true, pid: process.pid });
    send(res, 404, { error: 'not found' });
  } catch (e) { send(res, 500, { error: String(e) }); }
}).listen(process.env.PORT || 3000);
EOF
cat > migrate.js <<'EOF'
const fs = require('fs');
const pool = require('./db');
(async () => {
  await pool.query('create table if not exists schema_migrations(name text primary key)');
  const done = new Set((await pool.query('select name from schema_migrations')).rows.map(r => r.name));
  for (const f of fs.readdirSync(__dirname + '/migrations').sort()) {
    if (done.has(f)) continue;
    await pool.query(fs.readFileSync(__dirname + '/migrations/' + f, 'utf8'));
    await pool.query('insert into schema_migrations values($1)', [f]);
    console.log('applied', f);
  }
  await pool.end();
})().catch(e => { console.error(e); process.exit(1); });
EOF
cat > migrations/001_init.sql <<'EOF'
create table items(id serial primary key, title text not null);
insert into items(title) values ('first');
EOF
cat > test/api.test.js <<'EOF'
const test = require('node:test');
const assert = require('assert');
const base = 'http://127.0.0.1:3000';
test('health', async () => {
  const r = await fetch(base + '/health');
  assert.equal(r.status, 200);
});
test('create, list, delete', async () => {
  const r = await fetch(base + '/items', { method: 'POST', body: JSON.stringify({ title: 'from test' }) });
  assert.equal(r.status, 201);
  const made = await r.json();
  const list = await (await fetch(base + '/items')).json();
  assert.ok(list.some(i => i.id === made.id));
  await fetch(base + '/items/' + made.id, { method: 'DELETE' });
});
EOF
printf 'node_modules\n.env\n' > .gitignore
printf 'DATABASE_URL=postgres://app:secret@127.0.0.1:5432/appdb\nPORT=3000\nLOG_LEVEL=info\n' > .env
chown -R silo:silo /home/silo/app
su silo -c 'cd /home/silo/app && npm install --no-audit --no-fund >/home/silo/npm.log 2>&1 && npm run migrate && git init -q -b main && git config user.email silo@example.invalid && git config user.name Silo && git add -A && git commit -q -m "initial app"'
cat > /etc/init.d/web <<'EOF'
#!/bin/sh
### BEGIN INIT INFO
# Provides:          web
# Default-Start:     2 3 4 5
# Short-Description: the p2app web server
### END INIT INFO
case "$1" in
  start) cd /home/silo/app && { setsid nohup /usr/local/bin/web server.js >>/var/log/web.log 2>&1 </dev/null & echo $! > /run/web.pid; } ;;
  stop) [ -f /run/web.pid ] && kill $(cat /run/web.pid) 2>/dev/null; rm -f /run/web.pid; true ;;
  restart) "$0" stop; sleep 1; "$0" start ;;
  status) pidof web ;;
esac
exit 0
EOF
chmod 755 /etc/init.d/web
cat > /usr/local/bin/silo-p2-boot.sh <<'EOF'
#!/bin/bash
service postgresql start
for i in $(seq 60); do pg_isready -q -h 127.0.0.1 && break; sleep 1; done
service web start
EOF
chmod 755 /usr/local/bin/silo-p2-boot.sh
cat > /etc/xdg/autostart/silo-p2-stack.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=silo-p2-stack
Exec=sudo -n /usr/local/bin/silo-p2-boot.sh
EOF
echo setup done
SETUP
x $MAIN '/usr/local/bin/silo-p2-setup.sh 2>&1 | tail -5'; check "setup script ran" $? ""

echo "-- restart main so the stack starts only through the boot entry"
x $MAIN 'service web stop; service postgresql stop' >/dev/null 2>&1
"$SILO" stop $MAIN >/dev/null 2>&1; "$SILO" start $MAIN >/dev/null 2>&1; check "restart $MAIN (silo stop/start)" $? ""
ok=1; for i in $(seq 60); do stack_up $MAIN >/dev/null 2>&1 && { ok=0; break; }; sleep 3; done
check "stack (postgres + web on :3000) came up at boot" $ok "after ~$((i*3)) s; web pid $(webpid $MAIN)"
fi

echo "== project dir with the keep recipe"
mkdir -p "$PROJ/.silo"; cd "$PROJ"
cat > "$PROJ/.silo/recipe.yaml" <<'EOF'
keep:
  migrate: ["cd /home/silo/app && npm run migrate"]
  restart: [web]
  after: ["cd /home/silo/app && npm test"]
EOF
cat "$PROJ/.silo/recipe.yaml"
x $MAIN 'cd /home/silo/app && npm test 2>&1 | grep -E "^# (pass|fail)"'; check "main: npm test passes" $? ""
S0=$(mainstate); echo "main state before everything:"; echo "$S0"
PID0=$(webpid $MAIN); echo "main web pid $PID0"

echo "== three forks from main"
for c in $T1 $T2 $T3; do
  t=$(now); "$SILO" fork $MAIN --name $c >/dev/null 2>&1; r=$?; ft=$(( $(now)-t ))
  t=$(now); "$SILO" start $c >/dev/null 2>&1; r2=$?; st=$(( $(now)-t ))
  check "fork + start $c" $((r+r2)) "fork ${ft} s, start ${st} s"
done
ok=1; for c in $T1 $T2 $T3; do for i in $(seq 40); do stack_up $c >/dev/null 2>&1 && break; sleep 3; done; done
for c in $T1 $T2 $T3; do stack_up $c >/dev/null 2>&1; check "fork $c has the stack running (web pid $(webpid $c))" $? ""; done
echo "T1 fork head: $(x $T1 'git -C /home/silo/app rev-parse HEAD')"

cat > "$PROJ/mcp.json" <<EOF
{"mcpServers":{"silo":{"command":"$SILO","args":["mcp"]}}}
EOF
PRE() { cat <<EOF
You are working inside a Linux computer named $1. Reach it only through the silo MCP tools: call the exec tool with computer="$1" (each command runs as the unprivileged user "silo" and must finish within 170 seconds; use sudo only for what needs root, such as apt-get, service, or files outside /home/silo). A small Node web app lives in /home/silo/app (owned by silo, so run git, npm and edits there as silo, without sudo; it is a git repo and the git identity is already set). Postgres database appdb, DATABASE_URL in /home/silo/app/.env; the web server runs as the "web" service on port 3000 (sudo service web restart); npm run migrate applies migrations/; npm test runs the tests against the running server. Do only this task, on this computer only. Never print or commit .env or any credential. When finished, answer with one sentence.

Task: $2
EOF
}
TASK1="Install the CLI package ripgrep (apt package ripgrep) and add a \`search\` script to package.json that uses it (rg -n \"\$1\" server.js, for example). Put the new script as the last entry of the \"scripts\" object in package.json. Commit the change with git."
TASK2="Add an integer column \`priority\` (not null, default 0) to the items table through a NEW migration file in /home/silo/app/migrations (for example 002_priority.sql) and apply it with npm run migrate. Make POST /items accept an optional priority and GET /items return it, extend the tests accordingly, restart the web server, check npm test passes, and commit the code with git."
TASK3="Change the app's settings: add a line LOG_FLAG=verbose to /home/silo/app/.env (do not commit it), add the line  alias gs='git status'  to /home/silo/.bashrc, and create the setting file /home/silo/.config/p2app/settings.json containing {\"theme\":\"dark\"}. Do not commit anything."
runagent() { # computer task outfile
  PRE "$1" "$2" | (cd "$PROJ" && claude -p --model "$MODEL" --mcp-config "$PROJ/mcp.json" --strict-mcp-config --tools "" --allowedTools "mcp__silo__exec mcp__silo__status" --max-turns 40 --output-format json) > "$3" 2>"$3.err"
}
echo "== three live agent tasks (claude -p, model $MODEL, in parallel)"
t=$(now)
runagent $T1 "$TASK1" "$PROJ/a1.json" & P1=$!
runagent $T2 "$TASK2" "$PROJ/a2.json" & P2=$!
runagent $T3 "$TASK3" "$PROJ/a3.json" & P3=$!
wait $P1; R1=$?; wait $P2; R2=$?; wait $P3; R3=$?
say "agents finished after $(( $(now)-t )) s (exit $R1 $R2 $R3)"
for n in 1 2 3; do
  python3 - "$PROJ/a$n.json" "$n" <<'PY'
import json,sys
try:
    d=json.load(open(sys.argv[1]))
    print("T%s: turns=%s cost=$%.3f models=%s is_error=%s\n    result: %s"%(sys.argv[2],d.get("num_turns"),d.get("total_cost_usd",0),list(d.get("modelUsage",{}).keys()),d.get("is_error"),(d.get("result") or "")[:300].replace("\n"," ")))
except Exception as e:
    print("T%s: could not read agent output: %s"%(sys.argv[2],e)); print(open(sys.argv[1]).read()[:500])
PY
  cp "$PROJ/a$n.json" "$EVID/agent-t$n.json" 2>/dev/null
done

# bounded SECS OUTFILE CMD...: run a command, give up on it after SECS (a hung job is a result, not a stall)
bounded() {
  local secs=$1 out=$2; shift 2
  "$@" > "$out" 2>&1 & local pid=$!
  local i; for i in $(seq "$secs"); do kill -0 $pid 2>/dev/null || break; sleep 1; done
  if kill -0 $pid 2>/dev/null; then kill $pid 2>/dev/null; wait $pid 2>/dev/null; return 124; fi
  wait $pid; return $?
}
quiet_text() { grep -v '^\(Keeping\|Checking\|Undoing\|Putting\)[^:]*: [0-9]*s$' "$1"; }
ENGLOG="$REPO/app/SiloUI/src-tauri/target/verification/silod-dev-p2b.log"
engine_panic() { [ "${SHOW_ENGLOG:-0}" = 1 ] && [ -f "$ENGLOG" ] && echo "engine stderr ($ENGLOG): $(tail -n 3 "$ENGLOG" | tr '\n' ' ')"; }

dodiff() { # computer label
  local c=$1 t0 t1 lines
  t0=$(now); "$SILO" diff "$c" > "$EVID/$2.diff.txt" 2> "$EVID/$2.diff.err"; local rc=$?; t1=$(now)
  "$SILO" diff "$c" --json > "$EVID/$2.diff.json" 2>/dev/null
  lines=$(wc -l < "$EVID/$2.diff.txt" | tr -d ' ')
  echo "--- silo diff $c (rc=$rc, $((t1-t0)) s for capture+compare, $lines lines)"; cat "$EVID/$2.diff.txt"; sed 's/^/stderr: /' "$EVID/$2.diff.err" | head -5
  DLINES=$lines
}
jbuckets() { python3 - "$1" <<'PY'
import json,sys
d=json.load(open(sys.argv[1]))
for b in d.get("summary",[]): print("BUCKET", b["bucket"], "|", " ; ".join(b["lines"]))
PY
}

echo "== diffs"
for pair in "$T1:t1" "$T2:t2" "$T3:t3"; do
  c=${pair%%:*}; l=${pair##*:}
  dodiff $c $l; eval "L_$l=$DLINES"; jbuckets "$EVID/$l.diff.json" > "$EVID/$l.buckets.txt"
done
B1=$(cat "$EVID/t1.buckets.txt"); B2=$(cat "$EVID/t2.buckets.txt"); B3=$(cat "$EVID/t3.buckets.txt")
echo "T1 buckets:"; echo "$B1"; echo "T2 buckets:"; echo "$B2"; echo "T3 buckets:"; echo "$B3"
[ "$L_t1" -le 15 ] && [ "$L_t1" -gt 0 ]; check "T1 diff <= 15 lines" $? "$L_t1 lines"
echo "$B1" | grep -E '^BUCKET Packages' | grep -qi 'ripgrep'; ok1=$?
echo "$B1" | grep -E '^BUCKET Code' | grep -q '/app: '; ok2=$?
check "T1: ripgrep under Packages, commit under Code" $((ok1+ok2)) "packages=$ok1 code=$ok2"
[ "$L_t2" -le 15 ] && [ "$L_t2" -gt 0 ]; check "T2 diff <= 15 lines" $? "$L_t2 lines"
echo "$B2" | grep -E '^BUCKET Code' | grep -q '/app: '; ok1=$?
echo "$B2" | grep -E '^BUCKET Database changed' | grep -q 'schema changed'; ok2=$?
python3 -c "import json;d=json.load(open('$EVID/t2.diff.json'));import sys;sys.exit(0 if any('migrations/' in f for f in d.get('repo_files',[])+[k for r in d.get('code',[]) for k in r.get('dirty_new',{})]) else 1)"; ok3=$?
check "T2: migration under Code, schema under Database changed" $((ok1+ok2)) "code=$ok1 db=$ok2 (migrations/ file listed in repo_files/dirty: $ok3)"
[ "$L_t3" -le 15 ] && [ "$L_t3" -gt 0 ]; check "T3 diff <= 15 lines" $? "$L_t3 lines"
echo "$B3" | grep -E '^BUCKET Code' | grep -q '\.env changed (gitignored)'; ok1=$?
echo "$B3" | grep -E '^BUCKET Settings' | grep -q 'bashrc'; ok2=$?
echo "$B3" | grep -E '^BUCKET Settings' | grep -qE 'p2app|settings\.json|\.config'; ok3=$?
check "T3: .env under Code (gitignored), .bashrc and settings file under Settings & files" $((ok1+ok2+ok3)) "env=$ok1 bashrc=$ok2 settingsfile=$ok3"

echo "== keep (a): plain keep of T2 into main"
S_A0=$(mainstate); PID_A0=$(webpid $MAIN); echo "main pid before: $PID_A0; priority column on main: $(x $MAIN "sudo -u postgres psql -Atc \"select count(*) from information_schema.columns where table_name='items' and column_name='priority'\" appdb")"
bounded 120 "$EVID/fold-t2-dry.txt" "$SILO" fold $T2 --into $MAIN --dry-run; rc=$?
echo "-- silo fold --dry-run (rc=$rc):"; quiet_text "$EVID/fold-t2-dry.txt"
check "(a) silo fold --dry-run plans the keep" $rc ""
t=$(now); bounded 150 "$EVID/fold-t2.txt" "$SILO" fold $T2 --into $MAIN; rc=$?; KT=$(( $(now)-t ))
echo "-- silo fold $T2 --into $MAIN (rc=$rc, $KT s; 124 = still running after the 150 s bound):"; quiet_text "$EVID/fold-t2.txt" | tail -20
engine_panic
check "(a) silo fold $T2 --into $MAIN succeeded" $rc "rc=$rc after $KT s"
if [ $rc = 0 ]; then
  PID_A1=$(webpid $MAIN)
  grep -q 'migrate:' "$EVID/fold-t2.txt"; check "(a) migrate step ran" $? "$(grep -m1 'migrate:' "$EVID/fold-t2.txt")"
  [ "$PID_A1" != 0 ] && [ "$PID_A1" != "$PID_A0" ]; check "(a) web server pid changed" $? "$PID_A0 -> $PID_A1"
  x $MAIN "kill -0 $PID_A1 && curl -s -m 5 http://127.0.0.1:3000/health"; check "(a) web server running and answering" $? ""
  x $MAIN 'cd /home/silo/app && npm test 2>&1 | grep -E "^# (pass|fail)"'; check "(a) npm test passes on main" $? ""
  COL=$(x $MAIN "sudo -u postgres psql -Atc \"select data_type||' default '||column_default from information_schema.columns where table_name='items' and column_name='priority'\" appdb")
  [ -n "$COL" ]; check "(a) priority column exists on main" $? "$COL"
else
  S_A1=$(mainstate); echo "main after the failed keep: pid $(webpid $MAIN), fingerprint $( [ "$S_A0" = "$S_A1" ] && echo unchanged || echo CHANGED)"
  check "(a) migrate / pid change / npm test / column on main" 1 "not reachable: the keep did not finish"
fi
"$SILO" checkpoints $MAIN | head -6

echo "== (d) silo undo $MAIN after (a)"
t=$(now); bounded 90 "$EVID/undo.txt" "$SILO" undo $MAIN; rc=$?; UT=$(( $(now)-t ))
quiet_text "$EVID/undo.txt" | tail -12; engine_panic
if [ $rc = 0 ] && ! grep -q "Nothing to undo" "$EVID/undo.txt"; then
  check "(d) silo undo succeeded" $rc "rc=$rc, $UT s"
  UST=$("$SILO" list | awk -v n=$MAIN '$1==n{print $2}'); echo "state of $MAIN right after undo: $UST"
  if [ "$UST" != running ]; then echo "NOTE: undo leaves $MAIN $UST; starting it with silo start (restore does not restart the computer)"; "$SILO" start $MAIN >/dev/null 2>&1; fi
  ok=1; for i in $(seq 40); do stack_up $MAIN >/dev/null 2>&1 && { ok=0; break; }; sleep 3; done
  check "(d) server runs after undo" $ok "web pid $(webpid $MAIN)"
  COL2=$(x $MAIN "sudo -u postgres psql -Atc \"select count(*) from information_schema.columns where table_name='items' and column_name='priority'\" appdb")
  [ "$COL2" = 0 ]; check "(d) priority column is gone" $? "count=$COL2"
  S_D=$(mainstate)
  x $MAIN 'cd /home/silo/app && npm test 2>&1 | grep -E "^# (pass|fail)"'; check "(d) npm test passes at the pre-keep state" $? ""
  [ "$S_A0" = "$S_D" ]; check "(d) main's fingerprint equals the pre-keep one" $? "$( [ "$S_A0" = "$S_D" ] && echo identical || diff <(echo "$S_A0") <(echo "$S_D") | tr '\n' ' ')"
else
  check "(d) silo undo restores the pre-keep state" 1 "rc=$rc after $UT s (no successful keep to undo: $(head -c 200 "$EVID/undo.txt" | tr '\n' ' '))"
fi

echo "== keep (b): divergent keep of T1"
put $MAIN /tmp/p2-mainedit.sh 755 <<'E'
#!/bin/bash
cd /home/silo/app
sed -i 's|"test": "node --test test/"|"test": "node --test test/",\n    "lint": "echo lint"|' package.json
git commit -qam "add lint script on main"
git log --oneline | head -3
git diff HEAD~1 | grep '^[+-] '
E
x $MAIN 'sudo -u silo bash /tmp/p2-mainedit.sh; rm -f /tmp/p2-mainedit.sh'
echo "T1 changed on its side:"; x $T1 'cd /home/silo/app && sudo -u silo bash -c "git log --oneline | head -2 && git diff HEAD~1 -- package.json | grep \"^[+-] \""'
S_B0=$(mainstate); PID_B0=$(webpid $MAIN)
bounded 60 "$EVID/fold-t1-noinfo.txt" "$SILO" fold $T1 --dry-run; echo "-- silo fold $T1 --dry-run, no --into (rc=$?):"; quiet_text "$EVID/fold-t1-noinfo.txt" | head -5
grep -q 'not a computer' "$EVID/fold-t1-noinfo.txt"; check "fold's default target (the fork's source) resolves to the computer it was forked from" $((1-$?)) "$(head -c 200 "$EVID/fold-t1-noinfo.txt" | tr '\n' ' ')"
bounded 120 "$EVID/fold-t1-dry.txt" "$SILO" fold $T1 --into $MAIN --dry-run; echo "-- silo fold $T1 --into $MAIN --dry-run (rc=$?):"; quiet_text "$EVID/fold-t1-dry.txt" | head -20
t=$(now); bounded 100 "$EVID/fold-t1.txt" "$SILO" fold $T1 --into $MAIN; rc=$?; BT=$(( $(now)-t ))
echo "-- silo fold $T1 (rc=$rc, $BT s; 124 = still running after the 100 s bound):"; quiet_text "$EVID/fold-t1.txt" | head -20; engine_panic
[ "$rc" = 2 ] && grep -qiE 'diverg|conflict' "$EVID/fold-t1.txt"; check "(b) fold of T1 refused as divergent/conflict (rc 2)" $? "rc=$rc after $BT s"
grep -qiE 'diverg|conflict' "$EVID/fold-t1-dry.txt"; check "(b) fold --dry-run names the divergence" $? "$(grep -iE -m1 'diverg|conflict' "$EVID/fold-t1-dry.txt")"
S_B1=$(mainstate); PID_B1=$(webpid $MAIN)
[ "$S_B0" = "$S_B1" ] && [ "$PID_B0" = "$PID_B1" ]; check "(b) main byte-for-byte untouched (fingerprint and web pid)" $? "pid $PID_B0 -> $PID_B1; $( [ "$S_B0" = "$S_B1" ] && echo identical || diff <(echo "$S_B0") <(echo "$S_B1") | tr '\n' ' ')"

echo "== keep (c): uncovered data change"
"$SILO" fork $MAIN --name $TD >/dev/null 2>&1; "$SILO" start $TD >/dev/null 2>&1; check "fork + start $TD from main" $? ""
for i in $(seq 40); do stack_up $TD >/dev/null 2>&1 && break; sleep 3; done
x $TD "sudo -u postgres psql -c \"insert into items(title) select 'bulk '||g from generate_series(1,5) g\" appdb"
dodiff $TD data; jbuckets "$EVID/data.diff.json" > "$EVID/data.buckets.txt"; cat "$EVID/data.buckets.txt"
grep -q '^BUCKET Database changed' "$EVID/data.buckets.txt"; check "(c) diff shows Database changed" $? ""
S_C0=$(mainstate); PID_C0=$(webpid $MAIN)
bounded 60 "$EVID/fold-data-noinfo.txt" "$SILO" fold $TD --dry-run; echo "-- silo fold $TD --dry-run, no --into (rc=$?):"; quiet_text "$EVID/fold-data-noinfo.txt" | head -5
bounded 120 "$EVID/fold-data-dry.txt" "$SILO" fold $TD --into $MAIN --dry-run; echo "-- silo fold $TD --into $MAIN --dry-run (rc=$?):"; quiet_text "$EVID/fold-data-dry.txt" | head -20
t=$(now); bounded 100 "$EVID/fold-data.txt" "$SILO" fold $TD --into $MAIN; rc=$?; CT=$(( $(now)-t ))
echo "-- silo fold $TD (rc=$rc, $CT s):"; quiet_text "$EVID/fold-data.txt" | head -20; engine_panic
[ "$rc" = 2 ] && grep -q 'Make this main' "$EVID/fold-data.txt"; check "(c) fold refused, names Make this main (rc 2)" $? "rc=$rc after $CT s"
grep -q 'Make this main' "$EVID/fold-data-dry.txt"; check "(c) fold --dry-run names Make this main" $? ""
S_C1=$(mainstate); PID_C1=$(webpid $MAIN)
[ "$S_C0" = "$S_C1" ] && [ "$PID_C0" = "$PID_C1" ]; check "(c) main untouched" $? "pid $PID_C0 -> $PID_C1"

echo "== summary: $PASS passed, $FAILN failed"
echo "disk: $(df -h / | tail -1)"
