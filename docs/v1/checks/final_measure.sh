# Section 3 of final_run.sh (sourced): measurements on Linux. Numbers only from this run.
# Cold = main running and idle, no usable warm checkpoint (the last `silo up` is older than warm_max_age, 60 s),
#   so the fork first captures main (`silo-fork-<id>` checkpoint) and then starts. Images are cached.
# Clock = monotonic wall time of the `silo fork` process, from spawn to exit; it exits after the fork is
#   started and `true` has run in it (running and answering). The CLI polls jobs every 300 ms, so each value
#   can read up to 0.3 s late; the engine does not expose job timestamps to the CLI.
fp() { python3 "$CHECKS/final_footprint.py" "$1"; }
header_facts() {
echo "commit: $(git -C "$REPO" rev-parse HEAD); tracked changes under app/: [$(git -C "$REPO" status --porcelain app | tr '\n' ' ')]"
echo "machine: $(sysctl -n machdep.cpu.brand_string), $(( $(sysctl -n hw.memsize) / 1073741824 )) GiB RAM, $(sysctl -n hw.ncpu) cores, macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion)), $(uname -m)"
echo "bundle: $BUNDLE; engine pid $(engine_pid); silo CLI $("$SILO" --version) (release profile, debug assertions on, Dev channel)"
echo "msb: $("$BUNDLE/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/msb" --version 2>&1 | head -1); microsandboxVersion $(jfield microsandboxVersion < "$REPO/app/SiloUI/runtime-inputs.json") (source commit $(jfield sourceCommit < "$REPO/app/SiloUI/runtime-inputs.json" | cut -c1-12)); Silo patches on it: $(ls "$REPO"/app/SiloUI/patches/*.patch | wc -l | tr -d ' ')"
echo "libkrunfw: $(jfield libkrunfwVersion < "$REPO/app/SiloUI/runtime-inputs.json") (commit $(jfield libkrunfwCommit < "$REPO/app/SiloUI/runtime-inputs.json" | cut -c1-12)); libkrun (msb_krun crate, macOS gateway lock): $(grep -A1 'name = "msb_krun"$' "$REPO/app/SiloUI/src-tauri/macos-gateway/Cargo.lock" | grep version)"
python3 - "$REPO/app/SiloUI/guest-image/image-lock.json" <<'PY'
import json, sys
lock = json.load(open(sys.argv[1]))
for arch, image in lock['images'].items():
    print(f"guest image {arch}: {image['version']} digest {image['imageDigest']} archive sha256 {image['archiveSha256']} ({image['archiveBytes']} bytes, {image['unpackedBytes']} unpacked)")
PY
echo "computers: $("$SILO" list | tr -s ' ' | tr '\n' ';')"
}

measure_cold() {
cd "$PROJ" || return
: > "$RAW/cold.txt"; : > "$RAW/cold-fp.txt"
echo "main size: $("$SILO" list | grep '^main ')"
for i in 1 2 3; do
  say "cold run $i: waiting 70 s so no warm checkpoint is usable"; sleep 70
  timed_fork cold$i "$RAW/cold.txt" main --name e2e-final-cold$i -- true
  grep -q 'warm checkpoint' "$RAW/fork-cold$i.err" && echo "  NOTE: cold run $i reused a warm checkpoint"
  if [ $i = 1 ]; then sleep 20; echo "-- footprint of cold fork $i (20 s after ready)"; fp e2e-final-cold$i | tee -a "$RAW/cold-fp.txt"; fi
  drop_one e2e-final-cold$i
done
python3 "$CHECKS/final_stats.py" "Linux fork of a running main, cold, to running+ready" "$RAW/cold.txt"
}

measure_warm4() {
cd "$PROJ" || return
: > "$RAW/warm4.txt"; : > "$RAW/warm4-fp.txt"
for i in 1 2 3; do
  "$SILO" up --recipe demo > "$RAW/up-warm$i.out" 2>&1; sleep 3
  timed_fork warm4-$i "$RAW/warm4.txt" main -n 4 --name e2e-final-w$i -- true
  if grep -q 'Checkpointing main' "$RAW/fork-warm4-$i.err"; then echo "  NOTE: run $i took a new checkpoint (warm checkpoint not used)"; else echo "  run $i forked from the warm checkpoint (no new checkpoint)"; fi
  if [ $i = 1 ]; then sleep 20; for k in 1 2 3 4; do echo "-- footprint of e2e-final-w1-$k"; fp e2e-final-w1-$k | tee -a "$RAW/warm4-fp.txt"; done; fi
  for k in 1 2 3 4; do drop_one e2e-final-w$i-$k; done
done
python3 "$CHECKS/final_stats.py" "-n 4 from the warm checkpoint, to 4 running+ready" "$RAW/warm4.txt"
}
