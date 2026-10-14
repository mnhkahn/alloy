#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
mkdir -p target
clang -fobjc-arc -fblocks tests/updater_native.m -framework Foundation -framework AppKit -o target/updater-native-tests
target/updater-native-tests
