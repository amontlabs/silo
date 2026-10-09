#!/bin/sh
# Guest-only optional desktop recipe 1. Never run on the host.
set -eu
action=${1:-install}
case "$action" in install|update-streamer) ;; *) echo 'Usage: setup-desktop.sh install|update-streamer' >&2; exit 2 ;; esac
[ "$(id -u)" = 0 ] || { echo 'Desktop installation requires guest root' >&2; exit 1; }
. /etc/os-release
[ "$ID" = ubuntu ] && [ "$VERSION_ID" = 24.04 ] || { echo 'Desktop requires Ubuntu 24.04' >&2; exit 1; }
case "$(dpkg --print-architecture)" in
    arm64) arch=arm64 ;;
    amd64) arch=amd64 ;;
    *) echo 'Desktop requires ARM64 or AMD64' >&2; exit 1 ;;
esac
helper=${SILO_DESKTOP_SERVICE_SOURCE:-$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/desktop-service.py}
streamer_lock=${SILO_DESKTOP_STREAMER_LOCK_SOURCE:-$(dirname -- "$helper")/desktop-streamer-lock.json}
selkies_web_client_patch=${SILO_SELKIES_WEB_CLIENT_PATCH_SOURCE:-$(dirname -- "$helper")/patch-selkies-web-client.py}
selkies_display_patch=${SILO_SELKIES_DISPLAY_PATCH_SOURCE:-$(dirname -- "$helper")/patch-selkies-display-scaling.py}
desktop_packages_file=${SILO_DESKTOP_PACKAGES_SOURCE:-$(dirname -- "$helper")/desktop-packages.txt}
accessibility_source=${SILO_ACCESSIBILITY_HELPER_SOURCE:-$(dirname -- "$helper")/silo-accessibility.py}
[ -f "$helper" ] || { echo 'Desktop lifecycle helper is missing' >&2; exit 1; }
mkdir -p /var/lib/silo-desktop
chmod 0700 /var/lib/silo-desktop
exec 9>/var/lib/silo-desktop/install.lock
flock -w 5 9 || { echo 'Desktop installation is already running' >&2; exit 1; }

load_streamer_lock() {
    [ -f "$streamer_lock" ] || { echo 'Desktop streamer lock is missing' >&2; exit 1; }
    lock_record=$(python3 - "$streamer_lock" "$arch" <<'PY'
import json, re, sys

# SILO_STREAMER_LOCK_V1
lock = json.load(open(sys.argv[1], encoding='utf-8'))
if (set(lock) != {'schemaVersion', 'recipeVersion', 'version', 'resolution', 'assets'} or
        lock['schemaVersion'] != 1 or lock['recipeVersion'] != 4 or
        lock['version'] != '2.0.0' or lock['resolution'] != {'width': 1440, 'height': 900} or
        set(lock['assets']) != {'amd64', 'arm64'}):
    raise SystemExit('Invalid bundled desktop streamer lock')
asset = lock['assets'][sys.argv[2]]
url = ('https://github.com/selkies-project/selkies/releases/download/2.0.0/'
       'selkies-2.0.0-ubuntu24.04-' + sys.argv[2] + '.deb')
if (set(asset) != {'url', 'sha256'} or asset['url'] != url or
        not re.fullmatch(r'[0-9a-f]{64}', asset['sha256'])):
    raise SystemExit('Invalid bundled desktop streamer asset')
print(asset['url'], asset['sha256'], lock['version'], lock['recipeVersion'],
      lock['resolution']['width'], lock['resolution']['height'])
PY
    ) || { echo 'Invalid bundled desktop streamer lock' >&2; exit 1; }
    set -- $lock_record
    [ "$#" = 6 ] || { echo 'Invalid bundled desktop streamer lock' >&2; exit 1; }
    streamer_url=$1
    streamer_digest=$2
    streamer_version=$3
    streamer_recipe=$4
    streamer_width=$5
    streamer_height=$6
}

