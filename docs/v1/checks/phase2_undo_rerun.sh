#!/usr/bin/env bash
# Silo v1 phase 2 re-run, A: Linux undo exactness (3 cycles). Run:
#   bash docs/v1/checks/phase2_undo_rerun.sh 2>&1 | tee docs/v1/evidence/phase-2/linux-undo-rerun.txt
# Only `silo`; no `silo start` after undo (the product must bring main back running).
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
SILO="$REPO/app/SiloUI/target/debug/silo"
BUNDLE="$REPO/app/SiloUI/target/debug/bundle/macos/Silo Dev.app"
MAIN=e2e-p2r-main
PROJ=$(mktemp -d "${TMPDIR:-/tmp}/e2e-p2r-proj.XXXXXX")
PASS=0; FAILN=0
ts() { date +%H:%M:%S; }
say() { echo "[$(ts)] $*"; }
check() { local name=$1 ok=$2; shift 2; if [ "$ok" = 0 ]; then echo "[$(ts)] PASS  $name  -- $*"; PASS=$((PASS+1)); else echo "[$(ts)] FAIL  $name  -- $*"; FAILN=$((FAILN+1)); fi; }
x() { local c=$1; shift; "$SILO" exec "$c" --timeout 170 -- sudo bash -c "$*"; }
put() { local b; b=$(base64 | tr -d '\n'); x "$1" "echo $b | base64 -d > $2 && chmod ${3:-644} $2"; }
now() { date +%s; }
cleanup() {
  say "cleanup"
  for c in $("$SILO" list | awk '$1 ~ /^e2e-p2r-/{print $1}'); do "$SILO" stop "$c" >/dev/null 2>&1; done
  for c in $("$SILO" list | awk '$1 ~ /^e2e-p2r-/{print $1}'); do for i in $(seq 30); do "$SILO" drop "$c" --force >/dev/null 2>&1 && break; "$SILO" status "$c" >/dev/null 2>&1 || break; sleep 3; done; done
  rm -rf "$PROJ"; say "silo list after cleanup:"; "$SILO" list
  echo "== summary: $PASS passed, $FAILN failed"
}
trap cleanup EXIT
echo "== header"
echo "commit: $(git -C "$REPO" rev-parse HEAD); git status --porcelain app: [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "bundled msb: $("$BUNDLE/Contents/MacOS/msb" --version 2>&1 | head -1); silo: $("$SILO" --version); host $(sw_vers -productVersion) $(uname -m); date $(date -u +%FT%TZ)"
mainstate() { # a fingerprint of main's repo, database, packages and settings (no pids)
  x $MAIN 'cd /home/silo/app && G="git -c safe.directory=/home/silo/app" && echo head=$($G rev-parse HEAD) && echo status=[$($G status --porcelain | tr "\n" " ")] && echo files=$($G ls-files -z | xargs -0 sha256sum | sha256sum | cut -c1-16) && echo env=$(sha256sum .env | cut -c1-16) && echo schema=$(sudo -u postgres pg_dump -s appdb | grep -av "restrict" | sha256sum | cut -c1-16) && echo data=$(sudo -u postgres pg_dump -a appdb | grep -av "restrict" | sha256sum | cut -c1-16) && echo dpkg=$(dpkg-query -W | sha256sum | cut -c1-16) && echo bashrc=$(sha256sum /home/silo/.bashrc | cut -c1-16) && echo cfg=$(ls -A /home/silo/.config 2>/dev/null | tr "\n" ",")'
}
webpid() { x $1 'pidof -s web || echo 0'; }
stack_up() { # web answers and postgres answers
  x $1 'pg_isready -q -h 127.0.0.1 && curl -s -m 5 http://127.0.0.1:3000/items | head -c 200 | grep -q "^\["'
}


