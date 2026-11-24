#!/bin/bash
# ./kit selftest: every check that runs without a Mac and changes nothing outside a temporary
# folder. On a Mac it runs the same checks; the Linux analogues skip themselves there.
set -u
KIT_ROOT="${KIT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
cd "$KIT_ROOT" || exit 1
fails=0
step() { printf '\n--- %s\n' "$*"; }
ok() { printf 'ok   %s\n' "$*"; }
bad() { printf 'FAIL %s\n' "$*"; fails=$((fails + 1)); }

step "shell syntax"
for f in kit lib/*.sh tests/*.sh linux/*.sh; do
  [ -f "$f" ] || continue
  if bash -n "$f"; then ok "$f"; else bad "$f"; fi
done

step "shellcheck (warnings and errors)"
if command -v shellcheck >/dev/null 2>&1; then
  for f in kit lib/*.sh tests/*.sh linux/*.sh; do
    [ -f "$f" ] || continue
    if shellcheck -s bash -S warning -x "$f" >/dev/null 2>&1; then ok "$f"; else bad "$f"; shellcheck -s bash -S warning -x "$f" | head -20; fi
  done
else
  echo "shellcheck not installed, skipped"
fi

step "bash 3.2 constructs (macOS /bin/bash)"
for f in kit lib/*.sh tests/*.sh; do
  [ -f "$f" ] || continue
  hits="$(grep -nE 'declare -A|local -A|mapfile|readarray|\$\{[A-Za-z_]+,,|\$\{[A-Za-z_]+\^\^|declare -n|\|&|&>>' "$f" | grep -v '^[0-9]*:\s*#' || true)"
  if [ -z "$hits" ]; then ok "$f"; else bad "$f"; echo "$hits"; fi
done

step "GNU-only flags"
for f in kit lib/*.sh tests/*.sh; do
  [ -f "$f" ] || continue
  hits="$(grep -nE "date \+[^ ]*%N|stat -c|sed -i ['\"]?s|grep -P|\btimeout [0-9]|readlink -f|sort -V" "$f" | grep -v '^[0-9]*:\s*#' || true)"
  if [ -z "$hits" ]; then ok "$f"; else bad "$f"; echo "$hits"; fi
done

step "test headers and outcome keys"
if python3 py/kitreport.py check .; then ok "every header"; else bad "headers"; fi

step "python compiles"
for f in py/*.py py/*/*.py; do
  [ -f "$f" ] || continue
  if python3 -m py_compile "$f" 2>/dev/null; then ok "$f"; else bad "$f"; python3 -m py_compile "$f"; fi
done

step "C programs build"
tmpbin="$(mktemp -d)"
for f in c/*.c; do
  [ -f "$f" ] || continue
  if cc -O2 -Wall -Werror=implicit-function-declaration -o "$tmpbin/$(basename "$f" .c)" "$f" 2>"$tmpbin/err"; then ok "$f"; else bad "$f"; cat "$tmpbin/err"; fi
done
rm -rf "$tmpbin"

step "RESULTS.md is up to date"
cp RESULTS.md /tmp/.kit-results-before 2>/dev/null || : > /tmp/.kit-results-before
python3 py/kitreport.py doc . >/dev/null
if cmp -s RESULTS.md /tmp/.kit-results-before; then ok "RESULTS.md"; else bad "RESULTS.md was stale and has been rewritten; commit it"; fi
rm -f /tmp/.kit-results-before

step "Linux analogues and fakes"
for f in linux/test-*.sh; do
  [ -f "$f" ] || continue
  if bash "$f" > "/tmp/.kit-$(basename "$f").log" 2>&1; then ok "$f"; else bad "$f (log: /tmp/.kit-$(basename "$f").log)"; tail -20 "/tmp/.kit-$(basename "$f").log"; fi
done

echo
if [ "$fails" = 0 ]; then echo "selftest: all passed"; else echo "selftest: $fails failed"; fi
exit $((fails > 0))
