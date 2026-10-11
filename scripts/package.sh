#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
sh scripts/prepare-dependency.sh
profile="${1:-release}"
if [ "$profile" = "release" ]; then
    cargo build --release --locked
elif [ "$profile" = "debug" ]; then
    cargo build --locked
else
    echo "Usage: scripts/package.sh [release|debug]" >&2
    exit 1
fi
bundle="dist/Alloy.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "target/$profile/alloy" "$bundle/Contents/MacOS/Alloy"
iconset="target/AppIcon.iconset"
mkdir -p "$iconset"
for spec in "16 icon_16x16" "32 icon_16x16@2x" "32 icon_32x32" "64 icon_32x32@2x" "128 icon_128x128" "256 icon_128x128@2x" "256 icon_256x256" "512 icon_256x256@2x" "512 icon_512x512" "1024 icon_512x512@2x"; do
    set -- $spec
    sips -z "$1" "$1" assets/alloy-logo.png --out "$iconset/$2.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/AppIcon.icns"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Alloy</string>
<key>CFBundleDisplayName</key><string>Alloy</string>
<key>CFBundleIdentifier</key><string>com.mnhkahn.alloy</string>
<key>CFBundleExecutable</key><string>Alloy</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --deep --sign - "$bundle"
echo "Built $bundle"
