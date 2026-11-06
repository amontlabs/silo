# Section 4 of final_run.sh (sourced): the OpenClaw recipe, resolving the CONFIRM lines of the built-in recipe.
# main is replaced by an OpenClaw main for this section (dropped first when this run made it).
claw_linux() {
cd "$PROJ" || return
echo "== O1 silo up --recipe openclaw (replaces our demo main)"
if exists main && [ "$MADE_MAIN" = 1 ]; then drop_one main; elif exists main; then blocked "OpenClaw" "main exists and is not ours"; return; fi
t0=$(now); "$SILO" up --recipe openclaw > "$RAW/claw-up.out" 2>&1; rc=$?; MADE_MAIN=1
tail -8 "$RAW/claw-up.out"; check "O1 silo up --recipe openclaw" $rc "$(elapsed "$t0")s, includes the install (not a measurement)"
[ $rc = 0 ] || { blocked "OpenClaw" "recipe build failed: $(grep -iE 'error|fail' "$RAW/claw-up.out" | head -2 | tr '\n' ' ')"; return; }
[ "$(state main)" = running ] || "$SILO" start main >/dev/null 2>&1
echo "-- install"
xs main 'echo "PATH=$PATH"; command -v openclaw; openclaw --version 2>&1 | head -2; node --version 2>&1; ls -la ~/.openclaw 2>&1 | head -20'
xs main 'command -v openclaw' >/dev/null; check "O1 openclaw is installed (on the working account's PATH)" $? ""
echo "-- gateway service: unconfigured it exits (no account needed for the baseline config below)"
x main 'ls -la /etc/xdg/autostart | grep -i openclaw; cat /usr/local/libexec/silo-services/openclaw-gateway'
xs main 'timeout 10 openclaw gateway 2>&1 | grep -i "missing config"'
xs main 'openclaw setup --baseline 2>&1 | tail -3; openclaw config set gateway.mode local 2>&1 | tail -1
mkdir -p ~/.openclaw/credentials && echo FAKE-CHANNEL-LOGIN=not-real > ~/.openclaw/credentials/final-test.json
echo FAKE_BOT_TOKEN=not-real > ~/.openclaw/.env; find ~/.openclaw -maxdepth 2 | sort | head -30'
echo "-- restart main so the autostart entry starts the gateway at boot"
"$SILO" stop main >/dev/null 2>&1; "$SILO" start main >/dev/null 2>&1; say "waiting 40 s"; sleep 40
x main 'ps -eo pid,user,comm,args | grep -i "[o]penclaw" | cut -c1-150'
x main 'pgrep -fc "[o]penclaw-gateway"' | sed 's/^/gateway processes in main: /'
check "O1 the gateway runs in main as openclaw-gateway" $([ "$(x main 'pgrep -c -f "[o]penclaw-gateway"' | tail -1)" -ge 1 ]; echo $?) ""
echo "== O2 control fork of main (no recipe lists): the gateway comes up again, the files stay"
echo "-- OpenClaw keeps a single-gateway lock in ~/.openclaw/state/openclaw.sqlite; a fork copies main's fresh heartbeat,"
echo "-- so the fork's first boot start is refused until the heartbeat is 90 s old and nothing retries it. The check"
echo "-- therefore restarts each fork once the copied heartbeat is stale and looks at the gateway after that boot."
gw() { x $1 'pgrep -c -f "[o]penclaw-gateway"' | tail -1; }
# restart_stale C T0: stop and start C once at least 95 s have passed since T0 (the fork), then wait up to 60 s for a gateway
restart_stale() { local w; w=$(python3 -c "print(max(0, int(95 - ($(now) - $2))))"); say "waiting ${w} s until main's copied heartbeat is stale, then restarting $1"; sleep "$w"
  "$SILO" stop $1 >/dev/null 2>&1; "$SILO" start $1 >/dev/null 2>&1; for i in $(seq 12); do [ "$(gw $1)" -ge 1 ] && break; sleep 5; done; }
c=e2e-final-claw-ctl; t0=$(now); "$SILO" fork main --name $c > "$RAW/claw-ctl.out" 2>&1; "$SILO" start $c >/dev/null 2>&1; sleep 20
echo "first boot: gateway processes $(gw $c); $(xs $c 'grep -ho "GatewayLockNamespaceError[^.]*\.[^.]*\. holder [^,]*" /tmp/openclaw/*.log | tail -1')"
restart_stale $c "$t0"; x $c 'ps -eo pid,comm,args | grep -i "[o]penclaw" | cut -c1-110'; "$SILO" status $c | grep -i quiet
check "O2 control: gateway runs in the fork after a restart (what quiet must prevent)" $([ "$(gw $c)" -ge 1 ]; echo $?) ""
xs $c 'test -e ~/.openclaw/.env'; check "O2 control: .env is carried (no scrub list)" $? ""
drop_one $c
echo "== O3 quiet fork: --recipe-file with quiet [openclaw-gateway, openclaw], scrub [/home/silo/.openclaw/credentials, /home/silo/.openclaw/.env]"
q=e2e-final-claw; t0=$(now); "$SILO" fork main --name $q --recipe-file "$PROJ/.silo/recipes/openclaw.yaml" > "$RAW/claw-fork.out" 2>&1; "$SILO" start $q >/dev/null 2>&1
check "O3 fork started" $([ "$(state $q)" = running ]; echo $?) ""
restart_stale $q "$t0"
echo "-- processes matching openclaw in the quiet fork:"; x $q 'ps -eo pid,comm,args | grep -i "[o]penclaw" | cut -c1-110; echo count=$(pgrep -c -f "[o]penclaw")'
"$SILO" status $q | grep -i quiet; "$SILO" status $q --json > "$RAW/claw-status.json"
check "O3 no openclaw gateway process in the fork" $([ "$(x $q 'pgrep -c -f "[o]penclaw-gateway"' | tail -1)" = 0 ]; echo $?) ""
xs $q 'test ! -e ~/.openclaw/credentials/final-test.json && test ! -e ~/.openclaw/.env'; check "O3 scrub: credentials/ file and .env are gone in the fork" $? ""
xs main 'test -e ~/.openclaw/credentials/final-test.json && test -e ~/.openclaw/.env'; check "O3 main still has both" $? ""
echo "-- the fork keeps the install and the baseline config: $(xs $q 'command -v openclaw; test -e ~/.openclaw/openclaw.json && echo config-present' | tr '\n' ' ')"
drop_one $q
}
