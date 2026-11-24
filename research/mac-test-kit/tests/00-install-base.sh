#!/bin/bash
# id: 00
# title: Download macOS and install the base computer
# tier: setup
# time: about an hour (download of about 18 GB, then the install); minutes when already done
# needs: macOS 27 on Apple silicon, 100 GB free, the VM helper (./kit build), no sudo
# decides: computer-image, first-run-time-macos, which-macos-the-mac-runs (file 10 of the research)
# question: How long do the download of Apple's restore image and the install of macOS into a new computer take on this Mac, and does the install work at all?
# outcome installed: The base computer is installed and was never started; every vm test clones it.
#   The download and install times are the two long waits of first-run-time-macos before the
#   first start; computer-image keeps this installed system.
# outcome reused: A base computer was already there and is reused; nothing was measured this time
#   (delete ~/SiloKit/base/installed.silovm to measure again).
# outcome download-failed: The restore image could not be downloaded; the sentence says why.
#   Rerun: the download resumes where it stopped.
# outcome install-failed: VZMacOSInstaller failed; the error's domain, code and name are in the
#   sentence. computer-image's install step needs a retry or a fix before anything else.
. "$KIT_ROOT/lib/common.sh"

BASE_DIR="$(dirname "$KIT_BASE_VM")"
mkdir -p "$BASE_DIR"

if [ "${KIT_FAKE:-0}" = 1 ]; then
  kit_note "Linux run against linux/fake-silovm: the times are not the Mac's"
else
  kit_need_mac 27
fi
kit_need_silovm

hc="$("$SILOVM" host-check)" || kit_fail "silovm host-check failed"
kit_fact host_check "$hc"
hc_get() { printf '%s' "$hc" | python3 -c 'import json,sys; print(json.load(sys.stdin).get(sys.argv[1]))' "$1"; }
[ "$(hc_get virtualization_supported)" = True ] || kit_skip "this Mac does not support virtualization (host-check: $hc)"
[ "$(hc_get provisioning_api)" = True ] || kit_note "silovm has no guest provisioning here (macOS 27 SDK and macOS 27 needed): the base installs, but tests 01, 10 and 13 will skip"

if [ -d "$KIT_BASE_VM" ]; then
  kit_fact base_reused yes
  kit_outcome reused "A base computer is already installed at $KIT_BASE_VM; it is reused and nothing was measured."
  exit 0
fi

[ "${KIT_FAKE:-0}" = 1 ] || kit_need_free_gb 100

info="$("$SILOVM" ipsw-url)" || kit_fail "silovm ipsw-url failed: $info"
url="$(printf '%s' "$info" | python3 -c 'import json,sys; print(json.load(sys.stdin)["url"])')"
build="$(printf '%s' "$info" | python3 -c 'import json,sys; print(json.load(sys.stdin)["build"])')"
osv="$(printf '%s' "$info" | python3 -c 'import json,sys; print(json.load(sys.stdin)["os_version"])')"
kit_fact ipsw_url "$url"
kit_fact ipsw_build "$build"
kit_fact ipsw_os_version "$osv"

ipsw="$BASE_DIR/$build.ipsw"
part="$ipsw.part"

# The size, from the server (the last Content-Length after redirects).
size="$(curl -sSIL "$url" 2>/dev/null | tr -d '\r' | awk 'tolower($1)=="content-length:" {n=$2} END {print n+0}')"
have=0
[ -f "$part" ] && have="$(wc -c < "$part" | tr -d ' ')"
gb() { python3 -c "print('%.1f' % ($1 / 1e9))"; }

download_s=""
if [ -f "$ipsw" ]; then
  kit_fact ipsw_reused yes
else
  if [ "$size" -gt 0 ]; then
    kit_confirm "Downloads macOS $osv ($build) from Apple: $(gb "$size") GB, $(gb "$have") GB already there, into $BASE_DIR, then installs it into a new computer there (about an hour in all; delete $BASE_DIR to undo)."
  else
    kit_confirm "Downloads macOS $osv ($build) from Apple (size unknown, about 18 GB) into $BASE_DIR, then installs it (about an hour; delete $BASE_DIR to undo)."
  fi
  t0="$(kit_now)"
  if [ "$size" -gt 0 ] && [ "$have" -ge "$size" ]; then
    kit_note "the partial download is already complete"
  elif ! curl -fL --retry 5 --retry-delay "${KIT_RETRY_DELAY:-10}" --retry-all-errors -C - -o "$part" "$url" >&2; then
    have="$(wc -c < "$part" 2>/dev/null | tr -d ' ')"
    kit_outcome download-failed "curl stopped after $(gb "${have:-0}") of $(gb "$size") GB from $url; rerun ./kit run 00 to resume."
    exit 0
  fi
  t1="$(kit_now)"
  got="$(wc -c < "$part" | tr -d ' ')"
  if [ "$size" -gt 0 ] && [ "$got" -ne "$size" ]; then
    kit_outcome download-failed "the download holds $got bytes, the server announced $size; rerun ./kit run 00 to resume."
    exit 0
  fi
  mv "$part" "$ipsw"
  download_s="$(python3 -c "print('%.1f' % ($t1 - $t0))")"
  kit_measure download_seconds "$download_s" s "$(gb "$((got - have))") GB downloaded this run"
  kit_measure download_mbit_s "$(python3 -c "d=$t1-$t0; print('%.1f' % (($got - $have) * 8 / 1e6 / d) if d > 0 else 0)")" Mbit/s
fi
kit_measure ipsw_gb "$(gb "$(wc -c < "$ipsw" | tr -d ' ')")" GB

[ -n "$download_s" ] || kit_confirm "Installs macOS $osv from $ipsw into a new computer at $KIT_BASE_VM (about 15 to 30 minutes; delete it to undo)."

log="$KIT_WORK/install.log"
t0="$(kit_now)"
"$SILOVM" install --ipsw "$ipsw" --out "$KIT_BASE_VM" --disk-gb 80 --cpus 4 --memory-gb 8 --disk-format raw \
  > "$KIT_WORK/install.json" 2> "$log"
rc=$?
t1="$(kit_now)"
kit_attach "$log"
kit_attach "$KIT_WORK/install.json"
install_s="$(python3 -c "print('%.1f' % ($t1 - $t0))")"
if [ "$rc" -ne 0 ] || [ ! -d "$KIT_BASE_VM" ]; then
  kit_outcome install-failed "silovm install exited $rc after $install_s s: $(tail -1 "$KIT_WORK/install.json" 2>/dev/null)"
  exit 0
fi
kit_measure install_seconds "$install_s" s "macOS $osv ($build), raw 80 GB disk"
kit_measure base_disk_allocated_gb "$(du -sk "$KIT_BASE_VM" | awk '{printf "%.1f", $1/1048576}')" GB "space the installed computer takes"
kit_outcome installed "macOS $osv ($build) installed in $install_s s${download_s:+ after a download of $download_s s}; the base at $KIT_BASE_VM was never started."
