#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
	printf '%s\n' 'usage: build-network-bootstrap.sh OUTPUT' >&2
	exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
sdk_path=$(xcrun --sdk macosx --show-sdk-path)
output=$1

xcrun --sdk macosx swiftc \
	-sdk "$sdk_path" \
	-target arm64-apple-macos14.0 \
	-swift-version 5 \
	-parse-as-library \
	-O \
	-framework Foundation \
	-framework CryptoKit \
	-framework Security \
	-framework SystemConfiguration \
	-o "$output" \
	"$script_dir/silo-network-bootstrap.swift"