write_streamer_receipt() {
    python3 - "$arch" "$streamer_digest" "$streamer_version" "$streamer_recipe" \
        "$streamer_width" "$streamer_height" /var/lib/silo-desktop/streamer.json <<'PY'
import json, os, pathlib, sys, tempfile

# SILO_STREAMER_RECEIPT_V1
arch, digest, version, recipe, width, height, destination = sys.argv[1:]
destination = pathlib.Path(destination)
receipt = {
    'schemaVersion': 1, 'state': 'ready', 'backend': 'selkies',
    'version': version, 'recipeVersion': int(recipe), 'architecture': arch,
    'packageSha256': digest,
    'resolution': {'width': int(width), 'height': int(height)},
}
fd, temporary = tempfile.mkstemp(prefix='.streamer-', dir=destination.parent)
try:
    os.fchmod(fd, 0o600)
    with os.fdopen(fd, 'w', encoding='utf-8') as output:
        output.write(json.dumps(receipt, separators=(',', ':')) + '\n')
        output.flush()
        os.fsync(output.fileno())
    os.replace(temporary, destination)
finally:
    try:
        os.unlink(temporary)
    except FileNotFoundError:
        pass
PY
}

ensure_connection_credentials() {
    python3 - "$1" /var/lib/silo-desktop/connection.json <<'PY'
import json, os, pathlib, re, secrets, stat, sys, tempfile

# SILO_DESKTOP_CONNECTION_V1
create_if_missing, destination = sys.argv[1], pathlib.Path(sys.argv[2])
try:
    info = destination.lstat()
except FileNotFoundError:
    if create_if_missing != 'create':
        raise SystemExit('Existing desktop connection credentials are missing')
    receipt = {'username': 'silo', 'password': secrets.token_hex(32), 'port': 6901}
    fd, temporary = tempfile.mkstemp(prefix='.connection-', dir=destination.parent)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, 'w', encoding='utf-8') as output:
            output.write(json.dumps(receipt, separators=(',', ':')) + '\n')
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, destination)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
    info = destination.lstat()
if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or
        stat.S_IMODE(info.st_mode) != 0o600):
    raise SystemExit('Desktop connection credentials are not a root-owned mode-0600 file')
value = json.loads(destination.read_text(encoding='utf-8'))
if (not isinstance(value, dict) or value.get('username') != 'silo' or
        value.get('port') != 6901 or not isinstance(value.get('password'), str) or
        not re.fullmatch(r'[0-9a-f]{64}', value['password'])):
    raise SystemExit('Desktop connection credentials have an invalid format')
PY
}

install_streamer() {
    connection_policy=$1
    load_streamer_lock
    if [ "$connection_policy" = preserve ]; then
        ensure_connection_credentials preserve
    fi
    package=/var/lib/silo-desktop/selkies.deb
    curl --silent --show-error --fail --location --retry 2 --connect-timeout 30 --max-time 600 --proto '=https' --tlsv1.2 "$streamer_url" -o "$package.partial"
    printf '%s  %s\n' "$streamer_digest" "$package.partial" | sha256sum --check --status || { rm -f "$package.partial"; echo 'Desktop streamer download checksum mismatch' >&2; exit 1; }
    mv "$package.partial" "$package"
    apt-get -o DPkg::Lock::Timeout=120 -o Acquire::Retries=2 install -y --no-install-recommends --reinstall "$package"
    [ -x /usr/bin/selkies ] || { echo 'Pinned desktop streamer did not install /usr/bin/selkies' >&2; exit 1; }
    [ -f "$selkies_web_client_patch" ] || { echo 'Selkies web client patch helper is missing' >&2; exit 1; }
    python3 "$selkies_web_client_patch" "$arch"
    [ -f "$selkies_display_patch" ] || { echo 'Selkies display scaling patch helper is missing' >&2; exit 1; }
    python3 "$selkies_display_patch" "$arch"
    ensure_connection_credentials "$connection_policy"
    write_streamer_receipt
    rm -f "$package"
}

if [ "$action" = update-streamer ]; then
    [ -f /var/lib/silo-desktop/installed.json ] || { echo 'Install the Linux desktop first' >&2; exit 1; }
    desktop_status=$(python3 "$helper" status) || { echo 'Unable to verify desktop session state' >&2; exit 1; }
    session_state=$(printf '%s\n' "$desktop_status" | python3 -c 'import json,sys; value=json.load(sys.stdin); print(value.get("sessionState", value.get("state", "unknown")))') || { echo 'Unable to verify desktop session state' >&2; exit 1; }
    [ "$session_state" = stopped ] || { echo 'Stop the desktop before updating its streamer' >&2; exit 1; }
    install_streamer preserve
    install -m 0755 "$helper" /usr/local/bin/silo-desktop
    exit 0
