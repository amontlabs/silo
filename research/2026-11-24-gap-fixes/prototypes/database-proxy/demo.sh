#!/usr/bin/env bash
# Sets up a real Postgres 16 (TLS, scram-sha-256, real passwords), starts pgboundary.py in front of
# it, and runs the cases of the doc with psql. Needs: root (to run Postgres as the user postgres),
# openssl, psql, pgbench, and a Python with pglast and cryptography:
#   python3 -m venv /tmp/v && /tmp/v/bin/pip install pglast cryptography
#   PY=/tmp/v/bin/python bash demo.sh
# Everything lives in $D and is removed at the end, unless KEEP=1.
set -u
PY=${PY:-python3}
HERE=$(cd "$(dirname "$0")" && pwd)
D=${D:-/tmp/silo-g6-demo}
PGBIN=/usr/lib/postgresql/16/bin
PGPORT=55433 BPORT=56433
rm -rf "$D"; mkdir -p "$D"/certs "$D"/run; chown postgres "$D"
cd "$D"/certs
ca() { openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $1.key -out $1.pem \
  -days 7 -subj "/CN=$1" -addext basicConstraints=critical,CA:TRUE \
  -addext keyUsage=critical,keyCertSign,cRLSign 2>/dev/null; }
leaf() { openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout $1.key -out $1.csr -subj "/CN=$3" 2>/dev/null
  printf "subjectAltName=%s\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\n" "$4" > $1.ext
  openssl x509 -req -in $1.csr -CA $2.pem -CAkey $2.key -CAcreateserial -out $1.pem -days 7 -extfile $1.ext 2>/dev/null; }
ca upstream-ca; ca silo-ca
leaf server upstream-ca pg.upstream.test "DNS:pg.upstream.test,IP:127.0.0.1"
leaf boundary silo-ca db.prod.example "DNS:db.prod.example,DNS:db.ro.example"
chown postgres server.key server.pem; chmod 600 server.key

echo super-real-admin-pw > "$D"/admin.pw; chown postgres "$D"/admin.pw
su postgres -c "$PGBIN/initdb -D $D/data -U postgres --auth=scram-sha-256 --pwfile=$D/admin.pw" >/dev/null
cat >> "$D"/data/postgresql.conf <<EOF
port = $PGPORT
listen_addresses = '127.0.0.1'
unix_socket_directories = '$D'
ssl = on
ssl_cert_file = '$D/certs/server.pem'
ssl_key_file = '$D/certs/server.key'
EOF
printf "local all all scram-sha-256\nhostssl all all 127.0.0.1/32 scram-sha-256\n" > "$D"/data/pg_hba.conf
su postgres -c "$PGBIN/pg_ctl -D $D/data -l $D/pg.log start" >/dev/null; sleep 1.5
ADMIN="host=$D port=$PGPORT user=postgres"
export PGPASSWORD=super-real-admin-pw
psql "$ADMIN dbname=postgres" -q -c "create role app login password 'Real-App-Pa55w0rd-7f3e'" \
  -c "create role app_ro login password 'Real-RO-Pa55w0rd-91ac'" -c "create database shop owner app"
psql "$ADMIN dbname=shop" -q <<'EOF'
set role app;
create table orders(id serial primary key, total numeric);
insert into orders(total) select g from generate_series(1,5000) g;
create function archive_orders() returns int language plpgsql security definer as
  $$ declare n int; begin delete from orders where id > 4990; get diagnostics n = row_count; return n; end $$;
