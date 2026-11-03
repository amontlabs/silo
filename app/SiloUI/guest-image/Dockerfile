# syntax=docker/dockerfile:1
FROM ubuntu:24.04@sha256:224a1869083a311ef3f13648a154ba79832fbef6364d31493642ca03082da254
LABEL org.opencontainers.image.title="Silo Ubuntu guest" \
      org.opencontainers.image.description="Ubuntu 24.04 with Silo's Git integration, offline working-account tools, an Xfce desktop with the Selkies streamer, and Cua Driver for Linux computer use. No credentials." \
      org.opencontainers.image.source="https://github.com/amontlabs/silo" \
      org.opencontainers.image.version="ubuntu-24.04-v5"

# v3 content. Unchanged: working-account tools and the Git/GitHub setup.
COPY src-tauri/guest/setup-github.sh /tmp/silo-setup-github.sh
RUN sh /tmp/silo-setup-github.sh \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends sudo python3 openssh-sftp-server \
    && dpkg-query -W > /usr/local/share/silo-packages.txt \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/* /tmp/silo-setup-github.sh

# v4 content: the desktop and accessibility defaults. v5 replaces the LCU system packages
# and archive with Cua Driver.
#
# Packages, in groups:
#  1+2. Desktop and accessibility: src-tauri/guest/desktop-packages.txt, shared
#     with setup-desktop.sh (which verifies and, if needed, repairs the same set).
#     No mousepad (GTK 3.24's AT-SPI PasteText use-after-free crashes every GTK3
#     text view); gnome-text-editor (GTK4) is the text editor. Includes the dconf
#     stack and python3-pyatspi for the poller in src-tauri/guest/silo-accessibility.py.
#  3. The X libraries the Cua Driver binary links (libX11, libXi, libxkbcommon, libxcb,
#     libXext, libXau, libXdmcp, libbsd, libmd; checked with `ldd` for Cua Driver 0.38.0),
#     the XTest, XFixes and RandR libraries the desktop's input and display path uses, and the
#     few tools Selkies and the agent-tools recipe call (pulseaudio-utils, xdg-utils, xz-utils).
#     The packages v4 added only for the ChatGPT application (bubblewrap, ffmpeg,
#     GTK/Electron/NSS/CUPS/ALSA/TPM/USB libraries, Mesa Vulkan) are gone.
#
# The pinned Selkies .deb is downloaded, SHA-256 verified, installed and deleted
# inside this one RUN so no package file stays in any layer; the locks and the
# poller are bind-mounted, never COPYed. Apt lists and caches are cleaned here.
#
# Cua Driver (MIT, https://github.com/trycua/cua) is the pinned release in
# src-tauri/guest/cua-driver-lock.json, the one place to bump it. Its archive is downloaded and
# size- and SHA-256-verified in the same RUN, and only the `cua-driver` binary is installed, as
# /usr/local/lib/silo/cua-driver/<version>/cua-driver with a /usr/local/bin/cua-driver
# symlink; the other files of the archive are never extracted. Cua Driver sends product
# telemetry by default, so /etc/environment and /etc/profile.d switch it and the update check
# off for every account (the MCP registrations that Silo writes carry the same variables).
# The helper src-tauri/guest/silo-computer-use.py registers it with the agents through the launcher
# src-tauri/guest/silo-cua-driver-mcp (installed here; the helper writes the same text on v4 computers). Its licence
# MIT licence text is part of the notices below. No OpenAI file is ever part of the image.
#
# The third-party notices for the Selkies streamer, its bundled GPL/LGPL codecs, Cua Driver
# and the Ubuntu packages, with Cua Driver's MIT licence text (src-tauri/guest/guest-third-party-notices.md,
# the text that THIRD-PARTY-NOTICES.md repeats), ship in /usr/share/doc/silo-guest-third-party/.
ARG TARGETARCH
RUN --mount=type=bind,source=src-tauri/guest/desktop-streamer-lock.json,target=/mnt/desktop-streamer-lock.json \
    --mount=type=bind,source=src-tauri/guest/desktop-packages.txt,target=/mnt/desktop-packages.txt \
    --mount=type=bind,source=src-tauri/guest/silo-accessibility.py,target=/mnt/silo-accessibility.py \
    --mount=type=bind,source=src-tauri/guest/silo-cua-driver-mcp,target=/mnt/silo-cua-driver-mcp \
    --mount=type=bind,source=src-tauri/guest/cua-driver-shim,target=/mnt/cua-driver-shim \
    --mount=type=bind,source=src-tauri/guest/cua-driver-lock.json,target=/mnt/cua-driver-lock.json \
    --mount=type=bind,source=src-tauri/guest/guest-third-party-notices.md,target=/mnt/guest-third-party-notices.md \
    set -eu \
    && export DEBIAN_FRONTEND=noninteractive \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        $(cat /mnt/desktop-packages.txt) \
        libbsd0 libc6 libgl1 libglib2.0-bin libmd0 libx11-6 libx11-xcb1 libxau6 libxcb1 \
        libxcomposite1 libxdamage1 libxdmcp6 libxext6 libxfixes3 libxi6 libxkbcommon0 \
        libxrandr2 libxtst6 pulseaudio-utils xdg-utils xz-utils \
    && set -- $(python3 -c "import json,re,sys; a=json.load(open('/mnt/desktop-streamer-lock.json'))['assets'][sys.argv[1]]; assert a['url'].startswith('https://github.com/selkies-project/selkies/releases/download/2.0.0/') and re.fullmatch('[0-9a-f]{64}', a['sha256']); print(a['url'], a['sha256'])" "$TARGETARCH") \
    && url=$1 && sha256=$2 \
    && curl --silent --show-error --fail --location --retry 2 --connect-timeout 30 --max-time 600 \
        --proto '=https' --tlsv1.2 "$url" -o /tmp/selkies.deb \
    && echo "$sha256  /tmp/selkies.deb" | sha256sum --check --status \
    && apt-get install -y --no-install-recommends /tmp/selkies.deb \
    && test -x /usr/bin/selkies \
    && rm -f /tmp/selkies.deb \
    && set -- $(python3 -c "import json,re,sys; l=json.load(open('/mnt/cua-driver-lock.json')); a=l['assets'][sys.argv[1]]; assert l['schemaVersion']==1 and re.fullmatch(r'[0-9]+[.][0-9]+[.][0-9]+', l['version']) and a['url'].startswith('https://github.com/trycua/cua/releases/download/cua-driver-rs-v'+l['version']+'/') and re.fullmatch('[0-9a-f]{64}', a['sha256']) and re.fullmatch('[0-9a-f]{64}', a['binarySha256']) and isinstance(a['bytes'], int); print(a['url'], a['sha256'], a['bytes'], l['version'], a['binarySha256'])" "$TARGETARCH") \
    && cua_url=$1 && cua_sha256=$2 && cua_bytes=$3 && cua_version=$4 && cua_binary_sha256=$5 \
    && curl --silent --show-error --fail --location --retry 2 --connect-timeout 30 --max-time 600 \
        --proto '=https' --proto-redir '=https' --tlsv1.2 "$cua_url" -o /tmp/cua-driver.tar.gz \
    && echo "$cua_sha256  /tmp/cua-driver.tar.gz" | sha256sum --check --status \
    && test "$(stat -c %s /tmp/cua-driver.tar.gz)" = "$cua_bytes" \
    && install -d -m 0755 "/usr/local/lib/silo/cua-driver/$cua_version" \
    && tar -xzf /tmp/cua-driver.tar.gz -C "/usr/local/lib/silo/cua-driver/$cua_version" --no-same-owner cua-driver \
    && rm -f /tmp/cua-driver.tar.gz \
    && chmod 0755 "/usr/local/lib/silo/cua-driver/$cua_version/cua-driver" \
    && echo "$cua_binary_sha256  /usr/local/lib/silo/cua-driver/$cua_version/cua-driver" | sha256sum --check --status \
    && sed "s#@DRIVER@#/usr/local/lib/silo/cua-driver/$cua_version/cua-driver#" /mnt/cua-driver-shim > /usr/local/bin/cua-driver \
    && chmod 0755 /usr/local/bin/cua-driver \
    && printf '%s\n' 'CUA_DRIVER_RS_TELEMETRY_ENABLED=0' 'CUA_DRIVER_RS_UPDATE_CHECK=false' >> /etc/environment \
    && printf '%s\n' '# Cua Driver: no telemetry, no update check (Silo guest image).' \
        'export CUA_DRIVER_RS_TELEMETRY_ENABLED=0' 'export CUA_DRIVER_RS_UPDATE_CHECK=false' > /etc/profile.d/silo-cua-driver.sh \
    && chmod 0644 /etc/profile.d/silo-cua-driver.sh \
    && test "$(CUA_DRIVER_RS_TELEMETRY_ENABLED=0 CUA_DRIVER_RS_UPDATE_CHECK=false /usr/local/bin/cua-driver --version)" = "cua-driver $cua_version" \
    && ! ldd "/usr/local/lib/silo/cua-driver/$cua_version/cua-driver" | grep -q 'not found' \
    && install -D -m 0644 /mnt/guest-third-party-notices.md /usr/share/doc/silo-guest-third-party/NOTICES.md \
    && install -d -m 0755 /usr/local/libexec /etc/dconf/profile /etc/dconf/db/local.d /etc/xdg/autostart \
    && install -m 0755 /mnt/silo-accessibility.py /usr/local/libexec/silo-accessibility \
    && install -m 0755 /mnt/silo-cua-driver-mcp /usr/local/libexec/silo-cua-driver-mcp \
    && printf '%s\n' '[Desktop Entry]' 'Type=Application' 'Name=Silo accessibility' \
        'Comment=Lets assistive tools read Chromium and Electron web content' \
        'Exec=/usr/local/libexec/silo-accessibility' 'NoDisplay=true' \
        'X-GNOME-Autostart-enabled=true' > /etc/xdg/autostart/silo-accessibility.desktop \
    && printf '%s\n' 'user-db:user' 'system-db:local' > /etc/dconf/profile/user \
    && printf '%s\n' '[org/gnome/desktop/interface]' 'toolkit-accessibility=true' > /etc/dconf/db/local.d/00-silo-accessibility \
    && dconf update \
    && printf '%s\n' '[Default Applications]' 'text/plain=org.gnome.TextEditor.desktop' \
        'text/markdown=org.gnome.TextEditor.desktop' 'text/x-log=org.gnome.TextEditor.desktop' \
        'text/x-python=org.gnome.TextEditor.desktop' 'text/x-shellscript=org.gnome.TextEditor.desktop' \
        'application/json=org.gnome.TextEditor.desktop' 'application/xml=org.gnome.TextEditor.desktop' \
        > /etc/xdg/mimeapps.list \
    && install -d -m 0755 /usr/local/share/silo \
    && printf '%s\n' '{"schemaVersion":1,"version":"ubuntu-24.04-v5","capabilities":["desktop","accessibility","cua-driver"],"streamerVersion":"2.0.0","cuaDriverVersion":"'"$cua_version"'"}' > /usr/local/share/silo/guest-image.json \
    && dpkg-query -W > /usr/local/share/silo-packages.txt \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/* /tmp/*
CMD ["/bin/bash"]