fi

# Desktop sessions share the computer's required working account.
account=$(python3 "$helper" prepare-install)
[ "$account" = 'silo /home/silo' ] || { echo 'Unexpected desktop account' >&2; exit 1; }
desktop_user=silo
desktop_home=/home/silo
mkdir -p /usr/local/libexec /usr/local/share/silo
# Reapply the managed session recipe during upgrades, without closing live apps.
# Existing sessions adopt this environment on their next desktop restart.
configure_session() {
    cat > "$desktop_home/.vnc/xstartup" <<'SESSION'
#!/bin/sh
unset SESSION_MANAGER DBUS_SESSION_BUS_ADDRESS
export DISPLAY=:1
export XDG_RUNTIME_DIR=/run/silo-desktop/user
export XDG_CURRENT_DESKTOP=XFCE
exec dbus-run-session -- xfce4-session
SESSION
    chmod 0755 "$desktop_home/.vnc/xstartup"
    chown "$desktop_user:$(id -gn "$desktop_user")" "$desktop_home/.vnc/xstartup"
}
ensure_theme() {
    if [ "$(dpkg-query -W -f='${Status}' greybird-gtk-theme 2>/dev/null || true)" != 'install ok installed' ]; then
        export DEBIAN_FRONTEND=noninteractive
        apt-get -o DPkg::Lock::Timeout=120 -o Acquire::Retries=2 -o Acquire::http::Timeout=30 update
        apt-get -o DPkg::Lock::Timeout=120 -o Acquire::Retries=2 -o Acquire::http::Timeout=30 install -y --no-install-recommends greybird-gtk-theme
    fi
}
# v4 and later guest images ship the desktop packages, the pinned streamer and the
# accessibility defaults. They describe themselves in a marker file; trust it only
# after the packages and the streamer are verified, otherwise install normally.
image_marker=/usr/local/share/silo/guest-image.json
image_desktop_problem() {
    [ -f "$image_marker" ] || { echo none; return 0; }
    problem=$(python3 - "$image_marker" "$streamer_version" <<'PY'
import json, sys

# SILO_GUEST_IMAGE_MARKER_V1
try:
    marker = json.load(open(sys.argv[1], encoding='utf-8'))
except (OSError, ValueError):
    print('The guest image marker is unreadable')
    raise SystemExit(0)
capabilities = marker.get('capabilities') if isinstance(marker, dict) else None
if not isinstance(capabilities, list) or 'desktop' not in capabilities:
    print('none')
elif marker.get('schemaVersion') != 1 or marker.get('streamerVersion') != sys.argv[2]:
    print('The guest image describes a different desktop streamer')
else:
    print('ok')
PY
    ) || problem='The guest image marker is unreadable'
    [ "$problem" = ok ] || { echo "$problem"; return 0; }
    [ -f "$desktop_packages_file" ] || { echo 'The desktop package list is missing'; return 0; }
    image_packages=$(cat "$desktop_packages_file") || { echo 'The desktop package list is unreadable'; return 0; }
    for package in $image_packages; do
        [ "$(dpkg-query -W -f='${Status}' "$package" 2>/dev/null || true)" = 'install ok installed' ] || { echo "The guest image package $package is missing"; return 0; }
    done
    case "$(dpkg-query -W -f='${Version}' selkies 2>/dev/null || true)" in
        "$streamer_version"-*|"$streamer_version") ;;
        *) echo "The guest image does not contain Selkies $streamer_version"; return 0 ;;
    esac
    [ -x /usr/bin/selkies ] || { echo 'The guest image is missing /usr/bin/selkies'; return 0; }
    # Every program the session needs, not only the packages that normally own them.
    for command_name in xauth Xvfb pulseaudio xfce4-session xfwm4 dbus-run-session runuser; do
        command -v "$command_name" >/dev/null 2>&1 || { echo "The guest image is missing $command_name"; return 0; }
    done
    # The accessibility defaults: the poller, its autostart entry and the dconf database.
    [ -x /usr/local/libexec/silo-accessibility ] || { echo 'The guest image is missing the accessibility helper'; return 0; }
    [ -f /etc/xdg/autostart/silo-accessibility.desktop ] || { echo 'The guest image is missing the accessibility autostart entry'; return 0; }
    [ -f /etc/dconf/profile/user ] && [ -f /etc/dconf/db/local ] && grep -qx 'toolkit-accessibility=true' /etc/dconf/db/local.d/00-silo-accessibility 2>/dev/null || { echo 'The guest image is missing the accessibility settings'; return 0; }
    # The patchers need exactly the pinned web client and display module; they are idempotent.
    [ -f "$selkies_web_client_patch" ] && python3 "$selkies_web_client_patch" "$arch" >/dev/null 2>&1 || { echo 'The Selkies web client is missing or damaged'; return 0; }
    [ -f "$selkies_display_patch" ] && python3 "$selkies_display_patch" "$arch" >/dev/null 2>&1 || { echo 'The Selkies display module is missing or damaged'; return 0; }
    echo ok
}
# Restore the accessibility and default-application settings of the v4 guest image
# (the same files guest-image/Dockerfile writes) when the packages were reinstalled.
restore_image_defaults() {
    [ -f "$accessibility_source" ] || { echo 'Accessibility helper source is missing' >&2; exit 1; }
    install -d -m 0755 /usr/local/libexec /etc/dconf/profile /etc/dconf/db/local.d /etc/xdg/autostart /usr/local/share/silo
    install -m 0755 "$accessibility_source" /usr/local/libexec/silo-accessibility
    printf '%s\n' '[Desktop Entry]' 'Type=Application' 'Name=Silo accessibility' \
        'Comment=Lets assistive tools read Chromium and Electron web content' \
        'Exec=/usr/local/libexec/silo-accessibility' 'NoDisplay=true' \
        'X-GNOME-Autostart-enabled=true' > /etc/xdg/autostart/silo-accessibility.desktop
    printf '%s\n' 'user-db:user' 'system-db:local' > /etc/dconf/profile/user
    printf '%s\n' '[org/gnome/desktop/interface]' 'toolkit-accessibility=true' > /etc/dconf/db/local.d/00-silo-accessibility
    dconf update
    printf '%s\n' '[Default Applications]' 'text/plain=org.gnome.TextEditor.desktop' \
        'text/markdown=org.gnome.TextEditor.desktop' 'text/x-log=org.gnome.TextEditor.desktop' \
        'text/x-python=org.gnome.TextEditor.desktop' 'text/x-shellscript=org.gnome.TextEditor.desktop' \
        'application/json=org.gnome.TextEditor.desktop' 'application/xml=org.gnome.TextEditor.desktop' \
        > /etc/xdg/mimeapps.list
}
# The image cannot hold per-computer state: connection credentials, the web-client patch,
# receipts, the lifecycle helper and the session script. Never touches apt or the network.
provision_image_desktop() {
    [ -f "$selkies_web_client_patch" ] || { echo 'Selkies web client patch helper is missing' >&2; exit 1; }
    python3 "$selkies_web_client_patch" "$arch"
    [ -f "$selkies_display_patch" ] || { echo 'Selkies display scaling patch helper is missing' >&2; exit 1; }
    python3 "$selkies_display_patch" "$arch"
    ensure_connection_credentials create
    write_streamer_receipt
    if [ ! -d "$desktop_home/.vnc" ]; then
        install -d -m 0700 -o "$desktop_user" -g "$(id -gn "$desktop_user")" "$desktop_home/.vnc"
    fi
    configure_session
    install -m 0755 "$helper" /usr/local/bin/silo-desktop
    mkdir -p /usr/local/libexec
    cat > /usr/local/libexec/silo-desktop-boot <<'BOOT'
#!/bin/sh
exec /usr/local/bin/silo-desktop boot
BOOT
    chmod 0755 /usr/local/libexec/silo-desktop-boot
    dpkg-query -W > /var/lib/silo-desktop/packages.txt
    printf '%s\n' '{"version":"1","desktopRecipeVersion":1,"image":"preinstalled"}' > /var/lib/silo-desktop/installed.json
    printf '%s\n' installed > /var/lib/silo-desktop/install-stage
}
load_streamer_lock
image_problem=$(image_desktop_problem)
# A guest carrying the v4 marker is repaired to the complete v4 package set and settings.
v4_guest=0
[ "$image_problem" = none ] || v4_guest=1
if [ -f /var/lib/silo-desktop/installed.json ]; then
    # Explicit reruns revalidate a v4 desktop (preinstalled or already repaired) and
    # repair it through the full install below, which keeps the connection
    # credentials. Healthy, legacy and Kasm installs are only refreshed.
    if [ "$v4_guest" = 0 ] || [ "$image_problem" = ok ] || [ ! -f /var/lib/silo-desktop/streamer.json ]; then
        install -m 0755 "$helper" /usr/local/bin/silo-desktop
        configure_session
        ensure_theme
        python3 "$helper" status
        exit 0
    fi
