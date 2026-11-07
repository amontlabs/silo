#!/usr/bin/env bash
# Phase C1 measurements: crun containers under a hardening baseline on a Linux host. See
# docs/forks-ci/evidence/c1-containers/README.md for the method and the results.
#
#   bash docs/forks-ci/containers/setup.sh && bash docs/forks-ci/containers/run.sh
#
# Environment: SECTIONS (default: facts sec cold ten fork net desktop work), RUNS (3), TEN (10), C1_WORK (/mnt/c1),
# C1_OUT (evidence directory, default $C1_WORK/evidence).
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
SECTIONS=${SECTIONS:-facts sec cold ten fork net desktop work}
RUNS=${RUNS:-3}
TEN=${TEN:-10}
: > "$OUT/results.tsv"
: > "$OUT/summary.txt"
summ() { stats "$@" | tee -a "$OUT/summary.txt"; }

cleanup() {
  for d in "$CTR"/*/; do [ -d "$d" ] && stop_ctr "$(basename "$d")"; done
  sudo pkill -f "http.server 8000" 2>/dev/null
  say "cleanup done"
}
trap cleanup EXIT

section_facts() {
  {
    echo "date: $(date -u +%FT%TZ)"
    echo "runner: $(. /etc/os-release; echo "$PRETTY_NAME"), kernel $(uname -r), $(uname -m)"
    echo "cpu: $(lscpu | awk -F: '/Model name/{gsub(/^ +/,"",$2); print $2}'), $(nproc) cores; memory $(free -m | awk '/Mem:/{print $2}') MiB total, $(free -m | awk '/Mem:/{print $7}') MiB available"
    echo "virtualization: $(systemd-detect-virt 2>&1); /dev/kvm: $(ls /dev/kvm 2>&1)"
    echo "crun: $(crun --version | head -1) | $(crun --version | grep -i features | head -1)"
    echo "podman (image fetch only): $(podman --version)"
    echo "tinyproxy: $(tinyproxy -v 2>&1 | head -1); nft: $(nft --version)"
    echo "landlock: $("$BOOT/silo/init" --probe 2>&1); LSMs: $(cat /sys/kernel/security/lsm 2>&1)"
    echo "cgroup: $(stat -fc %T /sys/fs/cgroup) (cgroup2fs = v2); controllers: $(cat /sys/fs/cgroup/cgroup.controllers)"
    echo "overlay/upper filesystem: $(df -T "$CTR" | awk 'NR==2{print $2}'); overlay in kernel: $(grep -c overlay /proc/filesystems) "
    echo "sysctl apparmor_restrict_unprivileged_userns: $(sysctl -n kernel.apparmor_restrict_unprivileged_userns 2>&1)"
    echo "image: $(python3 -c "import json;i=json.load(open('$REPO/app/SiloUI/guest-image/image-lock.json'))['images']['amd64'];print(i['imageReference'],i['imageDigest'],i['architecture'])")"
    echo "scripts commit: $(git -C "$REPO" rev-parse HEAD 2>/dev/null)"
  } | tee "$RAW/facts.txt"
}

# The baseline from inside, and whether apt works with every capability dropped.
section_sec() {
  CTR_NET=1 CTR_CAPS=none start_ctr sec1 sec || return 1
  wait_exit sec1 60
  cp "$CTR/sec1/out" "$RAW/sec.txt"; cp "$CTR/sec1/err" "$RAW/sec.err"
  cat "$RAW/sec.txt" "$RAW/sec.err" | cut -c1-260
  stop_ctr sec1; sudo rm -rf "$CTR/sec1"
  say "apt as container root with every capability dropped"
  CTR_NET=1 CTR_CAPS=none start_ctr apt1 aptprobe || return 1
  wait_exit apt1 300
  cp "$CTR/apt1/out" "$RAW/aptprobe.txt"; cat "$RAW/aptprobe.txt"
  if grep -q '^APTPROBE install rc=0' "$RAW/aptprobe.txt"; then WORK_CAPS=none; else WORK_CAPS=relaxed; fi
  result apt_with_no_capabilities "$([ "$WORK_CAPS" = none ] && echo works || echo fails)"
  echo "$WORK_CAPS" > "$OUT/work-caps.txt"
  stop_ctr apt1; sudo rm -rf "$CTR/apt1"
}

# (1) Cold start to a ready shell, memory, one container.
section_cold() {
  : > "$RAW/cold-up.txt"; : > "$RAW/cold-ready.txt"; : > "$RAW/cold-pss.txt"
  for i in $(seq "$RUNS"); do
    CTR_NET=1 CTR_HOLD=1 start_ctr cold$i idle || { say "cold$i failed"; continue; }
    read -r up ready < "$CTR/cold$i/timing"
    echo "cold$i $up" >> "$RAW/cold-up.txt"; echo "cold$i $ready" >> "$RAW/cold-ready.txt"
    sleep 5
    pss_of cold$i > "$RAW/cold-mem-$i.txt"; echo "cold$i $(awk '$1=="PSS_MIB"{print $2}' "$RAW/cold-mem-$i.txt")" >> "$RAW/cold-pss.txt"
    stop_ctr cold$i; sudo rm -rf "$CTR/cold$i"
  done
  summ "container: process running in it (UP) from the start command" "$RAW/cold-up.txt"
  summ "container: cold start to a ready shell (network in place)" "$RAW/cold-ready.txt"
  summ "container: PSS of its processes 5 s after ready, idle shell (MiB)" "$RAW/cold-pss.txt" MiB
}

# (1) Ten at once.
section_ten() {
  : > "$RAW/ten-total.txt"; : > "$RAW/ten-pss.txt"; : > "$RAW/ten-pss-each.txt"
  for r in $(seq "$RUNS"); do
    local t0 fail=0; t0=$(now)
    local pids=()
    for k in $(seq "$TEN"); do ( CTR_NET=$k CTR_HOLD=1 start_ctr ten$r-$k idle ) & pids+=($!); done
    wait "${pids[@]}"
    local last=0 count=0
    for k in $(seq "$TEN"); do
      [ -s "$CTR/ten$r-$k/timing" ] || { fail=$((fail+1)); continue; }
      count=$((count+1)); local ts; ts=$(marker_ts "$CTR/ten$r-$k/out" READY)
      last=$(awk -v a="$last" -v b="$ts" 'BEGIN{print (b>a)?b:a}')
    done
    [ "$count" -gt 0 ] && echo "ten$r $(sub "$t0" "$last")" >> "$RAW/ten-total.txt"
    echo "run $r: $count of $TEN ready, $fail failed"
    sleep 5
    local sum=0
    for k in $(seq "$TEN"); do
      [ -s "$CTR/ten$r-$k/timing" ] || continue
      local p; p=$(pss_mib ten$r-$k); echo "ten$r-$k $p" >> "$RAW/ten-pss-each.txt"; sum=$(awk -v a="$sum" -v b="$p" 'BEGIN{print a+b}')
    done
    [ "$count" -gt 0 ] && echo "ten$r $sum" >> "$RAW/ten-pss.txt"
    for k in $(seq "$TEN"); do stop_ctr ten$r-$k; sudo rm -rf "$CTR/ten$r-$k"; done
  done
  summ "container: $TEN started at once, start to the last ready shell" "$RAW/ten-total.txt"
  summ "container: PSS summed over the $TEN, 5 s after the last ready (MiB)" "$RAW/ten-pss.txt" MiB
  summ "container: PSS of each of the ${TEN}x${RUNS} idle containers (MiB)" "$RAW/ten-pss-each.txt" MiB
}

# (1) and (4) Fork: a new container from a disk-only copy of another's upper layer; the manifest diff.
section_fork() {
  CTR_NET=1 CTR_CAPS=none start_ctr forkA fs-a || return 1
  wait_exit forkA 120; cp "$CTR/forkA/out" "$RAW/fork-a.txt"; cat "$RAW/fork-a.txt"
  stop_ctr forkA
  echo "upper layer of A: $(sudo du -sm "$CTR/forkA/upper" | cut -f1) MiB, $(sudo find "$CTR/forkA/upper" | wc -l) entries" | tee "$RAW/fork-a-size.txt"
  : > "$RAW/fork1-copy.txt"; : > "$RAW/fork1-ready.txt"
  for i in $(seq "$RUNS"); do
    local t0 t1; t0=$(now)
    mkdir -p "$CTR/fork1-$i" && in_userns cp -a "$CTR/forkA/upper" "$CTR/fork1-$i/upper"
    t1=$(now); echo "fork1-$i $(sub "$t0" "$t1")" >> "$RAW/fork1-copy.txt"
    CTR_NET=1 CTR_HOLD=1 start_ctr fork1-$i fs-b || { say "fork1-$i failed"; continue; }
    echo "fork1-$i $(sub "$t0" "$(marker_ts "$CTR/fork1-$i/out" READY)")" >> "$RAW/fork1-ready.txt"
    wait_marker "$CTR/fork1-$i/out" MODE_EXIT 60
    [ "$i" = 1 ] && { cp "$CTR/fork1-1/out" "$RAW/fork-b.txt"; cat "$RAW/fork-b.txt"; }
    sleep 3
    [ "$i" = 1 ] && pss_of fork1-1 > "$RAW/fork1-mem.txt"
    if [ "$i" != 1 ]; then stop_ctr fork1-$i; sudo rm -rf "$CTR/fork1-$i"; fi
  done
  summ "container fork: disk-only copy of A's upper layer" "$RAW/fork1-copy.txt"
  summ "container fork: copy to a ready shell in the fork" "$RAW/fork1-ready.txt"
  cat "$RAW/fork1-mem.txt" 2>/dev/null

  # The manifest diffs (fork1-1 is still running; its upper layer is readable as root).
  echo "-- A against the image"; sudo python3 "$C1/diff_upper.py" "$CTR/forkA/upper" --lower "$BOOT" --lower "$BASE" --tsv "$RAW/manifest-a.tsv" | tee "$RAW/manifest-a.txt"
  echo "-- fork against A (what the fork changed)"; sudo python3 "$C1/diff_upper.py" "$CTR/fork1-1/upper" "$CTR/forkA/upper" --lower "$BOOT" --lower "$BASE" --tsv "$RAW/manifest-b.tsv" | tee "$RAW/manifest-b.txt"
  head -n 30 "$RAW/manifest-b.tsv"
  grep -E $'^(D|M)\t' "$RAW/manifest-a.tsv" | head -n 20
  stop_ctr fork1-1; sudo rm -rf "$CTR/fork1-1"

  # Ten forks at once: ten copies in parallel, ten containers.
  : > "$RAW/forkten-total.txt"; : > "$RAW/forkten-copy.txt"; : > "$RAW/forkten-pss.txt"
  for r in $(seq "$RUNS"); do
    local t0 t1; t0=$(now)
    local pids=()
    for k in $(seq "$TEN"); do
      ( mkdir -p "$CTR/fkt$r-$k" && in_userns cp -a "$CTR/forkA/upper" "$CTR/fkt$r-$k/upper" ) & pids+=($!)
    done
    wait "${pids[@]}"
    t1=$(now); echo "fkt$r $(sub "$t0" "$t1")" >> "$RAW/forkten-copy.txt"
    pids=()
    for k in $(seq "$TEN"); do ( CTR_NET=$k CTR_HOLD=1 start_ctr fkt$r-$k fs-b ) & pids+=($!); done
    wait "${pids[@]}"
    local last=0 count=0
    for k in $(seq "$TEN"); do
      [ -s "$CTR/fkt$r-$k/timing" ] || continue
      count=$((count+1)); local ts; ts=$(marker_ts "$CTR/fkt$r-$k/out" READY)
      last=$(awk -v a="$last" -v b="$ts" 'BEGIN{print (b>a)?b:a}')
    done
    [ "$count" -gt 0 ] && echo "fkt$r $(sub "$t0" "$last")" >> "$RAW/forkten-total.txt"
    echo "run $r: $count of $TEN forks ready"
    sleep 5
    local sum=0
    for k in $(seq "$TEN"); do [ -s "$CTR/fkt$r-$k/timing" ] || continue; sum=$(awk -v a="$sum" -v b="$(pss_mib fkt$r-$k)" 'BEGIN{print a+b}'); done
    [ "$count" -gt 0 ] && echo "fkt$r $sum" >> "$RAW/forkten-pss.txt"
    for k in $(seq "$TEN"); do stop_ctr fkt$r-$k; sudo rm -rf "$CTR/fkt$r-$k"; done
  done
  summ "container fork: $TEN disk-only copies in parallel" "$RAW/forkten-copy.txt"
  summ "container fork: $TEN forks at once, start of the copies to the last ready shell" "$RAW/forkten-total.txt"
  summ "container fork: PSS summed over the $TEN forks (MiB)" "$RAW/forkten-pss.txt" MiB
}

# (4) The network path.
section_net() {
  CTR_NET=1 CTR_HOLD=1 CTR_CONNECT=3128,8080 CTR_BIND=8080 start_ctr net1 net || return 1
  wait_marker "$CTR/net1/out" NET_DONE 180
  cp "$CTR/net1/out" "$RAW/net.txt"; cat "$RAW/net.txt"
  sudo nsenter -t "$(cat "$CTR/net1/pid")" -n nft list ruleset > "$RAW/net-container-nft.txt" 2>&1; cat "$RAW/net-container-nft.txt"
  sudo nft list table inet silo_c1 > "$RAW/net-host-nft.txt" 2>&1; cat "$RAW/net-host-nft.txt"
  { echo "-- tinyproxy log (container address 10.200.1.2)"; grep -E '10\.200\.1\.2' "$WORK/tinyproxy.log" | tail -n 15
    echo "-- canary log (the host service; nothing from 10.200.1.2 may appear)"; grep -c '10\.200\.1\.2' "$WORK/canary.log"; } | tee "$RAW/net-proxy-log.txt"
  stop_ctr net1; sudo rm -rf "$CTR/net1"
}

# (2) The desktop under the baseline.
section_desktop() {
  CTR_NET=1 CTR_UID=0 CTR_CAPS=relaxed start_ctr prep prepchrome || return 1
  wait_exit prep 600; cp "$CTR/prep/out" "$RAW/desktop-prep.txt"; cat "$RAW/desktop-prep.txt"
  mkdir -p "$CTR/desktop"
  in_userns cp -a "$CTR/prep/upper" "$CTR/desktop/upper" && stop_ctr prep && sudo rm -rf "$CTR/prep"
  local t0; t0=$(now)
  CTR_NET=1 CTR_UID=1000 CTR_CAPS=none CTR_HOLD=1 CTR_CONNECT=3128,6901,8080 CTR_BIND=6901,8080 start_ctr desktop desktop || return 1
  wait_marker "$CTR/desktop/out" DESKTOP_DONE 600
  echo "desktop mode finished $(sub "$t0" "$(now)") s after the start"
  sleep 20
  cp "$CTR/desktop/out" "$RAW/desktop.txt"; cp "$CTR/desktop/err" "$RAW/desktop.err"
  grep -E '^(STEP|READY|UP|DESKTOP_DONE)' "$RAW/desktop.txt" | cut -c1-420
  pss_of desktop | tee "$RAW/desktop-mem.txt"
  tail -n 5 "$RAW/desktop.err"
  stop_ctr desktop; sudo rm -rf "$CTR/desktop"
}

# (3) The developer workload, fresh container each run.
section_work() {
  local caps; caps=$(cat "$OUT/work-caps.txt" 2>/dev/null || echo relaxed)
  echo "workload capabilities: $caps"
  : > "$RAW/work-total.txt"
  for i in $(seq "$RUNS"); do
    local t0; t0=$(now)
    CTR_NET=1 CTR_CAPS=$caps start_ctr work$i work || { say "work$i failed to start"; continue; }
    wait_exit work$i 2400 || say "work$i did not finish in 40 minutes"
    echo "work$i $(sub "$t0" "$(now)")" >> "$RAW/work-total.txt"
    cp "$CTR/work$i/out" "$RAW/work-$i.txt"; cp "$CTR/work$i/err" "$RAW/work-$i.err"
    grep -E '^(PHASE|PHASE_TAIL|WORKLOAD_DONE)' "$RAW/work-$i.txt" | cut -c1-300
    echo "upper layer after the workload: $(sudo du -sm "$CTR/work$i/upper" | cut -f1) MiB"
    stop_ctr work$i; sudo rm -rf "$CTR/work$i"
  done
  for phase in git_clone_express apt_update apt_install npm_install rustup_install cargo_install_hexyl git_clone_zlib make_zlib; do
    grep -h "^PHASE $phase " "$RAW"/work-[0-9].txt 2>/dev/null | awk '$4==0{print $2, $3}' > "$RAW/work-phase-$phase.txt"
    [ -s "$RAW/work-phase-$phase.txt" ] && summ "container workload: $phase" "$RAW/work-phase-$phase.txt"
  done
  summ "container workload: whole run including container start" "$RAW/work-total.txt"
}

for s in $SECTIONS; do
  echo; echo "== $s"
  case $s in
    facts) section_facts;; sec) section_sec;; cold) section_cold;; ten) section_ten;; fork) section_fork;;
    net) section_net;; desktop) section_desktop;; work) section_work;;
    *) echo "unknown section $s";;
  esac || say "section $s returned $?"
done
