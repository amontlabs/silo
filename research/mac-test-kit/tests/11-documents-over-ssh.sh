#!/bin/bash
# id: 11
# title: Documents, Desktop and Downloads over SSH
# tier: vm
# time: about 10 minutes
# needs: the installed computer of test 00, macOS 27 on the Mac
# step: 2
# decides: computer-reached-over-ssh, unmodified-agent-programs (where agents' work can live over SSH)
# question: In a provisioned computer reached over SSH, can a command list and write ~/Documents, ~/Desktop and ~/Downloads, or does macOS's privacy protection (TCC) refuse it?
# outcome open: Every folder can be listed and written over SSH, as the user and as root: agents can work
#   anywhere in the home folder; the first run needs to say nothing about these folders.
# outcome refused: The protected folders are refused over SSH ("Operation not permitted") while the
#   home folder itself works: agents reached over SSH cannot work in Documents, Desktop or Downloads
#   unless "Allow full disk access for remote users" is turned on inside the computer. Silo either
#   turns it on during setup (it is a switch under Remote Login, see test 22) or keeps work elsewhere
#   (~/work) and the first run says so.
# outcome mixed: Some folders or some actions are refused and others not; the facts name which. Read
#   them before choosing where work lives.
# outcome inconclusive: Even the home folder could not be written, or SSH failed; read the attached log.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

# The probe, run in the computer: for each folder, list it and write then delete a file.
# Prints lines "folder action ok|refused|error message".
PROBE='for d in "$HOME" "$HOME/Documents" "$HOME/Desktop" "$HOME/Downloads"; do
  n="${d#$HOME}"; n="${n#/}"; [ -n "$n" ] || n=home
  if out=$(ls "$d" 2>&1 >/dev/null); then echo "$n list ok"; else echo "$n list refused $out"; fi
  f="$d/.silokit-probe-$$"
  if out=$( (echo probe > "$f") 2>&1); then echo "$n write ok"; rm -f "$f"; else echo "$n write refused $out"; fi
done'

if guest_local; then
  kit_note "Linux run: the probe runs on a folder of the work folder, where nothing is protected"
  mkdir -p "$GUEST_HOME/Documents" "$GUEST_HOME/Desktop" "$GUEST_HOME/Downloads"
else
  kit_need_mac 27
  kit_need_silovm; kit_need_base_vm
  kit_confirm "This starts one macOS computer (a clone of the installed one, deleted at the end) for about 10 minutes."
fi
trap guest_cleanup EXIT
guest_up docs
guest_local || kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "first start with provisioning"

log="$KIT_WORK/probe.txt"
{
  echo "# as the user, over SSH"
  guest_sh "$PROBE"
  echo "# as root (sudo), over SSH"
  root_home="/Users/$VM_USER"; guest_local && root_home="$GUEST_HOME"
  guest_sudo "HOME=$root_home; $PROBE"
} > "$log" 2>&1
kit_attach "$log"

ok=0; refused=0
while read -r who; do
  case "$who" in
    "#"*) continue ;;
  esac
  # shellcheck disable=SC2086
  set -- $who
  [ $# -ge 3 ] || continue
  case "$3" in ok) ok=$((ok + 1)) ;; *) refused=$((refused + 1)) ;; esac
done < "$log"

root_facts="$(sed -n '/^# as root/,$p' "$log" | grep -v '^#' | awk '{print $1"_"$2"="$3}' | tr '\n' ' ')"
kit_fact as_root "$root_facts"
user_facts="$(sed -n '/^# as the user/,/^# as root/p' "$log" | grep -v '^#' | awk '{print $1"_"$2"="$3}' | tr '\n' ' ')"
kit_fact as_user "$user_facts"
kit_measure probes_ok "$ok" count
kit_measure probes_refused "$refused" count

home_ok="$(grep -c '^home .* ok' "$log")"
protected_refused="$(grep -E '^(Documents|Desktop|Downloads) ' "$log" | grep -vc ' ok$')"
protected_total="$(grep -cE '^(Documents|Desktop|Downloads) ' "$log")"
if [ "$home_ok" -lt 2 ]; then
  kit_outcome inconclusive "The home folder itself was not listed and written over SSH ($home_ok of 4 home probes passed); see probe.txt."
elif [ "$protected_refused" = 0 ]; then
  kit_outcome open "All $protected_total probes of Documents, Desktop and Downloads passed over SSH, as the user and as root. User: $user_facts"
elif [ "$protected_refused" = "$protected_total" ]; then
  kit_outcome refused "All $protected_total probes of Documents, Desktop and Downloads were refused over SSH (as the user and as root), while the home folder works. First refusal: $(grep -m1 refused "$log" | cut -c1-160)"
else
  kit_outcome mixed "$protected_refused of $protected_total probes of the protected folders were refused. User: $user_facts Root: $root_facts"
fi
