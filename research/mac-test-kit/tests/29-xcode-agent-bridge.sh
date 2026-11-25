#!/bin/bash
# id: 29
# title: Xcode's agent bridge used over SSH, and who answers its approval
# tier: guided
# time: about 25 minutes, with you at the computer's window for one or two clicks
# needs: the Xcode computer of test 23 with Xcode 26.3 or later, macOS 27 on the Mac
# decides: computers-macos-screen (whether main's screen must carry Xcode for agents), agents-drive-the-screen, xcode-in-a-computer (a step Silo's setup takes)
# question: With Xcode open on the computer's screen, can an agent reached over SSH use `xcrun mcpbridge` (Xcode's tools for outside agents), and when Xcode asks to allow it, can a script inside the computer answer, or only a person, and is the answer kept for the next agent process?
# outcome works-no-prompt: mcpbridge listed Xcode's tools over SSH with nothing to approve: agents
#   in main use Xcode's tools as soon as Xcode is open on main's screen.
# outcome script-answers-prompt: Xcode asked, and a script inside the computer (System Events,
#   over SSH) clicked Allow: Silo's setup, or an agent driving the screen, can answer the prompt
#   with nobody at the Mac.
# outcome approved-once: Xcode asked, a script could not answer, a person's Allow worked, and the
#   next agent process was not asked again: one click per computer, which Silo's setup can ask for
#   once, on the computer's screen.
# outcome prompt-every-process: Xcode asked again for each new agent process, and only a person
#   could answer: on main this prompt is on main's screen, not the Mac's, but agents still need
#   someone (or an agent driving the screen) at every start.
# outcome no-bridge: mcpbridge is missing or listed no tools even with a person at the screen; the
#   facts hold its errors (an older Xcode, or Xcode not running).
# outcome inconclusive: Xcode did not open on the computer's screen, or nobody answered.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"
. "$KIT_ROOT/lib/ios.sh"

W="$KIT_WORK"
# An MCP client's first three messages: initialize, initialized, tools/list (id 2).
cat > "$W/mcp-in.jsonl" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"silo-kit","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
EOF

if guest_local; then
  ios_fake || kit_skip "needs a Mac: Xcode only runs on macOS"
  kit_note "Linux run: the reader of mcpbridge's answers, on canned output"
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18"}}' \
    '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"BuildProject"},{"name":"RunTests"},{"name":"XcodeRead"}]}}' > "$W/ok.out"
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{}}' > "$W/waiting.out"
  printf '%s\n' 'mcpbridge: starting' '{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"Xcode is not running"}}' > "$W/error.out"
  a="$(ios_mcp_tools "$W/ok.out")"; b="$(ios_mcp_tools "$W/waiting.out")"; c="$(ios_mcp_tools "$W/error.out")"
  kit_measure linux_parsers_ok "$( [ "$a" = 3 ] && [ "$b" = none ] && [ "$c" = "error Xcode is not running" ] && echo 1 || echo 0)" bool "tools listed, no answer, an error"
  kit_outcome inconclusive "Linux run: only the reader of mcpbridge's answers ran; the bridge needs Xcode."
  exit 0
fi

kit_need_mac 27
kit_need_silovm
ios_need_xcode_vm
kit_confirm "This starts a clone of the Xcode computer with its screen in a window for about 25 minutes, opens Xcode on that screen and asks you to click in it once or twice."
trap guest_cleanup EXIT
ios_up bridge --gui
kit_fact computer_xcode "$(guest_sh 'xcodebuild -version' 2>/dev/null | tr '\n' ' ')"
if ! guest_sh 'xcrun --find mcpbridge' > "$W/find.txt" 2>&1; then
  kit_attach "$W/find.txt"
  kit_outcome no-bridge "xcrun --find mcpbridge found nothing: $(tail -1 "$W/find.txt" | cut -c1-160)"
  exit 0
fi
guest_put "$W/mcp-in.jsonl" "silokit/mcp-in.jsonl" || kit_fail "could not copy the MCP messages into the computer"

console="$(ios_console_user)"
if [ "$console" = none ]; then
  console="$(ios_screen_login --gui)" || { kit_outcome inconclusive "the computer did not come back after the restart for automatic login"; exit 0; }
fi
kit_fact screen_user "$console"
[ "$console" != none ] || { kit_outcome inconclusive "nobody is logged in on the computer's screen, even after automatic login"; exit 0; }

