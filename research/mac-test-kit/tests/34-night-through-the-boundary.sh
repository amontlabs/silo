#!/bin/bash
# id: 34
# title: A night of Claude Code and Codex through a boundary that reads them, against an hour without it
# tier: long
# time: about 10 hours: 8 through the boundary (KIT_34_HOURS), 1 without it (KIT_34_CONTROL_HOURS), and the set-up with you at the keyboard for the sign-ins (about 10 minutes)
# needs: the installed computer of test 00, macOS 27 on the Mac, network, Claude Code signed in with a Claude plan (Max) and Codex with a ChatGPT plan (either one is enough), you at the keyboard at the start, the Mac on power and left awake (the test holds a caffeinate assertion); test 21's outcome, when it ran, chooses how the programs trust the boundary
# step: 6
# decides: model-traffic-read, recognising-a-conversation, provider-sees-silos-connection, reading-each-agent-program (the night it leaves to find out)
# blocks: option A of the first build (#373): its step 6 is done when a night through the boundary is clean, and this is that night with a stand-in for Silo's boundary
# question: Signed in with their plans in a macOS computer and left to work for a whole night through a boundary on the Mac that ends TLS and reads every call, are both programs' conversations recognised whole, are the plan's headers read, is every reply that is not a normal one kept as a fact, does no sign-in reach a file, and do the programs fare as well as in an hour without the boundary?
# outcome night-clean: Every model call of each program carried the one conversation id the program
#   itself named, through its resumes (none without an id, none with two), the plan's headers were in
#   the replies (anthropic-ratelimit-unified-*; Codex's x-codex-* or its rate-limit events), every reply
#   that was not normal is a fact, no sign-in value is in any file, and the programs' own errors and
#   retries were no worse than without the boundary. model-traffic-read and recognising-a-conversation
#   (joined by the program's session) hold on a real night of plan traffic; provider-sees-silos-connection:
#   the providers treated the boundary's connection as the program's own; option A's step 6 has its
#   night, to run again with Silo's own boundary in place of this stand-in.
# outcome conversations-split: Some model calls carried no conversation id, or two that disagree, or
#   the traffic named a conversation the program never named, or one it named never showed; the facts
#   say which, for which program and how often. recognising-a-conversation's join by session does not
#   hold as it stands for that program, and option A's receipts per conversation would be wrong there:
#   the reader is fixed (a header or field this test did not know) before step 6 is called done.
# outcome headers-missing: Conversations were whole, but one program's replies carried no plan headers
#   (no anthropic-ratelimit-unified-* for Claude Code, no x-codex-* header and no rate-limit event for
#   Codex): option A shows that plan's share as not known, and spend at API prices only (#351);
#   model-traffic-read reads the share from somewhere else or drops it.
# outcome differs-from-control: Through the boundary the programs' own errors and retries were clearly
#   more frequent than without it (over twice the control's rate an hour plus 2, or a quarter more
#   failed launches), a provider answered with a challenge, or a program refused the boundary's
#   certificate. provider-sees-silos-connection is in question: the facts say what differed; before
#   option A ships, find whether it is the boundary's connection (TLS, HTTP/1.1 only, this test's SSH
#   hop) or the hour, with a second night.
# outcome credential-leaked: A sign-in value of the computer (~/.claude/.credentials.json, the keychain
#   item, ~/.codex/auth.json) or a secret the boundary saw in a request was found in a file this run
#   wrote; the facts name the files. Do not pack or share these results until those files are deleted.
#   The stand-in's redaction failed, and Silo's boundary must pass the same search before step 6 is done.
# outcome inconclusive: Neither program was signed in, the computer's requests never reached the
#   boundary, no model call went through it, or the control did not run; the sentence says which.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

BOUNDARY="$KIT_ROOT/py/34-boundary.py"
W="$KIT_WORK"
RES="$(dirname "$KIT_OUT")"
READ_HOSTS="api.anthropic.com chatgpt.com api.openai.com"
if guest_local; then
  HOURS="${KIT_34_HOURS:-0.0015}"; CONTROL="${KIT_34_CONTROL_HOURS:-0.0015}"
  LAUNCH_S=30; PAUSE=1; POLL=1
  kit_note "Linux run: the boundary between stand-ins for Claude Code and Codex (linux/34-fake-agent.py) and fake providers (linux/34-fake-providers.py), the guest's night script as written, phases of seconds"
