# Section 5 of final_run.sh (sourced): a macOS fork to ready. Source e2e-final-msrc, forks e2e-final-mfork*.
# Clock: `silo fork SRC --gui` wall time (monotonic) until it returns with computerUse.state ready, i.e. the
# fork is running, its desktop is up and the driver answers. At most 2 macOS VMs (source + one fork).
macos_fork() {
cd "$PROJ" || return
MS=e2e-final-msrc
echo "== X1 create the macOS source (needs the engine's Keychain access; the owner's Keychain repair is open)"
[ "$(nvms)" -le 0 ] || { blocked "macOS" "$(nvms) macOS VM(s) of someone else running"; return; }
t0=$(now); "$SILO" create $MS --macos --cpus 4 --memory 8 --disk 40 > "$RAW/macos-create.out" 2>&1; rc=$?
grep -v '^Creating .*: [0-9]*s$' "$RAW/macos-create.out" | tail -5; echo "create rc=$rc $(elapsed "$t0")s"
if [ $rc != 0 ]; then blocked "macOS fork" "silo create --macos failed: $(grep -v '^Creating' "$RAW/macos-create.out" | tail -1 | cut -c1-240)"; return; fi
"$SILO" start $MS > "$RAW/macos-start.out" 2>&1 || { blocked "macOS fork" "start failed: $(tail -1 "$RAW/macos-start.out" | cut -c1-240)"; return; }
echo "source: $(xs $MS 'sw_vers | tr "\n" " "')"
: > "$RAW/macos-fork.txt"
for i in 1 2 3; do
  f=e2e-final-mfork$i; t0=$(now)
  "$SILO" fork $MS --name $f --gui --gui-timeout 300 --json > "$RAW/macos-fork$i.json" 2> "$RAW/macos-fork$i.err"; rc=$?; s=$(elapsed "$t0")
  cu=$(jfield computerUse.state < "$RAW/macos-fork$i.json")
  echo "macOS fork $i: rc=$rc ${s}s computerUse=$cu state=$(state $f) macOS VMs running=$(nvms)"; tail -2 "$RAW/macos-fork$i.err"
  if [ $rc = 0 ] && [ "$cu" = ready ]; then echo "mfork$i $s" >> "$RAW/macos-fork.txt"; check "X1 macOS fork $i reaches ready" 0 "${s}s"
  else check "X1 macOS fork $i reaches ready" 1 "rc=$rc state=[$cu]"; fi
  [ $i = 1 ] && xs $f 'sw_vers | tr "\n" " "; id -un'
  drop_one $f
done
[ -s "$RAW/macos-fork.txt" ] && python3 "$CHECKS/final_stats.py" "macOS fork to ready (fork --gui returns ready)" "$RAW/macos-fork.txt"
drop_one $MS
}
