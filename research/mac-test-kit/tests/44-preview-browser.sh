#!/bin/bash
# id: 44
# title: The browser Silo starts for a port preview, on a Mac: names, WebRTC, other computers, its own traffic
# tier: guided
# time: about 5 minutes; you look at Chrome's window once
# needs: Google Chrome in /Applications (or KIT_44_CHROME), no VM; sudo with --allow-sudo only, to watch the Mac's name lookups with tcpdump (without it, the lookups half is not watched and the outcome is inconclusive)
# step: later
# decides: ports-reached-from-your-mac (#384's set-up: a browser of Silo's own per computer, a rule proxy, --host-resolver-rules so the browser looks no name up itself)
# question: #384 ran Chrome for Testing headless on Linux. On a Mac, in a headed, branded Google Chrome started with #384's flags (its own profile, a rule proxy per computer, WebRTC only through the proxy, no name looked up by the browser) and a page that tries every way out #384 measured, does any name carrying the page's secret reach the Mac's resolver, does the page reach another computer's preview, and what does a branded Chrome send by itself? Without --host-resolver-rules (the control), does the TURN server's name leak as #384 found?
# outcome flags-hold: With #384's flags no name carrying the secret was looked up on the Mac (watched with tcpdump) and no request reached
#   the other computer's preview: ports-reached-from-your-mac can take #384's set-up on a Mac; the facts list what Chrome sent by itself.
# outcome flags-leak: With #384's flags, a name carrying the secret reached the Mac's resolver, or the page reached the other computer's
#   preview; the sentence names the way. The set-up needs another wall on a Mac.
# outcome inconclusive: Chrome did not load the page, or the lookups were not watched (no --allow-sudo); the facts hold what was seen.
. "$KIT_ROOT/lib/common.sh"