else
  HOURS="${KIT_34_HOURS:-8}"; CONTROL="${KIT_34_CONTROL_HOURS:-1}"
  LAUNCH_S=$(( ${KIT_34_LAUNCH_MINUTES:-40} * 60 )); PAUSE=60; POLL=600
  kit_need_mac 27
  kit_need_silovm; kit_need_base_vm
  kit_need_cmd openssl "it ships with macOS"
  kit_need_cmd ssh "it ships with macOS"
  . "$KIT_ROOT/lib/ios.sh"
  kit_confirm "This starts one macOS computer for about $(python3 -c "print(round($HOURS + $CONTROL + 0.5, 1))") hours, installs Claude Code and Codex in it and asks you to sign in to them, then leaves both working on their own: $HOURS hours through a boundary on this Mac (127.0.0.1) that reads their traffic, then $CONTROL hours without it. It spends your Claude plan's and your ChatGPT plan's usage for the whole night, as two agents working without a break would: expect to reach their 5-hour limits and to use a real share of the weekly ones, which you will then not have for your own work. Traffic goes only to Anthropic and OpenAI; the Mac stays awake meanwhile; the computer is deleted at the end."
fi
NIGHT_S="$(python3 -c "print(int(float('$HOURS') * 3600))")"
CONTROL_S="$(python3 -c "print(int(float('$CONTROL') * 3600))")"
kit_fact phases "night $HOURS h through the boundary, control $CONTROL h without it, launches of at most $LAUNCH_S s"

BPID=""; TUN=""; FPID=""; CAF=""
trap 'touch "$W/tunnel.stop" 2>/dev/null; kill $BPID $TUN $FPID $CAF 2>/dev/null; guest_cleanup' EXIT
rm -rf "$W/boundary" "$W/guest" "$W/tls" "$W/tunnel.stop" "$W/tunnel.restarts"
mkdir -p "$W/boundary" "$W/guest"

# 1. The boundary's authority and certificate (the providers' names), and the boundary on this Mac.
guest_make_test_ca "$W/tls" || kit_fail "openssl could not make the test certificates"
UP_ARGS=""
if guest_local; then
  python3 -I "$KIT_ROOT/linux/34-fake-providers.py" "$W/fake.port" "$W/tls/chain.crt" "$W/tls/leaf.key" > "$W/fake.err" 2>&1 &
  FPID=$!
  i=0; while [ ! -s "$W/fake.port" ] && [ "$i" -lt 20 ]; do sleep 0.5; i=$((i + 1)); done
  FPORT="$(cat "$W/fake.port")" || kit_fail "the fake providers did not start"
  at=""; for h in $READ_HOSTS; do at="$at${at:+,}$h=127.0.0.1:$FPORT"; done
  UP_ARGS="--upstream-ca $W/tls/ca.crt --upstream-at $at"
fi
# shellcheck disable=SC2086
python3 -I "$BOUNDARY" serve --port-file "$W/boundary.port" --cert "$W/tls/chain.crt" --key "$W/tls/leaf.key" \
  --dir "$W/boundary" --max-seconds "$((NIGHT_S + CONTROL_S + 4 * 3600))" $UP_ARGS > "$W/boundary.err" 2>&1 &
BPID=$!
i=0; while [ ! -s "$W/boundary.port" ] && [ "$i" -lt 40 ]; do sleep 0.5; i=$((i + 1)); done
[ -s "$W/boundary.port" ] || { kit_attach "$W/boundary.err"; kit_fail "the boundary did not start"; }
BPORT="$(cat "$W/boundary.port")"
guest_local || { caffeinate -i -w $$ & CAF=$!; }

# 2. The computer, the two programs, the sign-ins.
guest_up night
guest_local || kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "first start with provisioning"
if guest_local; then
  mkdir -p "$GUEST_HOME/.local/bin" "$GUEST_HOME/.claude" "$GUEST_HOME/.codex"
  for p in claude codex; do
    printf '#!/bin/sh\nexec python3 -I "%s" %s "$@"\n' "$KIT_ROOT/linux/34-fake-agent.py" "$p" > "$GUEST_HOME/.local/bin/$p"
    chmod +x "$GUEST_HOME/.local/bin/$p"
  done
  # Planted sign-ins: linux/test-34.sh checks that none of these values reaches a file.
  python3 - "$GUEST_HOME" <<'EOF'
