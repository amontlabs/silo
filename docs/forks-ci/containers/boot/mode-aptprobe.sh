#!/bin/bash
# Can apt run as container root with every capability dropped? One small package.
export DEBIAN_FRONTEND=noninteractive
APT_OPTS=(-o APT::Sandbox::User=root -o "Acquire::http::Proxy=$http_proxy" -o "Acquire::https::Proxy=$http_proxy")
apt-get "${APT_OPTS[@]}" update >/tmp/apt-update.log 2>&1
echo "APTPROBE update rc=$? $(tail -n 2 /tmp/apt-update.log | tr '\n' ' ' | cut -c1-300)"
apt-get "${APT_OPTS[@]}" install -y --no-install-recommends jq >/tmp/apt-install.log 2>&1
echo "APTPROBE install rc=$? $(tail -n 4 /tmp/apt-install.log | tr '\n' ' ' | cut -c1-500)"
echo "APTPROBE jq $(jq --version 2>&1)"
