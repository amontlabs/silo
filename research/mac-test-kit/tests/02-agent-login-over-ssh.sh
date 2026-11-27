#!/bin/bash
# id: 02
# title: Claude Code and Codex signed in over SSH: reconnect, restart, checkpoint, fork
# tier: guided
# time: about 45 minutes, with you at the keyboard for the sign-ins (about 5 minutes)
# needs: the installed computer of test 00, macOS 27 on the Mac, network, a Claude subscription or Console account and a ChatGPT account (either one is enough)
# step: 2
# decides: fork-and-sign-in (where each login lands), runs-outlive-the-terminal (a login used from a session left running after its terminal closed), whether the first build's "ssh main, start Claude Code, go to bed" holds, agent-programs-logins-kept-out-of-copies
# question: Signed in over SSH inside a macOS computer, where does each program keep its login (a file or the keychain), and does the login still work after a reconnect, in a tmux or screen session that outlives its SSH connection, after a restart of the computer, in a copy restored from a checkpoint and in a fork with a new identity?
# outcome lasts-in-files: Both logins landed in files (~/.claude/.credentials.json, ~/.codex/auth.json)
#   and worked after reconnecting, after a restart, in the checkpoint copy and in the fork: "start
#   Claude Code, go to bed" holds over SSH; fork-and-sign-in can keep a login out of a fork by leaving
#   those files out, as it says.
# outcome lasts-in-keychain: The logins kept working, but at least one landed in the login keychain:
#   a fork copies it and Silo cannot leave one keychain item out (research 06, cross-cutting 1);
#   fork-and-sign-in must say, for macOS computers, that this login is copied.
# outcome lost-somewhere: A login that worked right after signing in failed later; the facts name the
#   first stage that failed for each program (reconnect, a session left running in tmux or screen
#   after its SSH connection closed, restart, checkpoint, fork). Before the first
#   build, the story "go to bed" changes, or Silo signs agents in another way (claude setup-token).
# outcome inconclusive: No login worked right after signing in, or the computer did not come back;
#   read the facts and the attached checks.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

if guest_local; then
  kit_note "Linux run: stand-ins for claude and codex (linux/fake-agents), a fake computer, no restart"
  CALL=1