import base64, json, os, secrets, sys
h = sys.argv[1]
t = lambda p: p + secrets.token_urlsafe(24)
jwt = lambda: ".".join(base64.urlsafe_b64encode(x).rstrip(b"=").decode() for x in
                       (b'{"alg":"RS256"}', json.dumps({"sub": t("planted-"), "exp": 1}).encode(), os.urandom(48)))
json.dump({"claudeAiOauth": {"accessToken": t("sk-ant-oat01-PLANTED"), "refreshToken": t("sk-ant-ort01-PLANTED"),
                              "subscriptionType": "max"}, "silokitCookie": t("PLANTEDCOOKIE")},
          open(os.path.join(h, ".claude/.credentials.json"), "w"))
json.dump({"tokens": {"access_token": jwt(), "id_token": jwt(), "refresh_token": t("rt_PLANTED"),
                      "account_id": t("acct-PLANTED")}}, open(os.path.join(h, ".codex/auth.json"), "w"))
EOF
fi
CLAUDE_V="$(guest_install_claude)"; CODEX_V="$(guest_install_codex)"
kit_fact claude_version "${CLAUDE_V:-not installed}"
kit_fact codex_version "${CODEX_V:-not installed}"
if ! guest_local; then
  cmd="$(guest_ssh_command -L 1455:localhost:1455)" || kit_fail "no address for the computer"
  kit_human "Open a new Terminal window on your Mac and paste this line (it reaches the computer as $VM_USER with the kit's own key, and forwards port 1455 for Codex's sign-in page):

  $cmd

In that window:
  1. Claude Code: run   claude auth login   and sign in with your Claude plan (Max) in your Mac's browser.
  2. Codex: run   codex login   and sign in with ChatGPT; if the page does not come back, run
     codex login --device-auth   instead.
  3. Type   exit.
Skip a program you have no plan for. The night starts when you press Enter here."
fi
signed() { # PROG: yes when the program says it is signed in
  local s
  if guest_local; then echo yes; return; fi
  case "$1" in
    claude) s="$(guest_sh "$GUEST_LIMIT limit 30 claude auth status --text 2>&1")" ;;
    codex) s="$(guest_sh "$GUEST_LIMIT limit 30 codex login status 2>&1")" ;;
  esac
  if [ -n "$s" ] && ! echo "$s" | grep -qiE 'not logged|not signed|no credentials|error'; then echo yes; else echo no; fi
}
CL_IN="$(signed claude)"; CX_IN="$(signed codex)"
kit_fact signed_in "claude=$CL_IN codex=$CX_IN"
PROGS=""; [ "$CL_IN" = yes ] && PROGS="claude"; [ "$CX_IN" = yes ] && PROGS="$PROGS codex"
[ -n "$PROGS" ] || { kit_outcome inconclusive "Neither Claude Code nor Codex was signed in; nothing ran."; exit 0; }

# 3. Trust: the way test 21 found, if it ran. The keychain when it was enough; otherwise each
# program's own setting. Codex's setting replaces its authorities, so it gets ours and the system's.
t21="$(python3 - "$KIT_WORK_ROOT/results" <<'EOF'
import glob, json, os, sys
best = (0, "none")
for p in glob.glob(os.path.join(sys.argv[1], "*", "results.jsonl")):
    for line in open(p, errors="replace"):
        try: e = json.loads(line)
        except ValueError: continue
        if e.get("test") == "21" and e.get("kind") == "outcome" and e.get("t", 0) > best[0]:
            best = (e["t"], e["key"])
