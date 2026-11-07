#!/bin/bash
# Installs Google Chrome (the guest image has no browser) with apt. Needs the relaxed capabilities: the baseline
# (none) cannot run dpkg and apt as container root. Its result is the "desktop" container's starting upper layer.
now() { date +%s.%N; }
t=$(now)
mkdir -p /tmp/c1-logs
curl -fsSL -m 300 -o /tmp/chrome.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb 2>/tmp/c1-logs/chrome-dl.log
echo "PREP download rc=$? $(awk -v a="$t" -v b="$(now)" 'BEGIN{printf "%.1fs", b-a}')"
t=$(now)
apt-get -o APT::Sandbox::User=root -o Acquire::http::Proxy="$http_proxy" -o Acquire::https::Proxy="$http_proxy" update -qq >/tmp/c1-logs/apt-update.log 2>&1
echo "PREP apt-update rc=$?"
DEBIAN_FRONTEND=noninteractive apt-get -o APT::Sandbox::User=root -o Acquire::http::Proxy="$http_proxy" -o Acquire::https::Proxy="$http_proxy" install -y --no-install-recommends /tmp/chrome.deb >/tmp/c1-logs/apt-install.log 2>&1
echo "PREP apt-install rc=$? $(awk -v a="$t" -v b="$(now)" 'BEGIN{printf "%.1fs", b-a}') $(tail -n 3 /tmp/c1-logs/apt-install.log | tr '\n' ' ' | cut -c1-300)"
rm -f /tmp/chrome.deb
