#!/bin/zsh
# The end of a release: delete everything compiled. target/ (every build
# made while developing and for the release) and dist/ (the two disk
# images, which are on the release by now), then look for a Onyx.app
# other than the installed one. All of it is rebuilt from the source.
#
# The dev build may be the app this is run from. Its files go with
# target/ and it keeps running, since a running program holds what it has
# open; once the turn is over it is quit and /Applications/Onyx.app is
# opened in its place (scripts/relaunch.sh --installed). So the installed
# copy must be a notarized release, or this refuses.
#
#   scripts/release-clean.sh
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
REPO="${ONYX_REPO:-alkaline-software/onyx}"
root="$PWD"
fail() { print -u2 "release-clean: $1"; exit 1; }

# ps, not pgrep: pgrep leaves out its own ancestors, and the app an agent's
# session runs in is one, so it reports nothing with the dev build up.
running=$(ps -axo pid=,command= | awk -v t="$root/target/" 'index($0, t) && $2 != "awk"' || true)
spctl -a -vv /Applications/Onyx.app 2>&1 | grep -q "Notarized Developer ID" \
  || fail "/Applications/Onyx.app is not a notarized release (docs/release.md, step 8)"

# Images still in dist/ are a release's. A release not yet published may
# be redone, and a redo wants them and the release builds.
if [[ -n "$(print -l dist/*.dmg(N))" ]]; then
  v=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
  draft=$(gh release view "v$v" --repo "$REPO" --json isDraft -q .isDraft 2>/dev/null || true)
  [[ "$draft" == false ]] || fail "dist/ holds disk images and v$v is not published (docs/release.md, step 7)"
fi

before=$(du -sh . | cut -f1)
cargo clean
rm -rf dist
print "release-clean: $before before, $(du -sh . | cut -f1) now"

# Launchpad and Spotlight list every bundle of the name they find.
others=$(mdfind "kMDItemFSName == 'Onyx.app'" | grep -vx "/Applications/Onyx.app" || true)
[[ -z "$others" ]] || fail "another Onyx.app is on the disk; Launchpad and Spotlight list each one:
$others"
print "release-clean: only /Applications/Onyx.app is left"
if [[ -n "$running" ]]; then
  print "release-clean: the build that is running has no files left; the installed one takes over"
  scripts/relaunch.sh --installed
fi