print(best[1])
EOF
)"
kit_fact test21_outcome "$t21"
guest_sh 'mkdir -p "$HOME/silokit/tls" "$HOME/silokit/34"' || kit_fail "could not make the computer's folders"
for f in ca.crt leaf.crt; do guest_put "$W/tls/$f" "silokit/tls/$f" || kit_fail "could not copy $f into the computer"; done
guest_put "$KIT_ROOT/guest/34-night.sh" "silokit/34-night.sh" || kit_fail "could not copy 34-night.sh into the computer"
guest_sh 'cat "$HOME/silokit/tls/ca.crt" /etc/ssl/cert.pem > "$HOME/silokit/tls/bundle.pem" 2>/dev/null; true'
NEED_CL_ENV=yes; NEED_CX_ENV=yes; KC=no
case "$t21" in
  both-trust-keychain) KC=yes; NEED_CL_ENV=no; NEED_CX_ENV=no ;;
  claude-only-keychain) KC=yes; NEED_CL_ENV=no ;;
  codex-only-keychain) KC=yes; NEED_CX_ENV=no ;;
esac
if [ "$KC" = yes ] && ! guest_local; then
  way="$(ios_trust_ca "/Users/$VM_USER/silokit/tls/ca.crt" "/Users/$VM_USER/silokit/tls/leaf.crt" api.anthropic.com "$W/trust.log")"
  kit_attach "$W/trust.log"
  kit_fact keychain_trust_installed_by "$way"
  [ "$way" = none ] && { NEED_CL_ENV=yes; NEED_CX_ENV=yes; KC=no; }
fi
ENV="DISABLE_AUTOUPDATER=1"
[ "$NEED_CL_ENV" = yes ] && ENV="$ENV NODE_EXTRA_CA_CERTS=\$HOME/silokit/tls/ca.crt"
[ "$NEED_CX_ENV" = yes ] && ENV="$ENV CODEX_CA_CERTIFICATE=\$HOME/silokit/tls/bundle.pem"
kit_fact trust_way "keychain=$KC; launch environment: $ENV"

# 4. The route: the providers' names point at 127.0.0.1 in the computer, where a relay run as root
# hands port 443 to an SSH reverse forward to the boundary. The tunnel is opened again if it drops.
if guest_local; then
  NIGHT_AT="SILOKIT_34_AT=127.0.0.1:$BPORT"; CONTROL_AT="SILOKIT_34_AT=127.0.0.1:$FPORT"
else
  NIGHT_AT=""; CONTROL_AT=""
  ip="$(vm_ip "$GB" 30)" || kit_fail "no address for the computer"
  (
    while [ ! -f "$W/tunnel.stop" ]; do
      # shellcheck disable=SC2046
      ssh $(_vm_ssh_opts) -o BatchMode=yes -o ExitOnForwardFailure=yes -i "$(_vm_key)" -N \
        -R "127.0.0.1:18443:127.0.0.1:$BPORT" "$VM_USER@$ip" < /dev/null >> "$W/tunnel.log" 2>&1
      [ -f "$W/tunnel.stop" ] || date +%s >> "$W/tunnel.restarts"
      sleep 5
    done
  ) &
  TUN=$!
  guest_put "$KIT_ROOT/guest/34-relay.pl" "silokit/34-relay.pl" || kit_fail "could not copy the relay into the computer"
  hosts=""
  for h in $READ_HOSTS; do hosts="$hosts
127.0.0.1 $h # silokit-34
::1 $h # silokit-34"; done
  guest_sudo "cp /etc/hosts /etc/hosts.silokit-34 && printf '%s\n' '$hosts' >> /etc/hosts && dscacheutil -flushcache; killall -HUP mDNSResponder 2>/dev/null
nohup /usr/bin/perl /Users/$VM_USER/silokit/34-relay.pl 18443 > /var/tmp/silokit-34-relay.log 2>&1 < /dev/null &
sleep 2; true" > /dev/null 2>&1
  sleep 3
  path_check="$(guest_sh "curl -s -o /dev/null -w '%{http_code}' --cacert \$HOME/silokit/tls/ca.crt --max-time 30 https://api.anthropic.com/silokit-34-path-check")"
  kit_fact path_check "${path_check:-nothing} (a status from Anthropic through the boundary)"
  case "$path_check" in
    ""|000) kit_attach "$W/tunnel.log"; kit_outcome inconclusive "The computer's requests to api.anthropic.com did not reach the boundary (curl said ${path_check:-nothing})."; exit 0 ;;
  esac
fi