fi
if [ "$image_problem" = ok ]; then
    printf '%s\n' 'Using the desktop preinstalled in the guest image' >&2
    printf '%s\n' installing > /var/lib/silo-desktop/install-stage
    provision_image_desktop
    /usr/local/bin/silo-desktop boot
    exit 0
elif [ "$image_problem" != none ]; then
    printf '%s; installing the desktop packages instead\n' "$image_problem" >&2
fi
if command -v Xvnc >/dev/null 2>&1 && [ ! -f /var/lib/silo-desktop/install-stage ]; then
    echo 'An existing unmanaged VNC installation conflicts with the Silo desktop' >&2
    exit 1
fi
available=$(df -Pk / | awk 'NR==2 {print $4}')
[ "$available" -ge 2097152 ] || { echo 'Desktop installation requires at least 2 GiB free disk space' >&2; exit 1; }
printf '%s\n' preparing > /var/lib/silo-desktop/install-stage
export DEBIAN_FRONTEND=noninteractive
# Recover packages unpacked before an interrupted installation. apt retains its
# own locks; never remove lock files or claim transactional rollback.
if [ -n "$(dpkg --audit)" ]; then
    dpkg --configure -a || apt-get -o DPkg::Lock::Timeout=120 -o Acquire::Retries=2 install -f -y
