#!/bin/bash
# id: 15
# title: A real macOS computer under agent-like work, a checkpoint per step
# tier: long
# time: 2 to 3 hours (KIT_15_STEPS shortens it)
# needs: the base computer of test 00, 120 GB free, a network for the guest (GitHub, npm, nodejs.org), no sudo on the Mac
# step: 4
# decides: checkpoint-per-system, speed-budgets (the checkpoint's target as the wait inside), restore-to-a-checkpoint, what-a-restore-leaves, research 07 "What to do if APFS behaves like XFS"
# question: How fast does a real macOS computer's disk image fragment under agent-like work with a checkpoint per step, how long does a write inside wait during a checkpoint, and does a checkpoint from mid-run boot clean with its repositories intact?
# outcome holds: The restored checkpoint booted clean, git and npm worked in it, and no flushed write inside waited more than 0.1 s during a checkpoint: checkpoints per step are fine as they are; the target can be written at the pieces reached.
# outcome waits-small: Restore was clean; the longest write wait inside during a checkpoint was between 0.1 and 1 s: write the checkpoint's target as that wait at the pieces reached, and count pieces (research 07 step 1).
# outcome waits-grow: Restore was clean but a write inside waited more than 1 s during a checkpoint, growing with the pieces: research 07's mitigations are needed (make the disk whole when main stops, then a layer per boot).
# outcome restore-broken: The mid-run checkpoint did not boot, its volume did not verify, git showed changed tracked files or npm ls failed in it: restore-to-a-checkpoint cannot rest on a clone of a running disk as taken here; look at the facts, then test 04's modes.
# outcome inconclusive: The work or the computer failed before enough steps; look at 15-steps.csv.
. "$KIT_ROOT/lib/common.sh"
. "$KIT_ROOT/lib/vm.sh"

BIN="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin"
STEPS="${KIT_15_STEPS:-24}"
KEEP="${KIT_15_KEEP:-12}"
REPOS="${KIT_15_REPOS:-expressjs/express sveltejs/svelte}"
NODE_LINE="${KIT_15_NODE:-latest-v22.x}"
CSV="$KIT_WORK/15-steps.csv"
echo "step,work_s,work_ok,extents,clone_s,wait_full_s,wait_fsync_s,mac_free_gb" > "$CSV"

if [ "${KIT_FAKE:-0}" = 1 ] || ! kit_is_mac; then
  # Linux analogue: the guest's work script and the write-wait probe run here for two steps.
  command -v perl >/dev/null 2>&1 && command -v sqlite3 >/dev/null 2>&1 || kit_skip "needs perl and sqlite3"
  H="$KIT_WORK/home"; mkdir -p "$H/sk"
  HOME="$H" perl "$KIT_ROOT/guest/writewait.pl" "$H/sk/ww.bin" "$H/sk/ww-full.log" full 0.05 0.001 &
  P=$!
  trap 'kill $P 2>/dev/null; rm -rf "$KIT_WORK/home"' EXIT
  for k in 1 2 3; do
    t="$(kit_now)"
    HOME="$H" sh "$KIT_ROOT/guest/agentwork.sh" "$k" || kit_fail "agentwork.sh failed at step $k"
    kit_measure "step${k}_work_s" "$(python3 -c "print('%.2f' % ($(kit_now) - $t))")" s "no repositories in the analogue"
  done
  sleep 0.5
  [ -s "$H/sk/ww-full.log.max" ] || kit_fail "the probe wrote nothing"
  kit_measure longest_probe_wait_s "$(cat "$H/sk/ww-full.log.max")" s "the probe beside the work, no checkpoint"
  [ -d "$H/work/s3" ] && [ ! -d "$H/work/s1" ] || kit_fail "the work did not keep two steps"
  kit_outcome inconclusive "Linux analogue: the work script and the probe ran for 3 steps; nothing measured of a Mac"
  exit 0
fi

