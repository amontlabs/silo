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
echo "-- gateway service"
x main 'ls -la /etc/xdg/autostart | grep -i openclaw; cat /etc/xdg/autostart/openclaw-gateway.desktop 2>&1 | head; cat /usr/local/bin/openclaw-gateway 2>&1 | head -5'
say "waiting 40 s for the autostart"; sleep 40
x main 'ps -eo pid,user,comm,args | grep -i "[o]penclaw" | cut -c1-170'
x main 'ps -eo comm,args | grep -ci "[o]penclaw"' | sed 's/^/openclaw-matching processes: /'
echo "-- what the gateway says when run in the foreground (10 s)"
xs main 'timeout 10 openclaw gateway 2>&1 | tail -15; echo exit=$?'
echo "-- where credentials and state live (names only)"
xs main 'find ~/.openclaw ~/.config/openclaw ~/.local/share/openclaw -maxdepth 2 2>/dev/null | head -40; ls -la ~ | grep -i claw'
xs main 'mkdir -p ~/.openclaw/credentials && echo FAKE-CHANNEL-LOGIN=not-real > ~/.openclaw/credentials/final-test.json && ls -la ~/.openclaw/credentials'
echo "== O2 plain fork of main: quiet and scrub from the recipe main was made from"
q=e2e-final-claw; "$SILO" fork main --name $q > "$RAW/claw-fork.out" 2>&1; "$SILO" start $q >/dev/null 2>&1
check "O2 fork started" $([ "$(state $q)" = running ]; echo $?) ""
say "fork settle 40 s"; sleep 40
echo "-- fork processes matching openclaw:"; x $q 'ps -eo pid,user,comm,args | grep -i "[o]penclaw" | cut -c1-170; echo count=$(ps -eo args | grep -ci "[o]penclaw")'
"$SILO" status $q | grep -i quiet; "$SILO" status $q --json > "$RAW/claw-status.json"
x $q 'ps -eo args | grep -i "[o]penclaw.*gateway"' >/dev/null; check "O2 no gateway process in the fork" $([ $? -ne 0 ]; echo $?) ""
xs $q 'test ! -e ~/.openclaw/credentials/final-test.json'; check "O2 scrub: ~/.openclaw/credentials/final-test.json is gone in the fork" $? ""
xs main 'test -e ~/.openclaw/credentials/final-test.json'; check "O2 main still has it" $? ""
echo "-- the fork keeps the install: $(xs $q 'command -v openclaw' | head -1)"
drop_one $q
}
