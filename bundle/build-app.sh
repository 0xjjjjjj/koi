#!/bin/bash
set -e
cargo build --release
APP="target/Koi.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"
cp target/release/koi "$APP/Contents/MacOS/koi"
cp bundle/Info.plist "$APP/Contents/Info.plist"
cp bundle/koi.icns "$APP/Contents/Resources/koi.icns"
echo "Built $APP"