reset role;
grant usage on schema public to app_ro; grant select on all tables in schema public to app_ro;
EOF
unset PGPASSWORD
echo -n Real-App-Pa55w0rd-7f3e > "$D"/run/app.pw; echo -n Real-RO-Pa55w0rd-91ac > "$D"/run/ro.pw
chmod 600 "$D"/run/*.pw
cat > "$D"/run/config.json <<EOF
{"listen": "127.0.0.1:$BPORT",
 "tls": {"cert": "$D/certs/boundary.pem", "key": "$D/certs/boundary.key"},
 "record": "$D/run/record.jsonl", "holds_dir": "$D/run/holds",
 "routes": [
  {"name": "shop (prod, writes)", "sni": "db.prod.example", "user": "app", "database": "shop",
   "placeholder": "silo-placeholder-7Qx2-shop",
   "upstream": {"host": "127.0.0.1", "port": $PGPORT, "sni": "pg.upstream.test",
     "ca": "$D/certs/upstream-ca.pem", "user": "app", "password_file": "$D/run/app.pw"},
   "hold": ["destructive", "opaque", "admin", "session", "unparsed"], "hold_limit_s": 4},
  {"name": "shop (prod, read)", "sni": "db.ro.example", "user": "app", "database": "shop",
   "placeholder": "silo-placeholder-7Qx2-shop",
   "upstream": {"host": "127.0.0.1", "port": $PGPORT, "sni": "pg.upstream.test",
     "ca": "$D/certs/upstream-ca.pem", "user": "app_ro", "password_file": "$D/run/ro.pw"},
   "read_only_session": true, "deny": ["session", "admin", "opaque"]}]}
EOF
(cd "$HERE" && exec "$PY" pgboundary.py "$D"/run/config.json > "$D"/run/boundary.log 2>&1) &
BPID=$!; sleep 1.5

# The computer's DATABASE_URL: the real user, a placeholder password, the boundary's CA.
Q="sslmode=verify-full&sslrootcert=$D/certs/silo-ca.pem&hostaddr=127.0.0.1"
W="postgresql://app:silo-placeholder-7Qx2-shop@db.prod.example:$BPORT/shop?$Q&channel_binding=require"
R="postgresql://app:silo-placeholder-7Qx2-shop@db.ro.example:$BPORT/shop?$Q"
say() { printf '\n### %s\n' "$*"; }

say "1. A read through the boundary: SCRAM-SHA-256-PLUS with the placeholder, the real password upstream"
psql "$W" -c "select current_user, count(*) from orders"
tail -1 "$D"/run/record.jsonl
say "2. A wrong password is refused by the boundary; the placeholder sent to the real server is refused there"
psql "${W/silo-placeholder-7Qx2-shop/guess}" -c "select 1" 2>&1 | tail -1
PGPASSWORD=silo-placeholder-7Qx2-shop psql "host=127.0.0.1 port=$PGPORT user=app dbname=shop sslmode=require" -c "select 1" 2>&1 | tail -1
say "3. DROP TABLE, nobody answers"
time psql "$W" -c "DROP TABLE orders"
say "4. DELETE without WHERE inside a transaction, denied by a file; the transaction stays consistent"
( sleep 1; for f in "$D"/run/holds/*.json; do i=${f%.json}; [ -e $i.seen ] || touch $i.deny $i.seen; done ) & H=$!
psql "$W" <<'EOF'
BEGIN;
DELETE FROM orders;
SELECT 1;
ROLLBACK;
SELECT count(*) AS orders_after FROM orders;
EOF
wait $H
say "5. The extended protocol (Parse/Bind/Execute): TRUNCATE, allowed by a file"
psql "$W" -q -c "create table scratch(a int)" 2>&1 | tail -1
( sleep 1; for f in "$D"/run/holds/*.json; do i=${f%.json}; [ -e $i.seen ] || touch $i.allow $i.seen; done ) & H=$!
printf 'TRUNCATE scratch \\bind \\g\n' | psql "$W"
wait $H
say "6. A DO block: the boundary cannot tell what it does, and the rule holds it"
psql "$W" -c "DO \$\$ BEGIN EXECUTE format('DROP TABLE %I', 'orders'); END \$\$"
say "7. Ctrl-C during a hold: psql's cancel request ends the hold"
psql "$W" -c "TRUNCATE orders" & P=$!; sleep 1; kill -INT $P; wait $P
say "8. The read route: a read role plus a read-only session"
psql "$R" <<'EOF'
SELECT current_user, current_setting('default_transaction_read_only') AS read_only;
DELETE FROM orders WHERE id = 1;
SELECT archive_orders();
BEGIN READ WRITE;
DO $$ BEGIN PERFORM set_config('default_transaction_read_only', 'off', false); END $$;
EOF
say "9. Added latency, pgbench -S, one client, 5 s each"
DIRECT="host=127.0.0.1 port=$PGPORT user=app dbname=shop sslmode=require password=Real-App-Pa55w0rd-7f3e"
PGPASSWORD=Real-App-Pa55w0rd-7f3e pgbench -q -i -s 2 "$DIRECT" >/dev/null 2>&1
VIA="host=db.prod.example hostaddr=127.0.0.1 port=$BPORT user=app dbname=shop sslmode=require password=silo-placeholder-7Qx2-shop"
for M in simple prepared; do
  echo "$M direct:  $(pgbench -n -S -M $M -T 5 "$DIRECT" 2>&1 | grep 'latency average')"
  echo "$M through: $(pgbench -n -S -M $M -T 5 "$VIA" 2>&1 | grep 'latency average')"
done

kill $BPID
su postgres -c "$PGBIN/pg_ctl -D $D/data stop" >/dev/null
[ "${KEEP:-0}" = 1 ] || rm -rf "$D"
