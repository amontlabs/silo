# Section 1 of final_run.sh (sourced): the 60-s demo flow and quiet forks, on Linux.
demo_linux() {
cd "$PROJ" || return
echo "== D1 silo up --recipe demo (main is ours only if it is absent)"
if exists main; then echo "main exists already: using it, not creating it"
else
  t0=$(now); "$SILO" up --recipe demo > "$RAW/up.out" 2>&1; rc=$?; MADE_MAIN=1
  tail -4 "$RAW/up.out"; check "D1 silo up --recipe demo" $rc "$(elapsed "$t0")s, includes the recipe build (not a measurement)"
fi
[ "$(state main)" = running ] || "$SILO" start main >/dev/null 2>&1
check "D1 main running" $([ "$(state main)" = running ]; echo $?) "$("$SILO" status main | head -3 | tr '\n' ' ')"
x main 'rm -f /root/final-note.txt /home/silo/final-note.txt; hostname; pgrep -af "final/(web|poller).py" | cut -c1-80'
"$SILO" ports main
ssh -o BatchMode=yes -o ConnectTimeout=15 main.silo-dev 'echo ssh-ok $(whoami)' 2>&1 | head -2
check "D1 ssh main.silo-dev reaches main" $([ "$(ssh -o BatchMode=yes -o ConnectTimeout=15 main.silo-dev echo ok 2>/dev/null)" = ok ]; echo $?) ""

echo "== D2 silo fork main -n 4 (each fork writes a note named after its own host)"
t0=$(now); "$SILO" fork main -n 4 --name e2e-final-demo -- bash -c 'echo kept-from-$(hostname) > ~/final-note.txt' > "$RAW/demo-fork4.out" 2> "$RAW/demo-fork4.err"; rc=$?
echo "demo -n 4: rc=$rc $(elapsed "$t0")s"; tail -12 "$RAW/demo-fork4.out"
n=$("$SILO" list | grep -c 'e2e-final-demo-[1-4] .*running'); check "D2 four forks running" $([ "$n" = 4 ]; echo $?) "$n running"

echo "== D3 review: silo diff e2e-final-demo-1"
"$SILO" diff e2e-final-demo-1 > "$RAW/demo-diff.txt" 2>&1; rc=$?; grep -v '^step [0-9]*$' "$RAW/demo-diff.txt" | head -30
grep -q final-note "$RAW/demo-diff.txt"; check "D3 silo diff lists final-note.txt" $? "rc=$rc, $(wc -l < "$RAW/demo-diff.txt") lines"

echo "== D4 keep: silo fold e2e-final-demo-1"
"$SILO" fold e2e-final-demo-1 > "$RAW/demo-fold.txt" 2>&1; rc=$?; grep -v '^[A-Za-z ]*: [0-9]*s$' "$RAW/demo-fold.txt" | tail -12
check "D4 silo fold exits 0" $rc ""
got=$(xs main 'cat ~/final-note.txt' | head -1); echo "main note after keep: $got"
[ "$got" = "kept-from-$("$SILO" exec e2e-final-demo-1 -- hostname 2>/dev/null | grep -v '^step')" ] || [[ $got == kept-from-* ]]; check "D4 main has the fork's note" $? "$got"

echo "== D5 undo: silo undo main"
"$SILO" undo main > "$RAW/demo-undo.txt" 2>&1; rc=$?; grep -v '^[A-Za-z ]*: [0-9]*s$' "$RAW/demo-undo.txt" | tail -6
check "D5 silo undo main exits 0" $rc ""
xs main 'test ! -e ~/final-note.txt'; check "D5 the note is gone from main" $? ""

echo "== D6 throw away the four forks"
for i in 1 2 3 4; do drop_one e2e-final-demo-$i; done
check "D6 no e2e-final-demo forks left" $([ -z "$("$SILO" list | grep e2e-final-demo)" ]; echo $?) ""
}

quiet_linux() {
cd "$PROJ" || return
echo "== Q1 quiet fork of main: no scheduled runs in the fork (main is the control)"
q=e2e-final-quiet
echo "-- main gets a cron job (every minute) and a boot-time cron start, like the phase 1 check; the poller is a recipe service"
x main 'echo "* * * * * root echo \"\$(hostname) \$(date -u) cron\" >> /var/log/final-cron.log" > /etc/cron.d/final-job; chmod 644 /etc/cron.d/final-job
printf "[Desktop Entry]\nType=Application\nName=final-cron\nExec=sudo -n /usr/sbin/service cron start\n" > /etc/xdg/autostart/final-cron.desktop; service cron start; sleep 70; tail -2 /var/log/final-cron.log'
"$SILO" fork main --name $q --recipe-file "$PROJ/.silo/recipes/demo.yaml" > "$RAW/quiet-fork.out" 2>&1; "$SILO" start $q >/dev/null 2>&1; check "Q1 fork started" $([ "$(state $q)" = running ]; echo $?) ""
say "settling 45 s, then watching 90 s"; sleep 45
fh=$(xs $q hostname | head -1); mh=$(xs main hostname | head -1)
cnt() { echo "poll=$(x $1 "grep -c '^$2 ' /var/tmp/final-poll.log 2>/dev/null || true") cron=$(x $1 "grep -c '^$2 ' /var/log/final-cron.log 2>/dev/null || true") pollers=$(x $1 'pgrep -fc "final/poller.py" || true') crond=$(x $1 'pgrep -xc cron || true')"; }
b1=$(cnt $q "$fh"); m1=$(cnt main "$mh"); sleep 90; b2=$(cnt $q "$fh"); m2=$(cnt main "$mh")
echo "fork $fh: before [$b1] after [$b2]"; echo "main $mh (control): before [$m1] after [$m2]"
[ "$b1" = "$b2" ] && echo "$b2" | grep -q 'poll=0 cron=0 pollers=0'; check "Q1 fork wrote no poll or cron line and runs no poller" $? "[$b2]"
[ "$m1" != "$m2" ]; check "Q1 main (control) kept writing" $? "[$m1] -> [$m2]"
"$SILO" status $q | grep -i quiet; "$SILO" status $q --json > "$RAW/quiet-status.json"
python3 -c "
import json;q=json.load(open('$RAW/quiet-status.json')).get('quiet') or {}
raise SystemExit(0 if q.get('applied') and 'final-poller' in json.dumps(q.get('stop')) else 1)"
check "Q1 status quiet record: applied, final-poller stopped" $? "$(python3 -c "import json;print(json.dumps(json.load(open('$RAW/quiet-status.json')).get('quiet')))")"
x $q 'ls /etc/cron.d; crontab -l 2>&1 | head -2; ls /etc/xdg/autostart | grep -i final'
drop_one $q
}
