#!/bin/bash
# id: 14
# title: What provisioning leaves in a new computer
# tier: vm
# time: about 15 minutes
# needs: the installed computer of test 00, macOS 27 on the Mac, network (softwareupdate asks Apple)
# step: 2
# decides: the checks before main is called ready (research 10, step 8), first-run-time-macos, file-changes-from-outside (FileVault off)
# question: After Apple's provisioning, is FileVault off, does the user hold a secure token, is SSH served, is the login keychain unlocked in an SSH session, and what do auto-login, sleep, the screen lock and softwareupdate look like?
# outcome ready-checks-hold: FileVault is off, the user has a secure token and owns the volume, sshd
#   runs, and the login keychain is unlocked and writable over SSH: the checks of step 8 can be written
#   as they are, and an agent signed in over SSH may write the keychain (see test 02).
# outcome keychain-locked-over-ssh: Everything else holds, but the login keychain is locked (or not
#   writable) in an SSH session: step 8's "keychain unlocked over SSH" check fails by design; Silo
#   either unlocks it (security unlock-keychain with the password it keeps) or expects logins in
#   files, as Claude Code's documentation says it falls back to.
# outcome setup-differs: FileVault is on, the user has no secure token, or sshd is not served as
#   expected: the facts say which; step 5 to 8 of research 10 change before the first build.
# outcome inconclusive: The checks could not run; read the attached log.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"
. "$KIT_ROOT/lib/guest.sh"

if guest_local; then
  kit_note "Linux run: the macOS commands are replaced by canned outputs from linux/fixtures/14-checks.txt"
else
  kit_need_mac 27
  kit_need_silovm; kit_need_base_vm
  kit_confirm "This starts one macOS computer with its screen in a window on your Mac (a clone of the installed one, deleted at the end), for about 15 minutes."
fi
trap guest_cleanup EXIT
if guest_local; then guest_up leaves; else guest_up leaves --gui; fi
guest_local || kit_measure seconds_to_ssh "$GUEST_SSH_SECONDS" s "first start with provisioning"

out="$KIT_WORK/checks.txt"
pw="$(vm_password)"
# Each check prints "=== name" then its output and its exit status. Run as root where macOS needs
# it; the keychain checks run as the user, in the SSH session, which is what an agent sees.
USER_CHECKS="$GUEST_LIMIT"'u=$(id -un)
c() { n="$1"; shift; echo "=== $n"; "$@" 2>&1; echo "--- status $?"; }
c sw_vers sw_vers
c fdesetup fdesetup status
c securetoken sysadminctl -secureTokenStatus "$u"
c apfs_users diskutil apfs listUsers /
c keychain_list security list-keychains
c keychain_default security default-keychain
c keychain_info limit 20 security show-keychain-info "$HOME/Library/Keychains/login.keychain-db"
c keychain_write limit 20 security add-generic-password -U -s silo-kit-probe -a "$u" -w probe
c keychain_read limit 20 security find-generic-password -s silo-kit-probe
security delete-generic-password -s silo-kit-probe >/dev/null 2>&1
c screenlock sysadminctl -screenLock status -password "$KIT_PW"
c pmset pmset -g
c admin dseditgroup -o checkmember -m "$u" admin
c sudo_nopass sudo -n true
c spotlight mdutil -s /
c gatekeeper spctl --status
c sip csrutil status
c setup_assistant pgrep -lx "Setup Assistant"
c login_session who
'
ROOT_CHECKS="$GUEST_LIMIT"'c() { n="$1"; shift; echo "=== $n"; "$@" 2>&1; echo "--- status $?"; }
c sshd launchctl print system/com.openssh.sshd
c remotelogin systemsetup -getremotelogin
c autologin defaults read /Library/Preferences/com.apple.loginwindow autoLoginUser
c kcpassword ls -l /etc/kcpassword
c setup_done ls -l /private/var/db/.AppleSetupDone
c softwareupdate limit 240 softwareupdate -l
'

if guest_local; then
  cp "$KIT_ROOT/linux/fixtures/14-checks.txt" "$out"
