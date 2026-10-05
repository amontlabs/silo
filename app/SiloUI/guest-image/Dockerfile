# syntax=docker/dockerfile:1
FROM ubuntu:24.04@sha256:224a1869083a311ef3f13648a154ba79832fbef6364d31493642ca03082da254
LABEL org.opencontainers.image.title="Silo Ubuntu guest" \
      org.opencontainers.image.description="Ubuntu 24.04 with Silo's Git integration, offline working-account tools, an Xfce desktop with the Selkies streamer, and the system libraries for Linux computer use. No credentials." \
      org.opencontainers.image.source="https://github.com/amontlabs/silo" \
      org.opencontainers.image.version="ubuntu-24.04-v4"

# v3 content. Unchanged: working-account tools and the Git/GitHub setup.
COPY src-tauri/guest/setup-github.sh /tmp/silo-setup-github.sh
RUN sh /tmp/silo-setup-github.sh \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends sudo python3 openssh-sftp-server \
    && dpkg-query -W > /usr/local/share/silo-packages.txt \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/* /tmp/silo-setup-github.sh

# v4 content: the desktop, computer-use dependencies and accessibility defaults.
#
# Packages, in groups:
#  1+2. Desktop and accessibility: src-tauri/guest/desktop-packages.txt, shared
#     with setup-desktop.sh (which verifies and, if needed, repairs the same set).
#     No mousepad (GTK 3.24's AT-SPI PasteText use-after-free crashes every GTK3
#     text view); gnome-text-editor (GTK4) is the text editor. Includes the dconf
#     stack and python3-pyatspi for the poller in src-tauri/guest/silo-accessibility.py.
#  3. LCU SYSTEM_PACKAGES (checked against v0.9.2; v0.8.5 added libxres1), a superset of the ChatGPT Linux
#     .deb's dependencies on Ubuntu 24.04, so LCU installs with --skip-system --offline.
#     The LCU archive staged below is the lock's version (guest/lcu-lock.json); the
#     published v4 image was built when the lock said 0.8.1.
#     Source: https://github.com/amontlabs/lcu/blob/v0.9.2/scripts/install.py
#     Packages already named in an earlier group are not repeated.
#
# The pinned Selkies .deb is downloaded, SHA-256 verified, installed and deleted
# inside this one RUN so no package file stays in any layer; the locks and the
# poller are bind-mounted, never COPYed. Apt lists and caches are cleaned here.
#
# The pinned LCU release archive (src-tauri/guest/lcu-lock.json, the one place to
# bump it) is downloaded and SHA-256 verified in the same RUN and kept, unextracted,
# under /usr/local/share/silo/lcu/. A VM installs it against the ChatGPT app the host
# mounts at boot (src-tauri/guest/silo-computer-use.py). LCU itself is not installed
# here, and no OpenAI file is ever part of the image.
#
# The third-party notices for the Selkies streamer, its bundled GPL/LGPL codecs and the
# Ubuntu packages (src-tauri/guest/guest-third-party-notices.md, the text that
# THIRD-PARTY-NOTICES.md repeats) ship in /usr/share/doc/silo-guest-third-party/.
ARG TARGETARCH
RUN --mount=type=bind,source=src-tauri/guest/desktop-streamer-lock.json,target=/mnt/desktop-streamer-lock.json \
    --mount=type=bind,source=src-tauri/guest/desktop-packages.txt,target=/mnt/desktop-packages.txt \
    --mount=type=bind,source=src-tauri/guest/silo-accessibility.py,target=/mnt/silo-accessibility.py \
    --mount=type=bind,source=src-tauri/guest/lcu-lock.json,target=/mnt/lcu-lock.json \
    --mount=type=bind,source=src-tauri/guest/guest-third-party-notices.md,target=/mnt/guest-third-party-notices.md \
    set -eu \
    && export DEBIAN_FRONTEND=noninteractive \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        $(cat /mnt/desktop-packages.txt) \
        bubblewrap ffmpeg libasound2t64 libatk-bridge2.0-0t64 libatk1.0-0t64 libatspi2.0-0t64 \
        libc6 libcairo2 libcups2t64 libdbus-1-3 libdrm2 libexpat1 libgbm1 libgcc-s1 \
        libgdk-pixbuf-2.0-0 libgl1 libglib2.0-bin libglib2.0-0t64 libgtk-3-0t64 libnotify4 \
        libnspr4 libnss3 libpango-1.0-0 libssl3t64 libstdc++6 libtss2-esys-3.0.2-0t64 \
        libtss2-mu-4.0.1-0t64 libtss2-tcti-device0t64 libudev1 libusb-1.0-0 libx11-6 \
        libx11-xcb1 libxcb-dri3-0 libxcb1 libxcomposite1 libxdamage1 libxext6 libxfixes3 \
        libxi6 libxkbcommon0 libxrandr2 libxres1 libxtst6 mesa-vulkan-drivers pulseaudio-utils \
        xdg-utils xz-utils \
    && set -- $(python3 -c "import json,re,sys; a=json.load(open('/mnt/desktop-streamer-lock.json'))['assets'][sys.argv[1]]; assert a['url'].startswith('https://github.com/selkies-project/selkies/releases/download/2.0.0/') and re.fullmatch('[0-9a-f]{64}', a['sha256']); print(a['url'], a['sha256'])" "$TARGETARCH") \
    && url=$1 && sha256=$2 \
    && curl --silent --show-error --fail --location --retry 2 --connect-timeout 30 --max-time 600 \
        --proto '=https' --tlsv1.2 "$url" -o /tmp/selkies.deb \
    && echo "$sha256  /tmp/selkies.deb" | sha256sum --check --status \
    && apt-get install -y --no-install-recommends /tmp/selkies.deb \
    && test -x /usr/bin/selkies \
    && rm -f /tmp/selkies.deb \
    && set -- $(python3 -c "import json,re,sys; l=json.load(open('/mnt/lcu-lock.json')); a=l['assets'][sys.argv[1]]; assert l['schemaVersion']==1 and re.fullmatch(r'[0-9][0-9A-Za-z.]*', l['version']) and a['url'].startswith('https://github.com/amontlabs/lcu/releases/download/v'+l['version']+'/') and re.fullmatch('[0-9a-f]{64}', a['sha256']); print(a['url'], a['sha256'])" "$TARGETARCH") \
    && lcu_url=$1 && lcu_sha256=$2 \
    && install -d -m 0755 /usr/local/share/silo/lcu \
    && lcu_archive=/usr/local/share/silo/lcu/${lcu_url##*/} \
    && curl --silent --show-error --fail --location --retry 2 --connect-timeout 30 --max-time 600 \
        --proto '=https' --tlsv1.2 "$lcu_url" -o "$lcu_archive" \
    && echo "$lcu_sha256  $lcu_archive" | sha256sum --check --status \
    && install -D -m 0644 /mnt/guest-third-party-notices.md /usr/share/doc/silo-guest-third-party/NOTICES.md \
    && install -d -m 0755 /usr/local/libexec /etc/dconf/profile /etc/dconf/db/local.d /etc/xdg/autostart \
    && install -m 0755 /mnt/silo-accessibility.py /usr/local/libexec/silo-accessibility \
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
    && printf '%s\n' '{"schemaVersion":1,"version":"ubuntu-24.04-v4","capabilities":["desktop","accessibility","lcu-system-packages","lcu-archive"],"streamerVersion":"2.0.0"}' > /usr/local/share/silo/guest-image.json \
    && dpkg-query -W > /usr/local/share/silo-packages.txt \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/* /tmp/*
CMD ["/bin/bash"]