# run_phase PHASE SECONDS "env": both programs work, each in its own loop in the computer, until
# the time is up; then whatever still runs is ended.
run_phase() {
  local phase="$1" secs="$2" envs="$3" p t_end left n
  guest_sh 'rm -f "$HOME/silokit/34/stop"'
  for p in $PROGS; do
    guest_sh "cd \$HOME/silokit && nohup env $ENV $envs sh \$HOME/silokit/34-night.sh $p $phase $secs $LAUNCH_S $PAUSE > \$HOME/silokit/34/$phase-$p.log 2>&1 < /dev/null &" ||
      kit_note "the $phase loop of $p did not start"
  done
  t_end=$(( $(date +%s) + secs ))
  while [ "$(date +%s)" -lt "$t_end" ]; do
    left=$(( t_end - $(date +%s) )); [ "$left" -gt "$POLL" ] && left="$POLL"
    sleep "$left"
    if ! guest_local && ! vm_running "$GB"; then kit_note "the computer stopped during the $phase"; return 1; fi
    n="$(wc -l < "$W/boundary/calls.jsonl" 2>/dev/null | tr -d ' ')"
    guest_local || kit_log "$phase: $(( (t_end - $(date +%s)) / 60 )) minutes left; ${n:-0} calls through the boundary so far"
  done
  guest_sh 'touch "$HOME/silokit/34/stop"'
  n=0
  while [ "$n" -lt $((LAUNCH_S + 120)) ]; do
    [ "$(guest_sh "ls \$HOME/silokit/34/$phase/*/finished 2>/dev/null | wc -l" | tr -d ' ')" -ge "$(echo "$PROGS" | wc -w | tr -d ' ')" ] && return 0
    sleep 2; n=$((n + 2))
  done
  kit_note "a $phase loop did not end by itself; ended"
  guest_sh 'pkill -f 34-night.sh; pkill -x claude; pkill -x codex; true'
}

# 5. The night, through the boundary.
NIGHT_START="$(date +%s)"
run_phase night "$NIGHT_S" "$NIGHT_AT"
NIGHT_END="$(date +%s)"
kit_measure night_hours "$(python3 -c "print(round(($NIGHT_END - $NIGHT_START) / 3600.0, 3))")" h "through the boundary"
[ -s "$W/tunnel.restarts" ] && kit_fact tunnel_restarts "$(wc -l < "$W/tunnel.restarts" | tr -d ' ') (the SSH tunnel of this test dropped and was opened again)"

# 6. The control: the route taken out, the same programs and settings, new conversations.
if ! guest_local; then
  guest_sudo "pkill -f 34-relay.pl; [ -f /etc/hosts.silokit-34 ] && mv /etc/hosts.silokit-34 /etc/hosts; dscacheutil -flushcache; killall -HUP mDNSResponder 2>/dev/null; true" > /dev/null 2>&1
  before="$(wc -l < "$W/boundary/calls.jsonl" 2>/dev/null | tr -d ' ')"
  direct="$(guest_sh "curl -s -o /dev/null -w '%{http_code}' --max-time 30 https://api.anthropic.com/silokit-34-path-check")"
  after="$(wc -l < "$W/boundary/calls.jsonl" 2>/dev/null | tr -d ' ')"
  kit_fact control_route "direct curl said ${direct:-nothing}; calls through the boundary before $before, after $after (should not change)"
fi
if [ "$CONTROL_S" -gt 0 ]; then
  run_phase control "$CONTROL_S" "$CONTROL_AT"
else
  kit_note "no control: KIT_34_CONTROL_HOURS is 0"
fi

# 7. What the programs and the boundary saw.
guest_sh 'tar -C "$HOME/silokit/34" --exclude "*-project" -cf - .' | tar -C "$W/guest" -xf - ||
  kit_note "could not copy the programs' own logs out of the computer"
python3 -I "$BOUNDARY" analyze --calls "$W/boundary/calls.jsonl" --conns "$W/boundary/conns.jsonl" --guest "$W/guest" \
  --night-hours "$HOURS" --control-hours "$CONTROL" --until "$NIGHT_END" > "$W/analysis.tsv" 2> "$W/analysis.err" ||
  { kit_attach "$W/analysis.err"; kit_fail "the analysis failed"; }
VERDICT=""
while IFS="$(printf '\t')" read -r k v; do
  if [ "$k" = verdict ]; then VERDICT="$v"; else kit_fact "$k" "$v"; fi
done < "$W/analysis.tsv"
for f in calls conns; do [ -f "$W/boundary/$f.jsonl" ] && kit_attach "$W/boundary/$f.jsonl"; done
kit_attach "$W/analysis.tsv"
for ph in night control; do for p in claude codex; do
  [ -f "$W/guest/$ph/$p/launches.tsv" ] && cp "$W/guest/$ph/$p/launches.tsv" "$W/launches-$ph-$p.tsv" && kit_attach "$W/launches-$ph-$p.tsv"
done; done
v() { python3 -c 'import json,sys
x = json.loads(sys.argv[1]).get(sys.argv[2])
print("; ".join(x) if isinstance(x, list) else x)' "$VERDICT" "$1"; }

# 8. No sign-in in any file: the computer's sign-ins (read over SSH into memory, never written) and
# every secret the boundary held, searched for in this test's work folder and the run's results.
skip=""; guest_local && skip="-$GUEST_HOME"
printf '%s\n' "$W" "$RES" ${skip:+"$skip"} > "$W/boundary/scan-request.part" && mv "$W/boundary/scan-request.part" "$W/boundary/scan-request"
i=0; while [ ! -s "$W/boundary/scan.json" ] && [ "$i" -lt 600 ]; do sleep 1; i=$((i + 1)); done
SCAN_B="$(cat "$W/boundary/scan.json" 2>/dev/null)"; NONE='{}'
# shellcheck disable=SC2016
SCAN_G="$(guest_sh "$GUEST_LIMIT"'cat "$HOME/.claude/.credentials.json" 2>/dev/null; echo; echo ---silokit---; cat "$HOME/.codex/auth.json" 2>/dev/null; echo; echo ---silokit---; command -v security >/dev/null 2>&1 && limit 20 security find-generic-password -s "Claude Code-credentials" -w 2>/dev/null; true' |
  python3 -I "$BOUNDARY" scan "$W" "$RES" ${skip:+"$skip"})"
kit_fact secrets_search "boundary's own: ${SCAN_B:-no answer}; the computer's sign-ins: ${SCAN_G:-no answer}"
leaks="$(python3 -c 'import json,sys
out = []
for s in sys.argv[1:]:
    try: out += json.loads(s).get("found", [])
    except ValueError: pass
print(" ".join(sorted(set(out))))' "${SCAN_B:-$NONE}" "${SCAN_G:-$NONE}")"
kit_fact boundary_redactions "$(python3 -c 'import json,sys
try: print(json.loads(sys.argv[1]).get("redactions"))
except ValueError: print("unknown")' "${SCAN_B:-$NONE}")"

# 9. The outcome.
summary="$(v summary)"
if [ -n "$leaks" ]; then
  kit_outcome credential-leaked "A sign-in value was found in: $leaks. $summary"
elif [ -z "$SCAN_B" ] || [ -z "$SCAN_G" ]; then
  kit_outcome inconclusive "The search for sign-in values did not answer (boundary: ${SCAN_B:-none}; computer: ${SCAN_G:-none}). $summary"
elif [ "$(v model_calls)" = 0 ]; then
  kit_outcome inconclusive "No model call went through the boundary (programs: $PROGS). $summary"
elif [ "$CONTROL_S" -le 0 ] || [ "$(v control_ran)" != "$(v night_ran)" ]; then
  kit_outcome inconclusive "The control did not run for every program that worked at night (night: $(v night_ran); control: $(v control_ran)). $summary"
elif [ -n "$(v differs)" ]; then
  kit_outcome differs-from-control "$(v differs). $summary"
elif [ -n "$(v split)" ]; then
  kit_outcome conversations-split "$(v split). $summary"
elif [ -n "$(v headers_missing)" ]; then
  kit_outcome headers-missing "$(v headers_missing). $summary"
else
  kit_outcome night-clean "Programs: $PROGS, trust: keychain=$KC, environment: $(echo "$ENV" | sed 's/DISABLE_AUTOUPDATER=1//; s/^ *//'). $summary"
fi
