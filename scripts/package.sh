#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
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
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Alloy</string>
<key>CFBundleDisplayName</key><string>Alloy</string>
<key>CFBundleIdentifier</key><string>com.mnhkahn.alloy</string>
<key>CFBundleExecutable</key><string>Alloy</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --deep --sign - "$bundle"
echo "Built $bundle"
