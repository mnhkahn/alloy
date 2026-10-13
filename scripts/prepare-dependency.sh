#!/bin/sh
set -eu
alloy_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dep_root="$alloy_root/../gpui-rhai"
for patch_name in gpui-rhai-hunk-controls.patch gpui-rhai-menu-focus.patch; do
    patch_file="$alloy_root/patches/$patch_name"
    if git -C "$dep_root" apply --reverse --check "$patch_file" 2>/dev/null; then
        continue
    fi
    git -C "$dep_root" apply --check "$patch_file"
    git -C "$dep_root" apply "$patch_file"
done