kit_need_mac 14
kit_need_silovm
kit_need_base_vm
kit_need_free_gb 120
[ -x "$BIN/clonebench" ] && [ -x "$BIN/extents" ] || kit_skip "the kit's programs are not built: run ./kit build"
kit_confirm "Test 15 starts a computer that downloads Node.js and the Command Line Tools inside it, then runs $STEPS steps of work with a checkpoint after each, 2 to 3 hours."

B="$(vm_fresh t15)" || exit 1  # vm_fresh already recorded why
C="$KIT_WORK/c"; mkdir -p "$C"
R="$KIT_WORK/restore.silovm"
trap 'vm_destroy "$R" 2>/dev/null; vm_destroy "$B"; rm -rf "$C" "$KIT_WORK/flat.img"' EXIT
# shellcheck disable=SC2046  # the options are words on purpose
vm_start "$B" $(vm_provision_args) >/dev/null
s="$(vm_ready "$B" "$VM_USER" 900)" || { kit_attach "$B/run.log"; kit_fail "the computer did not answer SSH"; }
kit_measure first_start_to_ssh_s "$s" s
gsh() { vm_ssh "$B" "$VM_USER" "$@"; }
jget() { python3 -c 'import json,sys; print(json.loads(sys.argv[1]).get(sys.argv[2], ""))' "$1" "$2"; }

# Tools inside: Node.js from nodejs.org, and the Command Line Tools for git (Homebrew's way).
gsh "cd ~ && f=\$(curl -fsSL https://nodejs.org/dist/$NODE_LINE/SHASUMS256.txt | awk '/darwin-arm64.tar.gz/{print \$2; exit}') && curl -fsSLO https://nodejs.org/dist/$NODE_LINE/\$f && mkdir -p node && tar -xzf \$f -C node --strip-components 1 && rm -f \$f && ~/node/bin/node --version" > "$KIT_WORK/node.txt" 2>&1 \
  || { kit_attach "$KIT_WORK/node.txt"; kit_fail "Node.js could not be installed inside"; }
kit_fact guest_node "$(tail -1 "$KIT_WORK/node.txt")"
vm_sudo "$B" "$VM_USER" "touch /tmp/.com.apple.dt.CommandLineTools.installondemand.in-progress; l=\$(softwareupdate -l 2>/dev/null | grep -B1 -E 'Command Line Tools' | awk -F'*' '/^ *\\*/ {print \$2}' | sed -e 's/^ *Label: //' -e 's/^ *//' | sort | tail -n1); [ -n \"\$l\" ] && softwareupdate -i \"\$l\" >/dev/null 2>&1; rm -f /tmp/.com.apple.dt.CommandLineTools.installondemand.in-progress" > "$KIT_WORK/clt.txt" 2>&1 || true
if gsh 'git --version' > "$KIT_WORK/git.txt" 2>&1; then kit_fact guest_git "$(cat "$KIT_WORK/git.txt")"
else kit_fact guest_git "none (Command Line Tools not installed: repositories come as tarballs, git status is not checked)"; fi

gsh 'mkdir -p ~/sk && cat > ~/sk/agentwork.sh' < "$KIT_ROOT/guest/agentwork.sh"
gsh 'cat > ~/sk/writewait.pl' < "$KIT_ROOT/guest/writewait.pl"
gsh '(nohup perl ~/sk/writewait.pl ~/sk/ww-full.bin ~/sk/ww-full.log full 0.1 0.05 </dev/null >/dev/null 2>&1 &); (nohup perl ~/sk/writewait.pl ~/sk/ww-fsync.bin ~/sk/ww-fsync.log fsync 0.1 0.05 </dev/null >/dev/null 2>&1 &)'

guest_time() { gsh "perl -MTime::HiRes=time -e 'printf(q(%.3f), time)'"; }
ck_bundle() { # ck_bundle DST: the checkpoint: the disk and the auxiliary storage, cloned
  rm -rf "$1"; mkdir -p "$1"
  local o; o="$("$BIN/clonebench" clone "$B/disk.img" "$1/disk.img")" || return 1
  for f in aux.img hardware-model.bin machine-id.bin config.json; do cp -c "$B/$f" "$1/$f" || return 1; done
  jget "$o" seconds
}