else
  # Give the first login a minute to show whatever it shows, then keep a picture of the screen.
  sleep 60
  "$SILOVM" ctl --bundle "$GB" screenshot "$KIT_WORK/first-login.png" >/dev/null 2>&1 &&
    kit_attach "$KIT_WORK/first-login.png"
  {
    guest_sh "KIT_PW='$pw'; $USER_CHECKS"
    guest_sudo "$ROOT_CHECKS"
  } > "$out" 2>&1
fi
sed "s/$pw/<password>/g" "$out" > "$out.kept" && kit_attach "$out.kept"

# section NAME: the output of one check; status NAME: its exit status.
section() { awk -v n="=== $1" '$0 == n {on = 1; next} /^=== / {on = 0} /^--- status/ {on = 0} on' "$out"; }
status() { awk -v n="=== $1" '$0 == n {on = 1; next} on && /^--- status/ {print $3; exit}' "$out"; }

grep -q '^=== sw_vers' "$out" || kit_outcome inconclusive "No check ran; see checks.txt.kept."
grep -q '^=== sw_vers' "$out" || exit 0

kit_fact guest_macos "$(section sw_vers | awk '/ProductVersion/ {print $2}') $(section sw_vers | awk '/BuildVersion/ {print $2}')"
fv="$(section fdesetup | head -1)"; kit_fact filevault "$fv"
tok="$(section securetoken | grep -io 'secure token is [a-z]*' | head -1)"; kit_fact secure_token "$tok"
owner="$(section apfs_users | grep -c 'Volume Owner: Yes')"; kit_fact apfs_volume_owners "$owner"
kit_fact keychain_info "$(section keychain_info | head -1 | cut -c1-200) (status $(status keychain_info))"
kit_fact keychain_write_over_ssh "status $(status keychain_write): $(section keychain_write | head -1 | cut -c1-160)"
kit_fact keychain_read_over_ssh "status $(status keychain_read)"
kit_fact screen_lock "$(section screenlock | head -2 | tr '\n' ' ' | cut -c1-200)"
kit_fact pmset_sleep "$(section pmset | grep -E '^ *(sleep|displaysleep|disksleep|standby|powernap) ' | awk '{print $1"="$2}' | tr '\n' ' ')"
kit_fact user_is_admin "status $(status admin)"
kit_fact sudo_without_password "status $(status sudo_nopass)"
kit_fact spotlight "$(section spotlight | tail -1 | cut -c1-120)"
kit_fact gatekeeper "$(section gatekeeper | head -1)"
kit_fact sip "$(section sip | head -1 | cut -c1-120)"
kit_fact setup_assistant_running "$(section setup_assistant | head -1) (status $(status setup_assistant))"
kit_fact sshd_launchctl "status $(status sshd): $(section sshd | grep -m1 'state =' | sed "s/^[[:space:]]*//")"
kit_fact remote_login "$(section remotelogin | head -1 | cut -c1-160)"
kit_fact auto_login_user "$(section autologin | head -1) (status $(status autologin))"
kit_fact kcpassword "status $(status kcpassword)"
kit_fact setup_done_marker "status $(status setup_done)"
kit_fact softwareupdate "$(section softwareupdate | grep -v '^Software Update Tool' | grep -v '^Finding' | grep -v '^$' | head -6 | tr '\n' ' ' | cut -c1-400)"

problems=""
echo "$fv" | grep -q 'FileVault is Off' || problems="$problems FileVault: $fv;"
echo "$tok" | grep -qi 'enabled' || problems="$problems secure token: ${tok:-none};"
[ "$(status sshd)" = 0 ] || problems="$problems sshd not in launchctl;"
kc_ok=1
[ "$(status keychain_write)" = 0 ] || kc_ok=0

summary="FileVault: $fv. Token: ${tok:-none}. Volume owners: $owner. sshd status $(status sshd). Keychain write over SSH: status $(status keychain_write). Auto-login: $(section autologin | head -1). Sleep: $(section pmset | awk '$1 == "sleep" {print $2}')."
if [ -n "$problems" ]; then
  kit_outcome setup-differs "Not as research 10 expects:$problems $summary"
elif [ "$kc_ok" = 0 ]; then
  kit_outcome keychain-locked-over-ssh "The login keychain refused a write over SSH. $summary"
else
  kit_outcome ready-checks-hold "$summary"
fi
