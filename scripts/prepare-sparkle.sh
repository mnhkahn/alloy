#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
version=2.10.0
checksum=c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c
sdk=target/sparkle-sdk
if [ -f "$sdk/.version" ] && [ "$(cat "$sdk/.version")" = "$version" ] && [ -x "$sdk/bin/generate_appcast" ]; then
    exit 0
fi
mkdir -p "$sdk"
archive="$sdk/Sparkle-$version.tar.xz"
if [ ! -f "$archive" ]; then
    curl -fsSL --retry 3 "https://github.com/sparkle-project/Sparkle/releases/download/$version/Sparkle-$version.tar.xz" -o "$archive.part"
    mv "$archive.part" "$archive"
fi
printf '%s  %s\n' "$checksum" "$archive" | shasum -a 256 -c -
tar -xf "$archive" -C "$sdk" Sparkle.framework bin LICENSE
printf '%s\n' "$version" > "$sdk/.version"
