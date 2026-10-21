#!/usr/bin/env bash
# Builds FilmCraft with the Voice Isolation extension and runs its checks: rustfmt, clippy
# (warnings are errors), the DSP tests (the effect's own plus FilmCraft's registry-wide stress
# tests), the project/render mapping tests, the engine integration tests, the i18n completeness
# tests and the headless UI tests. The first run compiles FilmCraft (10-30 minutes).
#
#   scripts/verify.sh [--quick] [FILMCRAFT_DIR]     (default: ~/Developer/filmcraft)
#
#   --quick   only the DSP and engine tests (no UI build)
set -uo pipefail

DIR="$HOME/Developer/filmcraft"
QUICK=0
for a in "$@"; do
    case "$a" in
        --quick) QUICK=1 ;;
        *) DIR="$a" ;;
    esac
done
[ -f "$DIR/crates/audio-dsp/src/effects/voice_isolation.rs" ] || {
    echo "ERROR: Voice Isolation is not installed in $DIR (run scripts/install.sh first)." >&2
    exit 1
}
cd "$DIR"
# On Windows, big debug test binaries exceed the MSVC linker's PDB module limit (LNK1140);
# FilmCraft's own UI tests hit it too. Build the checks without debug info there.
case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}" ;;
esac

FAILED=()
run() {
    local label="$1"
    shift
    printf '==> %s\n' "$label"
    cargo "$@" || FAILED+=("$label")
}

for c in filmcraft-audio-dsp filmcraft-project filmcraft-render filmcraft-engine filmcraft-ui-egui; do
    run "rustfmt $c" fmt -p "$c" --check
done
run "DSP tests (effect + registry stress tests)" test -p filmcraft-audio-dsp
run "project/render mapping tests" test -p filmcraft-project -p filmcraft-render
run "engine integration tests" test -p filmcraft-engine --lib voice_isolation
run "engine Essential Sound tests" test -p filmcraft-engine --lib essential_sound
if [ "$QUICK" = 0 ]; then
    # Two lints are allowed because unmodified FilmCraft code (file_dialogs.rs, timeline.rs) trips them
    # under clippy 1.95; every file the extension adds or changes is clean without them.
    run "clippy (warnings are errors)" clippy -p filmcraft-audio-dsp -p filmcraft-project -p filmcraft-render -p filmcraft-engine -p filmcraft-ui-egui --all-targets -- -D warnings -A clippy::nonminimal_bool -A clippy::collapsible_match
    run "i18n completeness tests" test -p filmcraft-ui-egui --lib i18n
    run "headless UI tests" test -p filmcraft-ui-egui --test voice_isolation_ui --test audio_fx_editor_ui --test essential_sound_ui
fi

echo
if [ "${#FAILED[@]}" -eq 0 ]; then
    echo "All checks passed."
    exit 0
fi
printf 'Failed: %s\n' "${FAILED[*]}" >&2
echo "See docs/TROUBLESHOOTING.md." >&2
exit 1
