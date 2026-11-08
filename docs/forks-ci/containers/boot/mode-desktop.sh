#!/bin/bash
# The Silo guest image's desktop under the baseline, as the image's unprivileged user (uid 1000).
# Xvfb, PulseAudio, Xfce, the Selkies streamer, Cua Driver, and Chrome if a prior step installed it.
export HOME=/home/ubuntu USER=ubuntu DISPLAY=:1 XDG_RUNTIME_DIR=/tmp/runtime-ubuntu
export CUA_DRIVER_RS_TELEMETRY_ENABLED=0 CUA_DRIVER_RS_UPDATE_CHECK=false
mkdir -p -m 700 "$XDG_RUNTIME_DIR"
now() { date +%s.%N; }
span() { awk -v a="$1" -v b="$(now)" 'BEGIN{printf "%.2f", b-a}'; }
step() { echo "STEP $1 $2 $3"; }
wait_for() { # SECONDS COMMAND...
  local limit=$1; shift; local t=$(now)
  while ! "$@" >/dev/null 2>&1; do
    awk -v a="$t" -v b="$(now)" -v l="$limit" 'BEGIN{exit !(b-a>l)}' && return 1
    sleep 0.2
  done
}
mkdir -p /tmp/c1-logs-ubuntu
pss_now() { python3 -c "
import glob
t = 0
for p in glob.glob('/proc/[0-9]*/smaps_rollup'):
    try:
        t += sum(int(l.split()[1]) for l in open(p) if l.startswith('Pss:'))
    except OSError:
        pass
print(f'{t/1024:.1f} MiB')"; }

t=$(now)
Xvfb :1 -screen 0 1440x900x24 +extension RANDR -noreset -nolisten tcp >/tmp/c1-logs-ubuntu/xvfb.log 2>&1 &
if wait_for 20 xdpyinfo -display :1; then step xvfb ok "$(span $t)s"; else step xvfb FAIL "$(head -c 300 /tmp/c1-logs-ubuntu/xvfb.log | tr '\n' ' ')"; fi

t=$(now)
pulseaudio --daemonize=no --exit-idle-time=-1 >/tmp/c1-logs-ubuntu/pulse.log 2>&1 &
if wait_for 20 pactl info; then step pulseaudio ok "$(span $t)s"; else step pulseaudio FAIL "$(head -c 300 /tmp/c1-logs-ubuntu/pulse.log | tr '\n' ' ')"; fi

t=$(now)
dbus-run-session -- startxfce4 >/tmp/c1-logs-ubuntu/xfce.log 2>&1 &
if wait_for 60 bash -c 'pgrep -x xfwm4 && pgrep -x xfce4-panel'; then step xfce ok "$(span $t)s"; else step xfce FAIL "$(head -c 300 /tmp/c1-logs-ubuntu/xfce.log | tr '\n' ' ') procs=$(pgrep -a -u ubuntu | cut -c1-40 | tr '\n' ';' | cut -c1-300)"; fi

t=$(now)
selkies --addr=127.0.0.1 --port=6901 --enable-https=false --basic-auth-user=ubuntu --basic-auth-password=c1test \
  --encoder=h264enc --use-cpu=true --mode=websockets --enable-dual-mode=false --enable-resize=true \
  --use-css-scaling=false --video-streaming-mode=false --framerate=30 --file-transfers=none \
  --audio-enabled=true --audio-bitrate=64000 --microphone-enabled=false >/tmp/c1-logs-ubuntu/selkies.log 2>&1 &
selkies_pid=$!
if wait_for 60 bash -c 'test "$(curl -s --noproxy "*" -o /dev/null -w "%{http_code}" -u ubuntu:c1test http://127.0.0.1:6901/)" = 200'; then
  step selkies-http ok "$(span $t)s"
else
  step selkies-http FAIL "$(tail -c 400 /tmp/c1-logs-ubuntu/selkies.log | tr '\n' ' ')"
fi
sleep 5
if kill -0 "$selkies_pid" 2>/dev/null; then step selkies-alive ok "5s after serving"; else step selkies-alive FAIL "$(tail -c 400 /tmp/c1-logs-ubuntu/selkies.log | tr '\n' ' ')"; fi

sleep 10
step pss-desktop-before-cua-and-browser "$(pss_now)" "(all processes in the container, read inside it: Xvfb, PulseAudio, Xfce, Selkies)"
if [ "${C1_MIN:-}" = 1 ]; then echo "DESKTOP_DONE"; exit 0; fi
t=$(now)
step cua-version "$(cua-driver --version 2>&1 | head -1)" ""
timeout 150 python3 /silo/cua_probe.py 2>&1 | sed 's/^/STEP /'

if [ -x /opt/google/chrome/chrome ]; then
  step chrome-version "$(/opt/google/chrome/chrome --version 2>&1 | head -1)" ""
  t=$(now)
  out=$(timeout 40 /opt/google/chrome/chrome --headless=new --disable-gpu --user-data-dir=/tmp/c1-chrome-a --dump-dom about:blank 2>&1); rc=$?
  echo "$out" > /tmp/c1-logs-ubuntu/chrome-sandbox-on.log
  step chrome-sandbox-on "rc=$rc" "$(echo "$out" | grep -i -E 'sandbox|namespace|FATAL|<html' | tr '\n' ' ' | cut -c1-500)"
  out=$(timeout 40 /opt/google/chrome/chrome --headless=new --disable-gpu --no-sandbox --user-data-dir=/tmp/c1-chrome-b --dump-dom about:blank 2>&1); rc=$?
  echo "$out" > /tmp/c1-logs-ubuntu/chrome-no-sandbox.log
  step chrome-no-sandbox "rc=$rc" "$(echo "$out" | grep -i -E '<html|FATAL' | tr '\n' ' ' | cut -c1-300)"
  t=$(now)
  /opt/google/chrome/chrome --no-sandbox --disable-gpu --user-data-dir=/tmp/c1-chrome-c --no-first-run about:blank >/tmp/c1-logs-ubuntu/chrome.log 2>&1 &
  if wait_for 40 bash -c 'xwininfo -root -tree | grep -qi chrome'; then step chrome-window-no-sandbox ok "$(span $t)s"; else step chrome-window-no-sandbox FAIL "$(tail -c 300 /tmp/c1-logs-ubuntu/chrome.log | tr '\n' ' ')"; fi
else
  step chrome NOT_INSTALLED "no browser in the image; the preparation step did not install one"
fi
sleep 10
step pss-desktop-with-chrome-window "$(pss_now)" "(the same, plus a headed Chrome with --no-sandbox on about:blank)"
cp -r /tmp/c1-logs-ubuntu /tmp/c1-logs-keep 2>/dev/null
echo "DESKTOP_DONE"