k=0; okn=0
while [ "$k" -lt "$STEPS" ]; do
  k=$((k + 1))
  t="$(kit_now)"
  # shellcheck disable=SC2086
  if gsh "sh ~/sk/agentwork.sh $k $REPOS" > "$KIT_WORK/work-$k.log" 2>&1; then wok=1; okn=$((okn + 1)); else wok=0; kit_attach "$KIT_WORK/work-$k.log"; fi
  wt="$(python3 -c "print('%.1f' % ($(kit_now) - $t))")"
  ext="$(jget "$("$BIN/extents" "$B/disk.img")" extents)"
  g0="$(guest_time)"
  cs="$(ck_bundle "$C/ck$k.silovm")" || kit_fail "checkpoint $k failed"
  g1="$(guest_time)"
  sleep 1
  wf="$(gsh "awk -v a=$g0 -v b=$g1 '\$1 >= a - 0.5 && \$1 <= b + 0.5 { if (\$2 > m) m = \$2 } END { print m + 0 }' ~/sk/ww-full.log 2>/dev/null")"
  wy="$(gsh "awk -v a=$g0 -v b=$g1 '\$1 >= a - 0.5 && \$1 <= b + 0.5 { if (\$2 > m) m = \$2 } END { print m + 0 }' ~/sk/ww-fsync.log 2>/dev/null")"
  free="$(df -g "$KIT_WORK" | awk 'NR==2 {print $4}')"
  echo "$k,$wt,$wok,$ext,$cs,${wf:-},${wy:-},$free" >> "$CSV"
  kit_log "step $k: work ${wt}s ok=$wok, $ext pieces, checkpoint ${cs}s, longest wait inside ${wf}s (F_FULLFSYNC) ${wy}s (fsync)"
  [ "$k" -gt "$KEEP" ] && rm -rf "$C/ck$((k - KEEP)).silovm"
  vm_running "$B" || kit_fail "the computer stopped at step $k"
done
kit_attach "$CSV"
kit_measure longest_wait_outside_checkpoints_full_s "$(gsh 'cat ~/sk/ww-full.log.max' 2>/dev/null || echo 0)" s "longest F_FULLFSYNC probe write over the whole run"
python3 - "$CSV" <<'EOF' > "$KIT_WORK/15-sum.txt"
import csv, sys
rows = list(csv.DictReader(open(sys.argv[1])))
f = lambda k: [float(r[k]) for r in rows if r[k] not in ("", None)]
print("final_extents", f("extents")[-1] if rows else 0, "pieces")
print("max_extents", max(f("extents") or [0]), "pieces")
print("max_clone_s", max(f("clone_s") or [0]), "s")
print("max_wait_full_s", max(f("wait_full_s") or [0]), "s")
print("max_wait_fsync_s", max(f("wait_fsync_s") or [0]), "s")
print("steps_ok", sum(1 for r in rows if r["work_ok"] == "1"), "steps")
EOF
while read -r n v u; do kit_measure "$n" "$v" "$u"; done < "$KIT_WORK/15-sum.txt"

# Restore a mid-run checkpoint into its own bundle, boot it, check it.
mid=$((STEPS - KEEP / 2)); [ "$mid" -lt 1 ] && mid=1
vm_stop "$B" 60 >/dev/null
rm -rf "$R"; cp -cR "$C/ck$mid.silovm" "$R" || kit_fail "copy of checkpoint $mid failed"
t="$(kit_now)"; vm_start "$R" >/dev/null
booted=no; vv=""; gs=""; nl=""
if vm_wait_ssh "$R" "$VM_USER" 900 >/dev/null; then
  booted=yes
  kit_measure restore_boot_to_ssh_s "$(python3 -c "print('%.1f' % ($(kit_now) - $t))")" s "checkpoint $mid"
  vv="$(vm_sudo "$R" "$VM_USER" 'diskutil verifyVolume / 2>&1 | tail -2' 2>&1 | tr '\n' ' ')"
  first="$(echo "$REPOS" | awk '{print $1}')"; name="$(basename "$first")"
  gs="$(vm_ssh_pw "$R" "$VM_USER" "cd ~/work/s$mid/$name && if git --version >/dev/null 2>&1; then git status --short --untracked-files=no | wc -l | tr -d ' '; else echo no-git; fi" 2>&1 | tail -1)"
  nl="$(vm_ssh_pw "$R" "$VM_USER" "cd ~/work/s$mid/$name && PATH=\$HOME/node/bin:\$PATH npm ls --depth=0 >/dev/null 2>&1 && echo ok || echo failed" 2>&1 | tail -1)"
