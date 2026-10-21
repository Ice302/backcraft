# Source (read-only copies)

The three new files of the extension, for reading without a FilmCraft checkout:

| File | Lives in FilmCraft at |
|---|---|
| `voice_isolation.rs` | `crates/audio-dsp/src/effects/voice_isolation.rs` (the DSP and its unit tests) |
| `voice_isolation_tests.rs` | `crates/engine/src/voice_isolation_tests.rs` (engine integration tests) |
| `voice_isolation_ui.rs` | `crates/ui-egui/tests/voice_isolation_ui.rs` (headless UI tests) |

The edits to existing FilmCraft files (registration, Essential Sound, the editor window,
translations) are only in [`../patches/voice-isolation.patch`](../patches/voice-isolation.patch),
which is the source of truth. Install with the scripts rather than copying these files.
