#!/bin/sh
# Start the build in target/debug as the system starts an app, not as a
# shell starts a program.
#
#   scripts/dev-app.sh [arguments for `open`]
#
# On a Mac a bare executable is nobody's app. Opened from the Dock or the
# Finder it is run inside the default terminal app, in a window of that
# app, and it and everything it starts carry that terminal's marks from
# then on: its bundle identifier, its ssh agent's socket. Started from an
# agent's shell it inherits the same from whatever started that shell. So
# the build goes in a bundle of its own,
# target/debug/dev.noindex/Onyx.app, and is opened through the system
# with an environment that holds only what a Dock launch has.
#
# The folder's name ends in ".noindex", which Spotlight passes by: in
# target/debug itself the bundle is indexed, and Launchpad and Spotlight
# list a second Onyx beside the installed one.
#
# The bundle is signed with a signing identity of the developer's when the
# keychain has one (SIGN_IDENTITY, else the first "Developer ID
# Application"). macOS keeps what an app was allowed (the Documents
# folder, the Desktop) by who signed it; a build signed by nobody is a new
# app each time it is built, and the system asks again at every launch
# after a build. With no identity it is signed by nobody and does ask.
# The executable is a copy, since signing writes into the file.
#
# Elsewhere the build is simply run.

cd "$(dirname "$0")/.." || exit 1
bin=target/debug/Onyx
[ -x "$bin" ] || { echo "no $bin: cargo build -p onyx-app" >&2; exit 1; }
[ "$(uname)" = Darwin ] || exec "./$bin"

app=target/debug/dev.noindex/Onyx.app
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
rm -f "$app/Contents/MacOS/Onyx"
cp -c "$bin" "$app/Contents/MacOS/Onyx" 2>/dev/null || cp "$bin" "$app/Contents/MacOS/Onyx"
cp -f crates/onyx-app/assets/icon/icon.icns "$app/Contents/Resources/Onyx.icns"
# The dev build has an identifier of its own, so the system keeps its
# permissions and its windows apart from the installed copy's.
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Onyx</string>
  <key>CFBundleDisplayName</key><string>Onyx</string>
  <key>CFBundleIdentifier</key><string>com.alkalinesoftware.onyx.dev</string>
  <key>CFBundleVersion</key><string>0</string>
  <key>CFBundleShortVersionString</key><string>dev</string>
  <key>CFBundleExecutable</key><string>Onyx</string>
  <key>CFBundleIconFile</key><string>Onyx</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleLocalizations</key><array><string>en</string><string>zh-Hans</string><string>zh-Hant</string><string>ja</string><string>ko</string><string>fr</string><string>de</string><string>es</string></array>
</dict>
</plist>
PLIST

identity="${SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}"
codesign --force --sign "${identity:--}" "$app" >/dev/null 2>&1 || codesign --force --sign - "$app" >/dev/null 2>&1

# `open` hands the app the opener's environment, so the opener's is made
# the one the Dock would give: no shell's PATH, no terminal's marks, the
# system's own ssh agent. ONYX_* go through, for a probe copy with a
# home of its own.
set -- "$@" "$app"
# shellcheck disable=SC2046 # each ONYX_ variable is one word on purpose
exec env -i \
    HOME="$HOME" USER="$USER" LOGNAME="${LOGNAME:-$USER}" SHELL="${SHELL:-/bin/zsh}" \
    PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    TMPDIR="$(getconf DARWIN_USER_TEMP_DIR)" \
    SSH_AUTH_SOCK="$(launchctl getenv SSH_AUTH_SOCK)" \
    $(env | grep -E '^ONYX_[A-Z_]+=' | grep -v ' ') \
    /usr/bin/open -n "$@"