fi
vm_stop "$R" 60 >/dev/null
kit_fact restore_booted "$booted"
kit_fact restore_verify_volume "$vv"
kit_fact restore_git_status_changed_files "$gs"
kit_fact restore_npm_ls "$nl"

# Making the disk whole (research 07, step 2), on the stopped disk: time and pieces after.
if python3 -c "import os,sys; s=os.statvfs(sys.argv[1]); sys.exit(0 if s.f_bavail*s.f_frsize > 1.3*os.stat(sys.argv[2]).st_blocks*512 else 1)" "$KIT_WORK" "$B/disk.img"; then
  t="$(kit_now)"
  python3 - "$B/disk.img" "$KIT_WORK/flat.img" <<'EOF'
import fcntl, os, sys
i = os.open(sys.argv[1], os.O_RDONLY); o = os.open(sys.argv[2], os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
fcntl.fcntl(o, 48, 1)  # F_NOCACHE
size = os.fstat(i).st_size; os.ftruncate(o, size); z = bytes(1 << 20)
for off in range(0, size, 1 << 20):
    b = os.pread(i, 1 << 20, off)
    if b != z[:len(b)]: os.pwrite(o, b, off)
os.fsync(o); os.close(i); os.close(o)
EOF
  kit_measure flatten_s "$(python3 -c "print('%.1f' % ($(kit_now) - $t))")" s "full copy of the stopped disk, holes kept"
  kit_measure flat_extents "$(jget "$("$BIN/extents" "$KIT_WORK/flat.img")" extents)" pieces
  kit_measure flat_clone_s "$(jget "$("$BIN/clonebench" clone "$KIT_WORK/flat.img" "$KIT_WORK/flat-ck.img")" seconds)" s
  rm -f "$KIT_WORK/flat.img" "$KIT_WORK/flat-ck.img"
else
  kit_note "not enough free space to time a full copy of the disk"
fi

res="$(python3 - "$KIT_WORK/15-sum.txt" "$booted" "$vv" "$gs" "$nl" "$STEPS" <<'EOF'
import sys
m = {l.split()[0]: float(l.split()[1]) for l in open(sys.argv[1])}
booted, vv, gs, nl, steps = sys.argv[2:7]
w = max(m.get("max_wait_full_s", 0), m.get("max_wait_fsync_s", 0))
s = ("%d of %s steps ran; the disk reached %d pieces (final %d); the longest checkpoint %.2f s; the longest flushed write inside during a checkpoint %.3f s; "
     "restored mid-run checkpoint: booted %s, verifyVolume [%s], git status changed files %s, npm ls %s"
     % (m.get("steps_ok", 0), steps, m.get("max_extents", 0), m.get("final_extents", 0), m.get("max_clone_s", 0), w, booted, vv.strip()[-80:], gs, nl))
ok_restore = booted == "yes" and ("appears to be OK" in vv or "exit code is 0" in vv) and nl == "ok" and gs in ("0", "no-git")
if m.get("steps_ok", 0) < max(3, int(steps) // 2): print("inconclusive|" + s)
elif not ok_restore: print("restore-broken|" + s)
elif w <= 0.1: print("holds|" + s)
elif w <= 1.0: print("waits-small|" + s)
else: print("waits-grow|" + s)
EOF
)"
kit_outcome "${res%%|*}" "${res#*|}"
