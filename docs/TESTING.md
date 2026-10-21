# Testing

The extension has 19 tests of its own plus an opt-in benchmark. Because it registers itself
with FilmCraft's effect registry, FilmCraft's own generic tests also run against it. The whole set
was run on Windows 11 with Rust 1.95.0, against FilmCraft commit `70f0fc7a` (2026-10-20).

## Results

| Check | Command (in the FilmCraft folder) | Result |
|---|---|---|
| Formatting (FilmCraft's `rustfmt.toml`) | `cargo fmt -p <crate> --check` for the 5 touched crates | clean |
| Clippy, warnings as errors, all targets | `cargo clippy -p filmcraft-audio-dsp -p filmcraft-project -p filmcraft-render -p filmcraft-engine -p filmcraft-ui-egui --all-targets -- -D warnings` | clean ¹ |
| DSP crate (effect + registry stress tests) | `cargo test -p filmcraft-audio-dsp` | 133 passed |
| Project + render (effect definitions, DSP mappings) | `cargo test -p filmcraft-project -p filmcraft-render` | 65 + 197 passed |
| Whole engine crate | `cargo test -p filmcraft-engine` | 659 passed |
| UI crate unit tests (incl. i18n completeness) | `cargo test -p filmcraft-ui-egui --lib` | 196 passed |
| Headless UI: Voice Isolation | `cargo test -p filmcraft-ui-egui --test voice_isolation_ui` | 3 passed |
| Headless UI: existing editors and Essential Sound | `--test audio_fx_editor_ui --test essential_sound_ui` | 3 + 1 passed |
| Dependency layering | `cargo xtask layers` | ok |
| Asset licensing | `cargo xtask assets` | ok |
| Web build | `cargo xtask wasm` (needs `rustup target add wasm32-unknown-unknown`) | ok (49 crates) |
| Install scripts | see [below](#install-scripts) | all scenarios pass |

¹ With `-A clippy::nonminimal_bool -A clippy::collapsible_match`. Clippy 1.95 flags those two
lints in four places in FilmCraft files the extension doesn't touch (`panels/file_dialogs.rs`,
`panels/timeline.rs`). Every file the extension adds or changes is clean without the allowances.

`scripts/verify` runs all of the above except the whole-engine and wasm runs.

## What the extension's tests check

**DSP** (`crates/audio-dsp/src/effects/voice_isolation.rs`, synthetic speech: a gliding harmonic
voice with formants, syllables and pauses):

| Test | Proves |
|---|---|
| `zero_amount_is_a_delayed_identity` | At Amount 0 the output is the input delayed by exactly `latency()`, at 8, 16, 44.1, 48, 96 and 192 kHz. |
| `voice_plus_removed_background_reconstructs_the_input` | Isolated Voice + Removed Background = the input, sample by sample. |
| `removes_steady_noise_and_keeps_the_voice` | Background ≥ 25 dB down once a word's tail is over; voice level within 2 dB; noise during speech reduced. |
| `amount_and_floor_scale_the_reduction` | Amount scales the reduction in dB; Background Floor caps it. |
| `center_focus_removes_side_panned_sound` | A hard-left melody drops ≥ 8 dB more with Center Focus 100 %, while the centred voice is unchanged. |
| `clean_studio_dialogue_is_left_alone` | Dialogue over a −75 dBFS floor changes by less than 1 dB. |
| `meter_reports_voice_and_pitch` | Voice is detected in speech and not in noise; the pitch estimate tracks the talker. |
| `hostile_input_and_construction` | NaN/±∞ samples don't poison the effect; sample rates 0, −48 k, NaN, ∞, 1, 1e9 and 0–1000 channels construct and run; mismatched channel lengths are safe. |
| `digital_silence_is_not_learned_as_the_noise_floor` | Noise right after 2 s of digital silence (a clip start) is still removed. |
| `output_gain_and_reset` | Output Gain is exact; `reset()` restores the initial state. |
| `voice_isolation_realtime` (ignored, benchmark) | 60 s of stereo at 48 kHz in 2.0 s: **30× real time** on one core (release build). |

FilmCraft's registry-wide tests also run against Voice Isolation:
`block_size_independence_with_param_changes` (identical output whatever block sizes the host
uses, parameters changing mid-stream), `zero_length_and_single_sample_blocks`,
`extreme_settings_stay_finite_and_denormal_free` (every parameter at min and at max, ±1e6
spikes, 40 s of silent tail with no NaN or denormals), `reset_restores_initial_behaviour`,
`registry_is_consistent` and `realtime_factor_sanity`. On the project and render side,
`every_audio_effect_has_dsp` and `direct_mappings_match_dsp_parameters` check that the
Effect Controls parameters match the DSP's ids, ranges, defaults and choices.

**Engine** (`crates/engine/src/voice_isolation_tests.rs`, through FilmCraft's real
commands and render path):

| Test | Proves |
|---|---|
| `listed_in_noise_reduction_and_applicable_by_id_and_name` | `effects.list` places it in *Audio Effects ▸ Noise Reduction/Restoration*; applies by id and by name; works as a mixer insert; undo removes it. |
| `hostile_parameters_are_refused_or_clamped_never_crash` | Wrong types, unknown ids and unknown choice names are refused with an error; absurd numbers (1e30, −1e30, f64::MAX) render finite audio. |
| `latency_is_compensated_sample_exactly` | With the effect at Amount 0, the clip renders identical to the clip without it (to 1e-4), in requests of 2 s, 4800 and 1031 samples, and after a seek. |
| `isolates_the_voice_and_the_background_is_the_rest` | Through the render path: background in pauses **−46.5 dB**, voice **−0.39 dB**; voice + removed background = the original. |
| `essential_sound_isolate_voice_drives_the_effect` | The switch adds the effect in the right order (after Reduce Rumble, before Reduce Noise); slider 8 = 80 %; clamping; the Repair switch bypasses it; off removes it. |
| `projects_round_trip_and_older_settings_load` | Settings survive save/load; Essential Sound settings saved before the extension existed still load. |

**UI** (`crates/ui-egui/tests/voice_isolation_ui.rs`, the real app driven headlessly via
automation ids):

| Test | Proves |
|---|---|
| `clip_editor_view_presets_listen_and_reset` | Effect Controls ▸ Edit… opens the editor with every control; a preset sets four parameters in **one** undo step; Listen switches the output; a slider drag is one undo step; Reset restores the defaults; Close closes. |
| `track_editor_from_the_mixer_slot` | The editor opens from a mixer insert and presets write to the insert. |
| `essential_sound_isolate_voice_switch` | Essential Sound ▸ Dialogue ▸ Repair shows Isolate Voice; clicking it adds the effect. |

The screenshots in [images/](images/) were taken by these tests (`FILMCRAFT_UI_SNAPSHOT_DIR=<dir>`).

## Install scripts

Run against throwaway FilmCraft checkouts with `install.ps1` / `uninstall.ps1` (Windows
PowerShell 5.1) and `install.sh` / `uninstall.sh` (bash):

| Scenario | Expected and observed |
|---|---|
| Clean checkout, LF line endings | Installs; every patched file matches the reference byte for byte. |
| Clean checkout, CRLF (`core.autocrlf=true`) | Installs with consistent CRLF endings; uninstall returns a completely clean tree. |
| Run twice | "already installed", exit 0. |
| Folder missing / not FilmCraft / not a git checkout | Clear error, exit 1. |
| Uncommitted edit in a patched file | Refused; **nothing changed**; the file is named. |
| Committed edit adjacent to a patched line (merge conflict) | Trial merge in a scratch worktree detects it; refused; **nothing changed**; no worktree left behind. |
| Committed edit two lines from a change | Plain apply fails, three-way merge succeeds; the user's edit is kept. |
| Uninstall after that | Extension removed, the user's edit kept, tree back at the user's commit. |
| Uninstall after editing the extension's own code | Refused; nothing changed. |

## Running on Windows: two things to know

- **LNK1140** (`too many modules for program database`): FilmCraft's large UI test binaries exceed
  the MSVC linker's PDB limit in a debug build, including FilmCraft's *own* UI tests. Build tests
  without debug info: `CARGO_PROFILE_DEV_DEBUG=0`. The verify scripts do this on Windows.
- **Disk space:** a debug build with debug info and tests took about 17 GB; with
  `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` the same set took 2.7 GB.

## A note on FilmCraft's effect-chain cache

`render::audio_fx` keeps effect chains in a process-wide cache keyed by clip id, effect chain,
sample rate and width. Clip ids start from the same values in every `Session`, so tests running
in parallel can, rarely, pick up each other's chains when they render the same clip id with the
same effect chain at the same sample positions. In the app there is one session, so this never
matters there. Tests that compare samples exactly therefore use either whole-range requests
(always a fresh chain) or an effect chain no other test uses.