W="$KIT_WORK"
LINUX=0
if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then LINUX=1; fi
if [ -n "${KIT_44_CHROME:-}" ]; then CHROME="$KIT_44_CHROME"
elif [ "$LINUX" = 1 ]; then CHROME="$(ls /opt/pw-browsers/chromium-*/chrome-linux*/chrome 2>/dev/null | head -1)"
else CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"; fi
[ -x "$CHROME" ] || kit_skip "needs Google Chrome in /Applications (or KIT_44_CHROME)"
kit_fact chrome "$("$CHROME" --version 2>/dev/null | head -1)"
SECS="${KIT_44_SECONDS:-20}"
SECRET="s$(python3 -c 'import secrets; print(secrets.token_hex(5))')"
D="$W/p"; mkdir -p "$D"; chmod 700 "$W" "$D"
FWD=""; PX=""; DUMP=""; CPID=""
cleanup() {
  [ -n "$CPID" ] && kill "$CPID" 2>/dev/null
  [ -n "$FWD" ] && kill "$FWD" 2>/dev/null
  [ -n "$PX" ] && kill "$PX" 2>/dev/null
  [ -n "$DUMP" ] && { if [ "$LINUX" = 1 ]; then kill "$DUMP"; else sudo kill "$DUMP"; fi; } 2>/dev/null
  rm -rf "$D/profile-"*
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

python3 -I "$KIT_ROOT/py/44-preview.py" --dir "$D" --secret "$SECRET" > "$D/fwd.log" 2>&1 & FWD=$!
python3 -I "$KIT_ROOT/py/kitproxy.py" --tcp 127.0.0.1:0 --log "$D/proxy.jsonl" --ready "$D/proxy.ready" \
  --allow '*.c1a7.localhost' --map '*.c1a7.localhost=127.0.0.1' > "$D/proxy.log" 2>&1 & PX=$!
i=0; while { [ ! -f "$D/forwarder.port" ] || [ ! -f "$D/proxy.ready" ]; } && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
FP="$(cat "$D/forwarder.port")"; PP="$(cut -d: -f2 "$D/proxy.ready")"
[ -n "$FP" ] && [ -n "$PP" ] || kit_fail "the forwarder or the rule proxy did not start"

WATCH=0
if [ "$LINUX" = 1 ] && [ "$(id -u)" = 0 ] && command -v tcpdump >/dev/null 2>&1; then
  tcpdump -l -n -i any port 53 > "$D/dns.txt" 2>/dev/null & DUMP=$!; WATCH=1
elif [ "$LINUX" = 0 ] && [ "${KIT_SUDO:-0}" = 1 ]; then
  kit_sudo "watch the Mac's name lookups (port 53) for the page's secret, until the test ends" true
  # shellcheck disable=SC2024  # the pid file is the user's own
  sudo sh -c "tcpdump -l -n -i any port 53 > '$D/dns.txt' 2>/dev/null & echo \$!" > "$D/dump.pid"; DUMP="$(cat "$D/dump.pid")"; WATCH=1
fi
sleep 1
kit_fact lookups_watched "$([ "$WATCH" = 1 ] && echo yes || echo "no (rerun with --allow-sudo)")"

run() { # label [extra flags...]
  local l="$1"; shift
  : > "$D/report.jsonl"
  local extra=""; [ "$LINUX" = 1 ] && extra="--headless=new --no-sandbox"
  # shellcheck disable=SC2086
  "$CHROME" --user-data-dir="$D/profile-$l" --no-first-run --no-default-browser-check \
    --proxy-server="http://127.0.0.1:$PP" --proxy-bypass-list='<-loopback>' \
    --force-webrtc-ip-handling-policy=disable_non_proxied_udp "$@" $extra \
    --app="http://silo.c1a7.localhost:$FP/3000" > "$D/chrome-$l.log" 2>&1 &
  CPID=$!
  sleep "$SECS"
  kill "$CPID" 2>/dev/null; sleep 1; kill -9 "$CPID" 2>/dev/null; wait "$CPID" 2>/dev/null; CPID=""
  cp "$D/report.jsonl" "$W/report-$l.jsonl"; kit_attach "$W/report-$l.jsonl"
  sleep 2
}
mark() { date +%s > "$D/mark-$1"; wc -l < "$D/dns.txt" 2>/dev/null | tr -d ' ' > "$D/dnsline-$1" || echo 0 > "$D/dnsline-$1"; wc -l < "$D/proxy.jsonl" 2>/dev/null | tr -d ' ' > "$D/pxline-$1" || echo 0 > "$D/pxline-$1"; }
leaks_of() { # label: secret names seen at the resolver during that run
  local from; from="$(cat "$D/dnsline-$1" 2>/dev/null || echo 0)"
  tail -n "+$((from + 1))" "$D/dns.txt" 2>/dev/null | grep -o "[a-z]*-$SECRET" | sort -u | tr '\n' ' '
}

mark silo; run silo --host-resolver-rules="MAP * ~NOTFOUND, EXCLUDE 127.0.0.1"; mark after-silo
if [ "$LINUX" = 0 ]; then
  kit_fact vscode_prompt "$(kit_ask_yn "Did Chrome ask whether to open Visual Studio Code (or another app) while the page was open?")"
fi
loaded="$(grep -c '"what": "done"' "$W/report-silo.jsonl" || true)"
silo_dns="$(leaks_of silo)"
cross="$(grep -c 'c2b8' "$D/forwarder.jsonl" 2>/dev/null || true)"
kit_fact silo.secret_lookups "${silo_dns:-none}"
kit_fact silo.other_computer_requests "${cross:-0}"
kit_fact silo.proxy_saw_secret_names "$(grep -o "[a-z]*-${SECRET}[a-z.-]*" "$D/proxy.jsonl" 2>/dev/null | sort -u | tr '\n' ' ')"
kit_fact silo.webrtc_candidates "$(grep -c webrtc-candidate "$W/report-silo.jsonl" || true)"
from="$(cat "$D/pxline-silo")"
kit_fact chrome_own_hosts "$(tail -n "+$((from + 1))" "$D/proxy.jsonl" 2>/dev/null | python3 -c '
import json, sys
hs = sorted({json.loads(l)["host"] for l in sys.stdin if l.strip()} )
print(" ".join(h for h in hs if "localhost" not in h and "silokit-leak" not in h) or "none")')"

run control; ctl_dns="$(leaks_of after-silo)"
kit_fact control.secret_lookups "${ctl_dns:-none}"

where="$([ "$LINUX" = 1 ] && echo "Linux, headless Chromium: " || echo "")"
if [ "${loaded:-0}" = 0 ]; then
  kit_outcome inconclusive "${where}the page never reported in (Chrome did not load it through the forwarder and the rule proxy)"
elif [ -n "$silo_dns" ] || [ "${cross:-0}" != 0 ]; then
  kit_outcome flags-leak "${where}with #384's flags:${silo_dns:+ looked up on the Mac: $silo_dns}$([ "${cross:-0}" != 0 ] && echo " $cross requests reached the other computer's preview")"
elif [ "$WATCH" = 0 ]; then
  kit_outcome inconclusive "${where}the page ran and reached no other computer, but the Mac's lookups were not watched (--allow-sudo)"
else
  kit_outcome flags-hold "${where}with #384's flags no secret name was looked up and nothing reached the other computer; without --host-resolver-rules: ${ctl_dns:-nothing looked up either}"
fi