else
  kit_need_mac 27
  kit_need_silovm; kit_need_base_vm
  kit_need_cmd ssh "it ships with macOS"
  kit_confirm "This starts a macOS computer, installs Claude Code and Codex in it (about 300 MB), asks you to sign in to them over SSH, then restarts and copies that computer twice (all deleted at the end). About 45 minutes."
  CALL=0
  [ "$(kit_ask_yn "Check each login with a real one-word request (about 8 small requests on your accounts)? Otherwise only the programs' status commands are used.")" = yes ] && CALL=1
fi
kit_fact real_requests "$CALL"

trap guest_cleanup EXIT
guest_up login
guest_local || kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "first start with provisioning"
ORIG="$GB"

if guest_local; then
  mkdir -p "$GUEST_HOME/.local/bin"
  cp "$KIT_ROOT/linux/fake-agents/claude" "$KIT_ROOT/linux/fake-agents/codex" "$GUEST_HOME/.local/bin/"
fi
kit_fact claude_version "$(guest_install_claude)"
kit_fact codex_version "$(guest_install_codex)"
kit_fact session_keepers "$(guest_sh 'for t in tmux screen; do command -v $t >/dev/null 2>&1 && printf "%s " "$t"; done; echo')"

# What a check prints, one key=value per line. Never a secret: only whether files and keychain
# items exist, the status commands' first lines, and the last line of each request's answer.
CHECK="$GUEST_LIMIT"'
cf="$HOME/.claude/.credentials.json"; af="$HOME/.codex/auth.json"
mode() { ls -ld "$1" | cut -c1-10; }
if [ -f "$cf" ]; then echo "claude_file=yes"; echo "claude_file_mode=$(mode "$cf")"; else echo "claude_file=no"; fi
if [ -f "$af" ]; then echo "codex_file=yes"; echo "codex_file_mode=$(mode "$af")"; else echo "codex_file=no"; fi
if command -v security >/dev/null 2>&1; then
  limit 20 security find-generic-password -s "Claude Code-credentials" >/dev/null 2>&1; echo "claude_keychain_item=$?"
  d=$(limit 30 security dump-keychain 2>&1)
  echo "keychain_claude_lines=$(echo "$d" | grep -ci claude)"
  echo "keychain_codex_lines=$(echo "$d" | grep -ciE "codex|openai")"
  echo "keychain_dump_error=$(echo "$d" | grep -i "security:" | head -1)"
fi
s=$(limit 30 claude auth status --text 2>&1); echo "claude_status_rc=$?"
echo "$s" | grep -v "^ *$" | head -3 | sed "s/^/claude_status=/"
s=$(limit 30 codex login status 2>&1); echo "codex_status_rc=$?"
echo "$s" | grep -v "^ *$" | head -2 | sed "s/^/codex_status=/"
if [ "$CALL" = 1 ]; then
  cd /tmp || exit 1
  o=$(limit 150 claude -p "Reply with the single word ok." 2>&1); echo "claude_call_rc=$?"
  echo "$o" | grep -v "^ *$" | tail -1 | sed "s/^/claude_call=/"
  o=$(limit 240 codex exec --skip-git-repo-check "Reply with the single word ok." 2>&1); echo "codex_call_rc=$?"
  echo "$o" | grep -v "^ *$" | tail -1 | sed "s/^/codex_call=/"
fi'

# check STAGE [OUTPUT]: run the check in the current computer (or read OUTPUT, a check that already
# ran), keep it, record facts. Sets CL_OK and CX_OK (yes, no) for the stage.
check() {
  local f="$KIT_WORK/check-$1.txt"
  if [ -n "${2:-}" ]; then guest_redact < "$2" > "$f"; else guest_sh "CALL=$CALL; $CHECK" 2>&1 | guest_redact > "$f"; fi
  kit_attach "$f"
  v() { sed -n "s/^$1=//p" "$f" | head -1; }
  kit_fact "$1_claude" "file=$(v claude_file) mode=$(v claude_file_mode) keychain_item_rc=$(v claude_keychain_item) keychain_lines=$(v keychain_claude_lines) status_rc=$(v claude_status_rc) status=$(v claude_status | cut -c1-80) call_rc=$(v claude_call_rc) call=$(v claude_call | cut -c1-80)"
  kit_fact "$1_codex" "file=$(v codex_file) mode=$(v codex_file_mode) keychain_lines=$(v keychain_codex_lines) status_rc=$(v codex_status_rc) status=$(v codex_status | cut -c1-80) call_rc=$(v codex_call_rc) call=$(v codex_call | cut -c1-80)"
  if [ "$CALL" = 1 ]; then
    CL_OK=no; [ "$(v claude_call_rc)" = 0 ] && v claude_call | grep -qi ok && CL_OK=yes
    CX_OK=no; [ "$(v codex_call_rc)" = 0 ] && v codex_call | grep -qi ok && CX_OK=yes
  else
    CL_OK=no; [ "$(v claude_status_rc)" = 0 ] && ! v claude_status | grep -qi 'not logged' && CL_OK=yes
    CX_OK=no; [ "$(v codex_status_rc)" = 0 ] && ! v codex_status | grep -qi 'not logged' && CX_OK=yes
  fi
  CL_FILE="$(v claude_file)"; CX_FILE="$(v codex_file)"
  CL_KC=no; [ "$(v claude_keychain_item)" = 0 ] && CL_KC=yes
  CX_KC=no; [ "${CX_FILE}" != yes ] && [ "$(v keychain_codex_lines)" -gt 0 ] 2>/dev/null && CX_KC=yes
  kit_fact "$1_works" "claude=$CL_OK codex=$CX_OK"
}

check before-login
kit_fact before_login_works "claude=$CL_OK codex=$CX_OK (both should be no)"

# --- The sign-ins, by the person, in their own SSH session.
if guest_local; then
  mkdir -p "$GUEST_HOME/.claude" "$GUEST_HOME/.codex"
  echo '{}' > "$GUEST_HOME/.claude/.credentials.json"; chmod 600 "$GUEST_HOME/.claude/.credentials.json"
  echo '{}' > "$GUEST_HOME/.codex/auth.json"; chmod 600 "$GUEST_HOME/.codex/auth.json"
else
  cmd="$(guest_ssh_command -L 1455:localhost:1455)" || kit_fail "no address for the computer"
  kit_human "Open a new Terminal window on your Mac and paste this line (it reaches the computer as $VM_USER, with the kit's own key; it forwards port 1455 for Codex's sign-in page):

  $cmd

In that window:
  1. Claude Code: run   claude auth login
     (or run claude and type /login). Open the link it shows in your Mac's browser, sign in, and
     paste the code back if it asks. Wait for \"Login successful\".
  2. Codex: run   codex login
     Open the link in your Mac's browser; the forwarded port brings the answer back. If that fails,
     run   codex login --device-auth   (device codes must be allowed in your ChatGPT security settings).
  3. Type   exit   to disconnect.
Skip a program you have no account for. Then come back here."
fi

# --- 1. Reconnect: the person's session is closed; this is a new SSH connection.
check after-reconnect
CL_IN="$CL_OK"; CX_IN="$CX_OK"
where() { # FILE KEYCHAIN: "file", "keychain", "file keychain" or "none"
  local w=""
  [ "$1" = yes ] && w="file"
  [ "$2" = yes ] && w="$w keychain"
  w="${w# }"; echo "${w:-none}"
}
loc_cl="$(where "$CL_FILE" "$CL_KC")"; loc_cx="$(where "$CX_FILE" "$CX_KC")"
kit_fact claude_login_location "$loc_cl"
kit_fact codex_login_location "$loc_cx"
if [ "$CL_IN" != yes ] && [ "$CX_IN" != yes ]; then
  kit_outcome inconclusive "Neither login worked right after signing in (Claude Code: $loc_cl, Codex: $loc_cx); see check-after-reconnect.txt."
  exit 0
fi
lost_cl=""; lost_cx=""
note_stage() { # STAGE: record the first stage each signed-in program fails at
  [ "$CL_IN" = yes ] && [ "$CL_OK" != yes ] && [ -z "$lost_cl" ] && lost_cl="$1"
  [ "$CX_IN" = yes ] && [ "$CX_OK" != yes ] && [ -z "$lost_cx" ] && lost_cx="$1"
  return 0
}

# --- 1b. A session that outlives its terminal: the first build's "ssh main, start Claude Code, go to
# bed", where #370 has `ssh main.keep` open tmux. The check starts in tmux or screen, the SSH
# connection that started it closes at once, and it runs on alone; its output is read afterwards.
KEEPER="$(guest_sh 'for t in tmux screen; do command -v $t >/dev/null 2>&1 && { echo $t; break; }; done')"
kit_fact detached_keeper "${KEEPER:-none}"
if [ -z "$KEEPER" ]; then
  kit_note "neither tmux nor screen is in the computer: the detached stage did not run"
else
  printf '%s\n' "CALL=$CALL; $CHECK" > "$KIT_WORK/detached-check.sh"
  guest_put "$KIT_WORK/detached-check.sh" silokit-detached.sh
  run='sh "$HOME/silokit-detached.sh" > "$HOME/silokit-detached.txt" 2>&1; touch "$HOME/silokit-detached.done"'
  guest_sh "rm -f \"\$HOME/silokit-detached.done\"; if [ $KEEPER = tmux ]; then tmux new-session -d -s silokit02 $(printf '%q' "$run"); else screen -dmS silokit02 sh -c $(printf '%q' "$run"); fi"
  i=0
  while ! guest_sh 'test -f "$HOME/silokit-detached.done"' 2>/dev/null && [ "$i" -lt 60 ]; do sleep 10; i=$((i + 1)); done
  if guest_sh 'test -f "$HOME/silokit-detached.done"' 2>/dev/null; then
    guest_get silokit-detached.txt "$KIT_WORK/detached-raw.txt"
    check detached "$KIT_WORK/detached-raw.txt"
    note_stage detached
  else
    kit_note "the check in $KEEPER did not end within 10 minutes"; CL_OK=no; CX_OK=no; note_stage detached
  fi
  guest_sh "rm -f \"\$HOME/silokit-detached.sh\" \"\$HOME/silokit-detached.txt\" \"\$HOME/silokit-detached.done\"" || true
fi

# --- 2. Restart the computer from inside.
how="$(guest_reboot)" || kit_note "the computer did not come back within 10 minutes of its restart"
kit_fact restart_mode "$how"
check after-restart
note_stage restart

# --- 3. A checkpoint: stop, clone the bundle, start the clone (same identity: a restore).
kit_fact stop_before_checkpoint "$(vm_stop "$ORIG" 60)"
guest_clone "$ORIG" "$KIT_WORK/checkpoint.silovm" || kit_fail "clone for the checkpoint failed"
if guest_start "$KIT_WORK/checkpoint.silovm"; then
  check in-checkpoint
  note_stage checkpoint
  vm_stop "$GB" 60 >/dev/null
else
  kit_note "the checkpoint copy did not answer SSH"; CL_OK=no; CX_OK=no; note_stage checkpoint-no-ssh
fi

# --- 4. A fork: a clone with a new machine identifier, auxiliary storage and MAC address.
guest_clone "$ORIG" "$KIT_WORK/fork.silovm" --new-identity || kit_fail "clone for the fork failed"
if guest_start "$KIT_WORK/fork.silovm"; then
  check in-fork
  note_stage fork
  vm_stop "$GB" 60 >/dev/null
else
  kit_note "the fork did not answer SSH"; CL_OK=no; CX_OK=no; note_stage fork-no-ssh
fi

kit_fact claude_first_failure "${lost_cl:-none} (signed in: $CL_IN)"
kit_fact codex_first_failure "${lost_cx:-none} (signed in: $CX_IN)"
text="Claude Code (signed in: $CL_IN, login in: $loc_cl, first failure: ${lost_cl:-none}); Codex (signed in: $CX_IN, login in: $loc_cx, first failure: ${lost_cx:-none}); restart: $how; checked with real requests: $CALL."
if [ -n "$lost_cl$lost_cx" ]; then
  kit_outcome lost-somewhere "$text"
elif echo "$loc_cl $loc_cx" | grep -q keychain; then
  kit_outcome lasts-in-keychain "$text"
else
  kit_outcome lasts-in-files "$text"
fi
