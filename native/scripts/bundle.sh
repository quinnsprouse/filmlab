#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
identity=${FILMLAB_SIGN:--}
out=target-dist/bundle
app="$out/Filmlab.app"

CARGO_TARGET_DIR=target-dist cargo build --profile dist --quiet
rm -rf "$out"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target-dist/dist/filmlab "$app/Contents/MacOS/Filmlab"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Filmlab</string>
  <key>CFBundleDisplayName</key><string>Filmlab</string>
  <key>CFBundleIdentifier</key><string>app.filmlab.mac</string>
  <key>CFBundleExecutable</key><string>Filmlab</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>26.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.photography</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST
codesign --force --sign "$identity" "$app"
ditto -c -k --keepParent "$app" "$out/Filmlab-$version.zip"
stage="$out/dmg"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname Filmlab -srcfolder "$stage" -ov -format ULFO "$out/Filmlab-$version.dmg"
rm -rf "$stage"
du -sh "$app" "$out/Filmlab-$version.zip" "$out/Filmlab-$version.dmg"
