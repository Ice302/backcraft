#!/usr/bin/env bash
# Removes the BackCraft Voice Isolation extension from a FilmCraft checkout.
#
#   scripts/uninstall.sh [FILMCRAFT_DIR]     (default: ~/Developer/filmcraft)
#
# Projects saved while it was installed still open afterwards: FilmCraft keeps the unknown
# "voice_isolation" effect in the project file but no longer processes it.
set -euo pipefail

DIR="${1:-$HOME/Developer/filmcraft}"
PATCH="$(cd "$(dirname "$0")/.." && pwd)/patches/voice-isolation.patch"

if [ ! -f "$DIR/crates/audio-dsp/src/effects/voice_isolation.rs" ]; then
    echo "Voice Isolation is not installed in $DIR."
    exit 0
fi
if git -C "$DIR" apply -R --check --whitespace=nowarn "$PATCH" 2>/dev/null; then
    git -C "$DIR" apply -R --whitespace=nowarn "$PATCH"
elif git -C "$DIR" apply -R --check --3way --whitespace=nowarn "$PATCH" >/dev/null 2>&1; then
    git -C "$DIR" apply -R --3way --whitespace=nowarn "$PATCH"
else
    echo "ERROR: nothing was changed: the extension's files were edited since it was installed, so they can't be reversed automatically." >&2
    echo "Use 'git -C \"$DIR\" status' to see them; see docs/TROUBLESHOOTING.md (Uninstalling)." >&2
    exit 1
fi
echo "Voice Isolation removed from $DIR."