# Xcode on the user's screen, opened from SSH: first as the user, then into the user's GUI
# session from root (launchctl asuser).
opened=none
guest_sh 'open -a Xcode' >/dev/null 2>&1
ios_wait 30
if guest_sh 'pgrep -x Xcode' >/dev/null 2>&1; then opened=open-over-ssh; else
  guest_sudo "launchctl asuser \$(id -u $VM_USER) sudo -u $VM_USER open -a Xcode" >/dev/null 2>&1
  ios_wait 30
  guest_sh 'pgrep -x Xcode' >/dev/null 2>&1 && opened=launchctl-asuser
fi
kit_fact xcode_opened_by "$opened"
shot() { "$SILOVM" ctl --bundle "$GB" screenshot "$W/$1.png" >/dev/null 2>&1 && kit_attach "$W/$1.png"; }
shot xcode-open
[ "$opened" != none ] || { kit_outcome inconclusive "Xcode did not open on the computer's screen from SSH"; exit 0; }

# call LABEL SECONDS: one new mcpbridge process, as an agent over SSH starts it. Prints the number
# of tools it listed, "none" or "error ...". Its stderr is kept.
call() {
  guest_sh "$GUEST_LIMIT cd \$HOME/silokit && (cat mcp-in.jsonl; sleep $2) | limit $(($2 + 5)) xcrun mcpbridge > mcp-$1.out 2> mcp-$1.err" >/dev/null 2>&1
  guest_get "silokit/mcp-$1.out" "$W/mcp-$1.out" 2>/dev/null
  guest_get "silokit/mcp-$1.err" "$W/mcp-$1.err" 2>/dev/null
  [ -s "$W/mcp-$1.err" ] && kit_attach "$W/mcp-$1.err"
  ios_mcp_tools "$W/mcp-$1.out"
}
is_n() { case "$1" in ''|*[!0-9]*) return 1 ;; *) [ "$1" -gt 0 ] ;; esac; }

# 1. Nobody at the screen.
call first 40 > "$W/r-first" & p=$!
sleep 15; shot during-first; wait "$p"
r1="$(cat "$W/r-first")"; kit_fact first_process "$r1"
if is_n "$r1"; then
  r2="$(call second 30)"; kit_fact second_process "$r2"
  kit_outcome works-no-prompt "mcpbridge listed $r1 tools over SSH with nothing to approve; a second process: $r2"
  exit 0
fi

# 2. A script inside the computer answers: System Events clicks the frontmost window's Allow.
call script 60 > "$W/r-script" & p=$!
sleep 15
guest_sh "osascript -e 'tell application \"System Events\" to tell (first process whose frontmost is true) to click (first button of window 1 whose name starts with \"Allow\")' 2>&1" > "$W/osascript.txt" 2>&1
kit_attach "$W/osascript.txt"
wait "$p"
r_script="$(cat "$W/r-script")"; kit_fact script_answered "$r_script; osascript: $(tail -1 "$W/osascript.txt" | cut -c1-160)"
if is_n "$r_script"; then
  kit_outcome script-answers-prompt "a click by System Events from SSH let mcpbridge list $r_script tools"
  exit 0
fi

# 3. A person answers.
call person 240 > "$W/r-person" & p=$!
sleep 10; shot during-person
kit_human "In the computer's window on your Mac, Xcode (or macOS) asks whether to allow an agent to use Xcode.
  Click Allow. If Xcode asks you to turn on access for external agents in its Settings (Intelligence), turn it on.
  If nothing is shown, look at Xcode's window anyway, then press Enter."
wait "$p"
r_person="$(cat "$W/r-person")"; kit_fact person_answered "$r_person"
if ! is_n "$r_person"; then
  kit_outcome no-bridge "mcpbridge listed no tools even with a person at the screen: first $r1, script $r_script, person $r_person; stderr: $(tail -1 "$W/mcp-person.err" 2>/dev/null | cut -c1-160)"
  exit 0
fi

# 4. The next agent process, nobody at the screen: is the approval kept?
r_after="$(call after 40)"; kit_fact next_process "$r_after"
shot after
if is_n "$r_after"; then
  kit_outcome approved-once "a person's Allow let mcpbridge list $r_person tools, and the next process listed $r_after with nobody asked"
else
  kit_outcome prompt-every-process "a person's Allow worked ($r_person tools), the next process got $r_after: each new agent process is asked again"
fi
