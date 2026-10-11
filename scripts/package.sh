#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
sh scripts/prepare-dependency.sh
profile="${1:-release}"
version=$(python3 scripts/bundle-info.py --version "${2:-}")
if [ "$profile" = "release" ]; then
    cargo build --release --locked
elif [ "$profile" = "debug" ]; then
    cargo build --locked
else
    echo "Usage: scripts/package.sh [release|debug] [version]" >&2
    exit 1
fi
bundle="dist/Alloy.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "target/$profile/alloy" "$bundle/Contents/MacOS/Alloy.new"
mv -f "$bundle/Contents/MacOS/Alloy.new" "$bundle/Contents/MacOS/Alloy"
iconset="target/AppIcon.iconset"
mkdir -p "$iconset"
for spec in "16 icon_16x16" "32 icon_16x16@2x" "32 icon_32x32" "64 icon_32x32@2x" "128 icon_128x128" "256 icon_128x128@2x" "256 icon_256x256" "512 icon_256x256@2x" "512 icon_512x512" "1024 icon_512x512@2x"; do
    set -- $spec
    sips -z "$1" "$1" assets/alloy-logo.png --out "$iconset/$2.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/AppIcon.icns"
python3 scripts/bundle-info.py --version "$version" --output "$bundle/Contents/Info.plist"
codesign --force --deep --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
echo "Built $bundle ($version)"
