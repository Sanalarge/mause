#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../.."

base=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
if [ -z "${VERSION:-}" ]; then
    series=${base%.*}
    patch=${base##*.}
    last=$(git tag -l "v$series.*" | sed "s/^v$series\.//" | grep -E '^[0-9]+$' | sort -n | tail -1 || true)
    if [ -n "$last" ] && [ "$last" -ge "$patch" ]; then patch=$((last + 1)); fi
    VERSION="$series.$patch"
fi
version=$VERSION
out=dist
app="$out/Mause.app"
dmg="$out/mause-v$version-macos-universal.dmg"

if [ "$version" != "$base" ]; then
    backup=$(mktemp -d)
    cp Cargo.toml Cargo.lock "$backup/"
    trap 'cp "$backup/Cargo.toml" "$backup/Cargo.lock" .' EXIT
    VERSION=$version perl -0pi -e 's/^version = "[^"]*"/version = "$ENV{VERSION}"/m' Cargo.toml
    cargo update --workspace --quiet
fi

echo "› Derleniyor (Apple Silicon + Intel), sürüm $version"
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin

echo "› Mause.app"
rm -rf "$app" "$dmg"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/mause" \
    target/aarch64-apple-darwin/release/mause target/x86_64-apple-darwin/release/mause
[ -f THIRD_PARTY_VIDEO.md ] && cp THIRD_PARTY_VIDEO.md "$app/Contents/Resources/"

iconset=$(mktemp -d)/Mause.iconset
mkdir -p "$iconset"
png() {
    if command -v rsvg-convert >/dev/null; then
        rsvg-convert -w "$1" -h "$1" assets/logo.svg -o "$2"
    else
        local tmp
        tmp=$(mktemp -d)
        qlmanage -t -s 1024 -o "$tmp" assets/logo.svg >/dev/null 2>&1
        sips -z "$1" "$1" "$tmp/logo.svg.png" --out "$2" >/dev/null
        rm -rf "$tmp"
    fi
}
for s in 16 32 128 256 512; do
    png "$s" "$iconset/icon_${s}x${s}.png"
    png $((s * 2)) "$iconset/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Mause.icns"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Mause</string>
  <key>CFBundleDisplayName</key><string>Mause</string>
  <key>CFBundleIdentifier</key><string>app.mause.Mause</string>
  <key>CFBundleExecutable</key><string>mause</string>
  <key>CFBundleIconFile</key><string>Mause</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Mikrofonunuzu karşı bilgisayara aktarmak için.</string>
  <key>NSAudioCaptureUsageDescription</key><string>Bu bilgisayarın sesini karşı bilgisayara aktarmak için.</string>
  <key>NSAppleEventsUsageDescription</key><string>Bildirim göstermek için.</string>
</dict>
</plist>
EOF

if [ -n "${SIGN_IDENTITY:-}" ]; then
    echo "› İmza: $SIGN_IDENTITY"
    sign() {
        codesign --force --options runtime --timestamp \
            --entitlements packaging/macos/Mause.entitlements --sign "$SIGN_IDENTITY" "$1"
    }
    sign "$app/Contents/MacOS/mause"
    sign "$app"
else
    echo "› İmza: yerel (ad-hoc) — Developer ID olmadan kullanıcı ilk açılışta Gatekeeper uyarısı görür"
    codesign --force --deep --sign - "$app"
fi
codesign --verify --strict "$app"

echo "› $dmg"
stage=$(mktemp -d)
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Uygulamalar"
hdiutil create -quiet -volname "Mause $version" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg"
rm -rf "$stage"
[ -n "${SIGN_IDENTITY:-}" ] && codesign --force --timestamp --sign "$SIGN_IDENTITY" "$dmg"

if [ -n "${NOTARY_PROFILE:-}" ]; then
    echo "› Apple onayı (notarization)"
    xcrun notarytool submit "$dmg" --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$dmg"
    spctl --assess --type open --context context:primary-signature --verbose "$dmg"
fi

hdiutil verify -quiet "$dmg"
echo "✓ $dmg ($(du -h "$dmg" | cut -f1))"
