# Helpers for tests whose computer's network goes through gvproxy (gvisor-tap-vsock), the stack
# research 04 recommends for network-stack. Sourced after lib/common.sh and lib/vm.sh. Written for
# macOS /bin/bash 3.2; every helper also runs on Linux, with gvproxy's Linux build.
#
# One gvproxy per computer, all its files in one folder D (the test's work folder or below it):
#   D/gvproxy.yaml   its configuration (net_gvproxy_config)
#   D/gv.sock        the datagram socket the computer's network card talks to: give silovm
#                    --net vfkit:D/gv.sock
#   D/api.sock       gvproxy's HTTP API on the Mac (leases, port forwards); only this user
#   D/ssh.sock       a Unix socket gvproxy forwards to port 22 inside (net_expose_ssh)
#   D/ssh_config     ssh settings that reach the computer through D/ssh.sock (net_ssh)
# The computer answers SSH at no address of the Mac: ssh goes through D/ssh.sock with nc -U.

NET_GVPROXY_TAG="v0.9.0"   # 7 October 2026; has the fix for spinning after sleep (0.8.9)
NET_GVPROXY_URL="https://github.com/containers/gvisor-tap-vsock/releases/download/$NET_GVPROXY_TAG"
# The release's own sha256sums file, checked on 24 November 2026.
_net_gvproxy_asset() {
  case "$(uname -s)/$(uname -m)" in
    Darwin/*) echo "gvproxy-darwin e2a6825ba789fe588828b418c09aaf16525dec6d8cb2d16c94e72535dd4606be" ;;
    Linux/x86_64) echo "gvproxy-linux-amd64 95c0ee5b5e5d401094ed58ac2f984ba5e935c2cb416c8752329fcfacb721aa9a" ;;
    Linux/aarch64|Linux/arm64) echo "gvproxy-linux-arm64 c1ea864636182a774235c8866c70f2a0223dfcd4f08d24e37a7e82d9882beb9c" ;;
    *) echo "" ;;
  esac
}
NET_GVPROXY="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/gvproxy-$NET_GVPROXY_TAG"

# Silo's gvproxy (#380, which grows #363's -sealed patch): gvisor-tap-vsock's main at the commit
# below (v0.9.0 and 52 commits; the patch needs its dns.go), with net/gvproxy-silo.patch (a copy
# of #380's research/2026-11-25-gvproxy-component/gvproxy-silo.patch), built with Go on this
# machine. It runs with -sealed (nothing of the Mac or a private network) or -closed (no way out).
NET_GVPROXY_COMMIT=197eea05a7b22da4c30c299b63fc93eddfaef893   # main on 13 November 2026
NET_GVPROXY_SEALED="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/gvproxy-197eea05-silo"
NET_GO_VERSION=1.26.0   # what gvisor-tap-vsock v0.9.0's go.mod asks for
# go.dev/dl's sha256 of each archive, checked on 25 November 2026.
_net_go_asset() {
  case "$(uname -s)/$(uname -m)" in
    Darwin/arm64) echo "go$NET_GO_VERSION.darwin-arm64.tar.gz b1640525dfe68f066d56f200bef7bf4dce955a1a893bd061de6754c211431023" ;;
    Linux/x86_64) echo "go$NET_GO_VERSION.linux-amd64.tar.gz aac1b08a0fb0c4e0a7c1555beb7b59180b05dfc5a3d62e40e9de90cd42f88235" ;;
    Linux/aarch64|Linux/arm64) echo "go$NET_GO_VERSION.linux-arm64.tar.gz bd03b743eb6eb4193ea3c3fd3956546bf0e3ca5b7076c8226334afe6b75704cd" ;;
    *) echo "" ;;
  esac
}

# The addresses of gvproxy's default subnet, which the configuration below keeps.
NET_GATEWAY=192.168.127.1
NET_GUEST_IP=192.168.127.2
NET_HOST_ALIAS=192.168.127.254
# Where the hardened configuration sends 192.168.127.254 instead of the Mac's 127.0.0.1: an
# address of TEST-NET-2 (RFC 5737), which no network routes.
NET_BLACKHOLE=198.51.100.1

net_sha256() { python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$1"; }

# net_gvproxy_present: true when the pinned gvproxy is already downloaded and its sha256 matches.
net_gvproxy_present() {
  local want; want="$(_net_gvproxy_asset | cut -d' ' -f2)"
  [ -n "$want" ] && [ -x "$NET_GVPROXY" ] && [ "$(net_sha256 "$NET_GVPROXY")" = "$want" ]
}

# net_gvproxy_get: download the pinned release binary for this system and check its sha256 against
# the release's sha256sums. The caller asks kit_confirm first. Returns 1 on a mismatch.
net_gvproxy_get() {
  net_gvproxy_present && return 0
  local asset name want got tmp
  asset="$(_net_gvproxy_asset)"
  [ -n "$asset" ] || { kit_log "gvproxy has no release binary for $(uname -s)/$(uname -m)"; return 1; }
  name="${asset%% *}"; want="${asset#* }"
  mkdir -p "$(dirname "$NET_GVPROXY")"
  tmp="$NET_GVPROXY.download"
  curl -fsSL --retry 2 -o "$tmp" "$NET_GVPROXY_URL/$name" || { rm -f "$tmp"; kit_log "download of $name failed"; return 1; }
  got="$(net_sha256 "$tmp")"
  if [ "$got" != "$want" ]; then
    rm -f "$tmp"
    kit_log "gvproxy's sha256 is $got, the release says $want: not used"
    return 1
  fi
  chmod 755 "$tmp" && mv "$tmp" "$NET_GVPROXY"
}

# net_go: a Go of version NET_GO_VERSION: the one on PATH if it is that version, else the pinned
# archive from go.dev, checked against its sha256 and kept under the kit's bin folder. Prints the
# path of its go command. The caller asks kit_confirm first.
net_go() {
  local asset name want got dir tmp
  if command -v go >/dev/null 2>&1 && [ "$(go env GOVERSION 2>/dev/null)" = "go$NET_GO_VERSION" ]; then
    command -v go; return 0
  fi
  dir="${KIT_WORK_ROOT:-$HOME/SiloKit}/bin/go-$NET_GO_VERSION"
  [ -x "$dir/bin/go" ] && { echo "$dir/bin/go"; return 0; }
  asset="$(_net_go_asset)"
  [ -n "$asset" ] || { kit_log "no Go archive pinned for $(uname -s)/$(uname -m)"; return 1; }
  name="${asset%% *}"; want="${asset#* }"
  tmp="$dir.download"
  mkdir -p "$(dirname "$dir")"
  curl -fsSL --retry 2 -o "$tmp" "https://go.dev/dl/$name" || { rm -f "$tmp"; kit_log "download of $name failed"; return 1; }
  got="$(net_sha256 "$tmp")"
  [ "$got" = "$want" ] || { rm -f "$tmp"; kit_log "$name's sha256 is $got, go.dev says $want: not used"; return 1; }
  rm -rf "$dir.x" && mkdir -p "$dir.x" && tar -xzf "$tmp" -C "$dir.x" && mv "$dir.x/go" "$dir" && rm -rf "$dir.x" "$tmp" || return 1
  echo "$dir/bin/go"
}

# net_sealed_present: true when Silo's gvproxy is built, from the patch as it is now (its sha256
# is kept beside the binary, so a new copy of the patch is built again).
net_sealed_present() {
  [ -x "$NET_GVPROXY_SEALED" ] && "$NET_GVPROXY_SEALED" -help 2>&1 | grep -q -- -closed &&
    [ "$(cat "$NET_GVPROXY_SEALED.patch-sha256" 2>/dev/null)" = "$(net_sha256 "$KIT_ROOT/net/gvproxy-silo.patch")" ]
}

# net_sealed_build: build gvproxy at NET_GVPROXY_COMMIT with net/gvproxy-silo.patch into
# NET_GVPROXY_SEALED. Needs git, and Go (net_go); Go fetches gvproxy's modules from
# proxy.golang.org, checked against go.sum. The caller asks kit_confirm first.
net_sealed_build() {
  net_sealed_present && return 0
  local go src
  go="$(net_go)" || return 1
  src="${KIT_WORK_ROOT:-$HOME/SiloKit}/work/gvproxy-src"
  rm -rf "$src"
  git clone -q https://github.com/containers/gvisor-tap-vsock "$src" || return 1
  git -C "$src" -c advice.detachedHead=false checkout -q "$NET_GVPROXY_COMMIT" || { kit_log "no commit $NET_GVPROXY_COMMIT in gvisor-tap-vsock"; return 1; }
  git -C "$src" apply "$KIT_ROOT/net/gvproxy-silo.patch" || { kit_log "Silo's patch does not apply"; return 1; }
  (cd "$src" && CGO_ENABLED=0 GOTOOLCHAIN=local GOFLAGS=-mod=readonly "$go" build -o "$NET_GVPROXY_SEALED" ./cmd/gvproxy) >&2 || return 1
  net_sha256 "$KIT_ROOT/net/gvproxy-silo.patch" > "$NET_GVPROXY_SEALED.patch-sha256"
  rm -rf "$src"
  net_sealed_present
}

# net_mac_norm MAC: lower case, two digits per byte, as gvproxy's DHCP server writes it.
net_mac_norm() { python3 -c 'import sys; print(":".join("%02x" % int(x, 16) for x in sys.argv[1].split(":")))' "$1"; }

# net_bundle_mac BUNDLE: the MAC address in the bundle's config.json.
net_bundle_mac() {
  python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["mac"])' "$1/config.json"
}

# net_gvproxy_config D MODE MAC: write D/gvproxy.yaml.
#   default   gvproxy as it comes: 192.168.127.254 is the Mac's 127.0.0.1, and the computer can
#             use gvproxy's API at 192.168.127.1:80 (open port forwards on the Mac) with no token.
#   hardened  what research 04 says Silo needs: 192.168.127.254 goes nowhere, and the API inside
#             needs a token the computer never sees (D/api-token, mode 0600).
#   sealed    Silo's gvproxy with -sealed (net_gvproxy_start D sealed): no API inside, no address
#             mapped to the Mac, no connection or ping to the Mac's addresses or a private,
#             link-local, CGNAT or multicast one. Names are still asked of the Mac's resolver.
#   closed    the same with -closed: nothing goes out and names are refused; forwards into the
#             computer (Silo's SSH) still work.
# Both give the computer 192.168.127.2 (a static lease for MAC) and open no port on the Mac: no
# forward of 127.0.0.1:2222, which gvproxy's command line adds by default.
net_gvproxy_config() {
  local d="$1" mode="$2" mac; mac="$(net_mac_norm "$3")"
  mkdir -p "$d"; chmod 700 "$d"
  {
    echo "log-level: info"
    echo "listen:"
    echo "  - unix://$d/api.sock"
    echo "interfaces:"
    echo "  vfkit: unixgram://$d/gv.sock"
    echo "pid-file: $d/gvproxy.pid"
    echo "log-file: $d/gvproxy.log"
    if [ "$mode" = hardened ]; then
      (umask 077; python3 -c 'import secrets; print(secrets.token_urlsafe(32))' > "$d/api-token")
      chmod 600 "$d/api-token"
      echo "api-token-file: $d/api-token"
    fi
    echo "stack:"
    echo "  mtu: ${NET_MTU:-1500}"
    echo "  subnet: 192.168.127.0/24"
    echo "  gatewayIP: $NET_GATEWAY"
    echo "  deviceIP: $NET_GUEST_IP"
    echo "  hostIP: $NET_HOST_ALIAS"
    echo "  dhcpStaticLeases:"
    echo "    $NET_GUEST_IP: $mac"
    if [ "$mode" = hardened ]; then
      echo "  nat:"
      echo "    $NET_HOST_ALIAS: $NET_BLACKHOLE"
    fi
  } > "$d/gvproxy.yaml"
}

# net_gvproxy_start D [sealed|closed]: start gvproxy with D/gvproxy.yaml (Silo's build, with
# -sealed or -closed, when one is given), wait for its API socket. Prints its pid.
net_gvproxy_start() {
  local d="$1" i=0 pid bin="$NET_GVPROXY" flag=""
  case "${2:-}" in sealed|closed) bin="$NET_GVPROXY_SEALED"; flag="-$2" ;; esac
  rm -f "$d/gv.sock" "$d/api.sock" "$d/ssh.sock" "$d/gvproxy.pid"
  [ "${#d}" -lt 90 ] || { kit_log "socket folder path is ${#d} bytes; a Unix socket path must stay under 104"; return 1; }
  # shellcheck disable=SC2086
  "$bin" -config "$d/gvproxy.yaml" $flag > "$d/gvproxy.out" 2>&1 &
  pid=$!
  echo "$pid" > "$d/gvproxy.run.pid"
  while [ ! -S "$d/api.sock" ] || [ ! -S "$d/gv.sock" ]; do
    kill -0 "$pid" 2>/dev/null || { cat "$d/gvproxy.out" >&2; return 1; }
    i=$((i + 1)); [ "$i" -lt 100 ] || return 1
    sleep 0.1
  done
  echo "$pid"
}

# net_gvproxy_stop D: stop the gvproxy started in D, if it runs.
net_gvproxy_stop() {
  local d="$1" pid i=0
  [ -f "$d/gvproxy.run.pid" ] || return 0
  pid="$(cat "$d/gvproxy.run.pid")"
  kill "$pid" 2>/dev/null
  while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
  kill -9 "$pid" 2>/dev/null
  rm -f "$d/gvproxy.run.pid" "$d/gv.sock" "$d/api.sock" "$d/ssh.sock"
}

# net_api D PATH [JSON]: one call to gvproxy's API on the Mac; a POST when JSON is given. Prints
# the HTTP status; the body goes to D/api.body.
net_api() {
  local d="$1" path="$2"
  if [ -n "${3:-}" ]; then
    curl -s -m 10 --unix-socket "$d/api.sock" -o "$d/api.body" -w '%{http_code}' -X POST -d "$3" "http://gvproxy$path"
  else
    curl -s -m 10 --unix-socket "$d/api.sock" -o "$d/api.body" -w '%{http_code}' "http://gvproxy$path"
  fi
}

# net_lease D MAC: the address gvproxy's DHCP server gave MAC, or nothing.
net_lease() {
  local d="$1" mac; mac="$(net_mac_norm "$2")"
  [ "$(net_api "$d" /services/dhcp/leases)" = 200 ] || return 1
  python3 -c '
import json, sys
for ip, m in json.load(open(sys.argv[1])).items():
    if m.lower() == sys.argv[2]:
        print(ip)
' "$d/api.body" "$mac"
}

# net_wait_lease D MAC SECONDS: wait for a lease. Prints the address; 1 when none came in time.
net_wait_lease() {
  local d="$1" mac="$2" limit="$3" t0 ip
  t0="$(kit_now)"
  while :; do
    ip="$(net_lease "$d" "$mac" 2>/dev/null)"
    [ -n "$ip" ] && { echo "$ip"; return 0; }
    python3 -c "import sys,time; sys.exit(0 if time.time() - $t0 < $limit else 1)" || return 1
    sleep 1
  done
}

# net_expose_ssh D IP: ask gvproxy to listen on D/ssh.sock and forward each connection to IP:22
# inside, then write D/ssh_config for it. Prints the HTTP status (200 when it worked).
net_expose_ssh() {
  local d="$1" ip="$2" code i=0
  rm -f "$d/ssh.sock"
  code="$(net_api "$d" /services/forwarder/expose "{\"local\":\"$d/ssh.sock\",\"protocol\":\"unix\",\"remote\":\"tcp://$ip:22\"}")"
  while [ "$code" = 200 ] && [ ! -S "$d/ssh.sock" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
  : > "$d/known_hosts"
  cat > "$d/ssh_config" <<EOF
Host guest
  HostName guest
  ProxyCommand nc -U "$d/ssh.sock"
  UserKnownHostsFile "$d/known_hosts"
  StrictHostKeyChecking no
  LogLevel ERROR
  ConnectTimeout 10
  ServerAliveInterval 5
  ServerAliveCountMax 3
EOF
  echo "$code"
}

# net_limit SECONDS cmd...: run cmd, kill it after SECONDS (macOS has no timeout command).
net_limit() {
  local n="$1" p w rc; shift
  "$@" & p=$!
  ( sleep "$n"; kill -TERM "$p" 2>/dev/null ) >/dev/null 2>&1 & w=$!
  wait "$p"; rc=$?
  kill "$w" 2>/dev/null; wait "$w" 2>/dev/null
  return "$rc"
}

# net_ssh D USER cmd...: one command inside, through D/ssh.sock, with the kit's key.
net_ssh() {
  local d="$1" u="$2"; shift 2
  if [ -t 0 ]; then  # never the terminal's input (see vm_ssh)
    ssh -F "$d/ssh_config" -o BatchMode=yes -o IdentitiesOnly=yes -i "$(_vm_key)" -l "$u" guest "$@" < /dev/null
  else
    ssh -F "$d/ssh_config" -o BatchMode=yes -o IdentitiesOnly=yes -i "$(_vm_key)" -l "$u" guest "$@"
  fi
}

# net_ssh_pw D USER cmd...: the same with the provisioned password (before the key is set).
net_ssh_pw() {
  local d="$1" u="$2"; shift 2
  vm_password >/dev/null
  SSH_ASKPASS="$(_vm_askpass)" SSH_ASKPASS_REQUIRE=force DISPLAY=none \
    ssh -F "$d/ssh_config" -o PreferredAuthentications=password,keyboard-interactive \
    -o PubkeyAuthentication=no -l "$u" guest "$@" < /dev/null
}

# net_ready D BUNDLE USER SECONDS: wait until SSH through D/ssh.sock answers (the key first, then
# the password), and put the kit's key in. Prints the seconds since the run's "started" event.
net_ready() {
  local d="$1" b="$2" u="$3" limit="$4" t0 pub
  t0="$(kit_now)"
  pub="$(cat "$(_vm_key).pub")"
  while :; do
    if [ -n "$b" ]; then vm_running "$b" || return 1; fi
    net_limit 20 net_ssh "$d" "$u" true < /dev/null 2>/dev/null && break
    if net_limit 20 net_ssh_pw "$d" "$u" true 2>/dev/null; then
      net_limit 30 net_ssh_pw "$d" "$u" "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo '$pub' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys" >/dev/null 2>&1
      net_limit 20 net_ssh "$d" "$u" true < /dev/null 2>/dev/null && break
    fi
    python3 -c "import sys,time; sys.exit(0 if time.time() - $t0 < $limit else 1)" || return 1
    sleep 3
  done
  if [ -n "$b" ]; then
    python3 - "$b/run.jsonl" "$(kit_now)" <<'EOF'
import json, sys
t = None
for line in open(sys.argv[1]):
    try: e = json.loads(line)
    except ValueError: continue
    if e.get("event") == "started": t = e.get("t")
print("%.1f" % (float(sys.argv[2]) - t) if t else "nan")
EOF
  else
    python3 -c "print('%.1f' % ($(kit_now) - $t0))"
  fi
}

# net_proc_sample PID: "RSS_KB CPU_SECONDS" of a process, from ps (macOS and Linux formats).
net_proc_sample() {
  ps -o rss= -o time= -p "$1" 2>/dev/null | python3 -c '
import sys
f = sys.stdin.read().split()
if len(f) < 2: sys.exit(1)
t, days = f[1], 0
if "-" in t: d, t = t.split("-", 1); days = int(d)
parts = [float(x) for x in t.split(":")]
s = 0.0
for p in parts: s = s * 60 + p
print(f[0], "%.2f" % (s + days * 86400))
'
}

# net_my_lan_ip: the Mac's address on the network its default route leaves by (none: nothing).
net_my_lan_ip() {
  python3 -c '
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
try:
    s.connect(("192.0.2.1", 9)); print(s.getsockname()[0])
except OSError: pass
'
}
