#!/usr/bin/env bash
# Installs the BackCraft Voice Isolation extension into a FilmCraft source checkout
# (macOS / Linux / Git Bash). Nothing outside that checkout is changed.
#
#   scripts/install.sh [--clone] [--test] [FILMCRAFT_DIR]
#
#   FILMCRAFT_DIR  the FilmCraft checkout (default: ~/Developer/filmcraft)
#   --clone        clone FilmCraft there first (tested commit, new branch "voice-isolation")
#   --test         run scripts/verify.sh afterwards
set -euo pipefail

PINNED_COMMIT=70f0fc7a75f2681bc18302e67eaf0c9d41c68873
MIN_RUST=1.95.0
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PATCH="$ROOT/patches/voice-isolation.patch"
CLONE=0
TEST=0
DIR="$HOME/Developer/filmcraft"
for a in "$@"; do
    case "$a" in
        --clone) CLONE=1 ;;
        --test) TEST=1 ;;
        -h | --help) sed -n '2,10p' "$0"; exit 0 ;;
        *) DIR="$a" ;;
    esac
done

fail() { printf '\nERROR: %s\nSee docs/TROUBLESHOOTING.md for help.\n' "$1" >&2; exit 1; }
step() { printf '==> %s\n' "$1"; }
warn() { printf 'WARNING: %s\n' "$1" >&2; }
version_ge() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]; }

step "Checking tools"
command -v git >/dev/null || fail "git was not found; install it first."
[ -f "$PATCH" ] || fail "The patch file is missing: $PATCH"
if command -v rustc >/dev/null; then
    RV="$(rustc --version | sed -E 's/^rustc ([0-9.]+).*/\1/')"
    if version_ge "$RV" "$MIN_RUST"; then
        echo "    rustc $RV (FilmCraft needs $MIN_RUST or newer)"
    else
        warn "rustc $RV is older than $MIN_RUST, which FilmCraft needs. Run 'rustup update stable' before building (docs/INSTALL.md)."
    fi
else
    warn "Rust was not found. Install it from https://rustup.rs before building FilmCraft (docs/INSTALL.md)."
fi

if [ ! -d "$DIR" ]; then
    [ "$CLONE" = 1 ] || fail "No FilmCraft checkout at '$DIR'. Pass its path, or --clone to download it there."
    step "Cloning FilmCraft into $DIR"
    mkdir -p "$(dirname "$DIR")"
    git clone https://github.com/storytold/filmcraft.git "$DIR" || fail "git clone failed (network?). Retry, or clone manually (docs/INSTALL.md)."
    git -C "$DIR" switch -c voice-isolation "$PINNED_COMMIT" || fail "Could not check out the tested FilmCraft commit $PINNED_COMMIT."
fi
[ -f "$DIR/crates/audio-dsp/src/effects/mod.rs" ] || fail "'$DIR' does not look like a FilmCraft checkout (crates/audio-dsp is missing)."
git -C "$DIR" rev-parse --git-dir >/dev/null 2>&1 || fail "'$DIR' is not a git checkout. Clone FilmCraft with git (docs/INSTALL.md)."

if [ -f "$DIR/crates/audio-dsp/src/effects/voice_isolation.rs" ]; then
    echo "Voice Isolation is already installed in $DIR. (Run scripts/uninstall.sh first to reinstall.)"
    [ "$TEST" = 1 ] && exec "$ROOT/scripts/verify.sh" "$DIR"
    exit 0
fi

HEAD="$(git -C "$DIR" rev-parse HEAD)"
if [ "$HEAD" != "$PINNED_COMMIT" ]; then
    warn "FilmCraft is at ${HEAD:0:10}; the extension was tested at ${PINNED_COMMIT:0:10}. It usually still applies; if not, see docs/INSTALL.md (Newer FilmCraft versions)."
fi

step "Applying the extension"
# Dry runs first, so a checkout the patch doesn't fit is never left half-changed.
if git -C "$DIR" apply --check --whitespace=nowarn "$PATCH" 2>/dev/null; then
    git -C "$DIR" apply --whitespace=nowarn "$PATCH"
else
    echo "    A plain apply does not fit this checkout; trying a three-way merge..."
    if ! WHY="$(git -C "$DIR" apply --check --3way --whitespace=nowarn "$PATCH" 2>&1)"; then
        printf '%s\n' "$WHY" | grep -E 'error|conflict' | sed 's/^/    /' >&2
        fail "Nothing was changed: the patch does not fit this checkout. If you edited FilmCraft files yourself, commit those edits first ('git commit -am \"my changes\"') and run this again; for a much newer FilmCraft see docs/INSTALL.md (Newer FilmCraft versions)."
    fi
    # --check can't see merge conflicts, so merge in a scratch worktree first
    TRIAL="${TMPDIR:-/tmp}/fc-vi-$$"
    git -C "$DIR" worktree add --detach --quiet "$TRIAL" HEAD || fail "Could not create a scratch worktree to test the merge in ($TRIAL)."
    MERGED=1
    git -C "$TRIAL" apply --3way --whitespace=nowarn "$PATCH" >/dev/null 2>&1 || MERGED=0
    CONFLICTS="$(git -C "$TRIAL" diff --name-only --diff-filter=U)"
    git -C "$DIR" worktree remove --force "$TRIAL"
    if [ "$MERGED" = 0 ]; then
        printf '    conflict: %s\n' $CONFLICTS >&2
        fail "Nothing was changed: the extension conflicts with changes in this checkout (listed above). Resolve it by hand as described in docs/INSTALL.md (Newer FilmCraft versions), or install on a fresh checkout of the tested commit."
    fi
    git -C "$DIR" apply --3way --whitespace=nowarn "$PATCH" || fail "git could not apply the patch (see the messages above)."
fi
[ -f "$DIR/crates/audio-dsp/src/effects/voice_isolation.rs" ] || fail "The patch reported success but the new files are missing."

printf '\nVoice Isolation installed into %s\nChanged files:\n' "$DIR"
git -C "$DIR" status --short
cat <<EOF

Next:
  cd "$DIR"
  cargo run -p filmcraft --release
Then: Effects panel > Audio Effects > Noise Reduction/Restoration > Voice Isolation,
or Essential Sound > Dialogue > Repair > Isolate Voice. See docs/USER-GUIDE.md.
EOF

if [ "$TEST" = 1 ]; then
    exec "$ROOT/scripts/verify.sh" "$DIR"
fi