fi
apt-get -o DPkg::Lock::Timeout=120 -o Acquire::Retries=2 -o Acquire::http::Timeout=30 update
if [ "$v4_guest" = 1 ]; then
    [ -f "$desktop_packages_file" ] || { echo 'The desktop package list is missing' >&2; exit 1; }
    desktop_packages=$(cat "$desktop_packages_file")
else
    desktop_packages='ca-certificates curl python3 sudo dbus-x11 at-spi2-core xfce4-session xfce4-panel xfce4-settings xfdesktop4 xfwm4 thunar xfce4-terminal mousepad greybird-gtk-theme fonts-dejavu-core xauth x11-utils procps xvfb pulseaudio'
fi
apt-get -o DPkg::Lock::Timeout=120 -o Acquire::Retries=2 -o Acquire::http::Timeout=30 install -y --no-install-recommends $desktop_packages
printf '%s\n' installing > /var/lib/silo-desktop/install-stage
install_streamer create
[ "$v4_guest" = 0 ] || restore_image_defaults
if [ ! -d "$desktop_home/.vnc" ]; then
    install -d -m 0700 -o "$desktop_user" -g "$(id -gn "$desktop_user")" "$desktop_home/.vnc"
fi
configure_session
install -m 0755 "$helper" /usr/local/bin/silo-desktop
mkdir -p /usr/local/libexec
cat > /usr/local/libexec/silo-desktop-boot <<'BOOT'
#!/bin/sh
exec /usr/local/bin/silo-desktop boot
BOOT
chmod 0755 /usr/local/libexec/silo-desktop-boot
dpkg-query -W > /var/lib/silo-desktop/packages.txt
printf '%s\n' '{"version":"1","desktopRecipeVersion":1}' > /var/lib/silo-desktop/installed.json
printf '%s\n' installed > /var/lib/silo-desktop/install-stage
rm -f "$package"
/usr/local/bin/silo-desktop boot
