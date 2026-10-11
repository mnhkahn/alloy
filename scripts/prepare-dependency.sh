#!/bin/sh
set -eu
alloy_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dep_root="$alloy_root/../gpui-rhai"
patch_file="$alloy_root/patches/gpui-rhai-hunk-controls.patch"
if git -C "$dep_root" apply --reverse --check "$patch_file" 2>/dev/null; then
    exit 0
fi
git -C "$dep_root" apply --check "$patch_file"
git -C "$dep_root" apply "$patch_file"