echo "== main $MAIN: create, install the stack"
"$SILO" create $MAIN --linux --cpus 2 --memory 3 --disk 10 >/dev/null 2>&1; check "create $MAIN" $? ""
"$SILO" start $MAIN >/dev/null 2>&1; check "start $MAIN" $? ""
echo "guest OS: $(x $MAIN '. /etc/os-release; echo $PRETTY_NAME kernel $(uname -r)')"
x $MAIN 'rm -f /root/apt.rc; export DEBIAN_FRONTEND=noninteractive; nohup bash -c "apt-get update -qq; apt-get install -y -qq postgresql nodejs npm git curl; echo \$? > /root/apt.rc" >/root/apt.log 2>&1 </dev/null &'
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


x $MAIN 'service web stop; service postgresql stop' >/dev/null 2>&1
"$SILO" stop $MAIN >/dev/null 2>&1; "$SILO" start $MAIN >/dev/null 2>&1; check "restart $MAIN" $? ""
ok=1; for i in $(seq 60); do stack_up $MAIN >/dev/null 2>&1 && { ok=0; break; }; sleep 3; done
check "stack came up at boot" $ok "web pid $(webpid $MAIN)"

mkdir -p "$PROJ/.silo"; cd "$PROJ"
cat > "$PROJ/.silo/recipe.yaml" <<'EOF'
keep:
  migrate: ["cd /home/silo/app && npm run migrate"]
  restart: [web]
  after: ["cd /home/silo/app && npm test"]
EOF
for n in 1 2 3; do
  F=e2e-p2r-f$n
  echo "== cycle $n"
  S0=$(mainstate); P0=$(webpid $MAIN); echo "pre-keep: pid=$P0"; echo "$S0"
  "$SILO" fork $MAIN --name $F >/dev/null 2>&1; r=$?; "$SILO" start $F >/dev/null 2>&1; check "cycle $n fork+start" $((r+$?)) ""
  for i in $(seq 40); do stack_up $F >/dev/null 2>&1 && break; sleep 3; done
  x $F "sudo -u silo bash -c 'cd /home/silo/app && echo \"alter table items add column cyc$n integer not null default 0;\" > migrations/002_cyc$n.sql && npm run migrate && git add -A && git commit -qm \"cycle $n migration\"'"; check "cycle $n migration applied+committed in fork" $? ""
  "$SILO" fold $F --into $MAIN > "$PROJ/fold$n.txt" 2>&1; rc=$?; grep -v '^\(Keeping\|Checking\|Undoing\|Putting\)[^:]*: [0-9]*s$' "$PROJ/fold$n.txt" | tail -12
  check "cycle $n fold" $rc "rc=$rc"
  c=$(x $MAIN "sudo -u postgres psql -Atc \"select count(*) from information_schema.columns where table_name='items' and column_name='cyc$n'\" appdb")
  [ "$c" = 1 ]; check "cycle $n column exists on main after fold" $? "count=$c"
  "$SILO" undo $MAIN > "$PROJ/undo$n.txt" 2>&1; rc=$?; grep -v '^\(Keeping\|Checking\|Undoing\|Putting\)[^:]*: [0-9]*s$' "$PROJ/undo$n.txt" | tail -8
  check "cycle $n undo" $rc "rc=$rc"
  st=$("$SILO" list | awk -v n=$MAIN '$1==n{print $2}'); echo "state right after undo (no script start): $st"
  [ "$st" = running ]; check "cycle $n main running after undo" $? "$st"
  ok=1; for i in $(seq 40); do stack_up $MAIN >/dev/null 2>&1 && { ok=0; break; }; sleep 3; done
  check "cycle $n stack answers after undo" $ok ""
  S1=$(mainstate); P1=$(webpid $MAIN); echo "post-undo: pid=$P1"
  [ "$S0" = "$S1" ]; check "cycle $n fingerprint identical (git/files/schema/data/packages)" $? "$( [ "$S0" = "$S1" ] && echo identical || diff <(echo "$S0") <(echo "$S1") | tr '\n' ' ')"
  [ "$P0" = "$P1" ]; check "cycle $n web pid unchanged" $? "$P0 -> $P1"
  "$SILO" stop $F >/dev/null 2>&1; "$SILO" drop $F --force >/dev/null 2>&1
done
echo "disk: $(df -h / | tail -1)"
