# How Voice Isolation works

- [Signal flow](#signal-flow)
- [The four cues](#the-four-cues)
- [From mask to sound](#from-mask-to-sound)
- [Integration points](#integration-points)
- [What changed from the first draft](#what-changed-from-the-first-draft)
- [Limits](#limits)

## Signal flow

```text
 input channels ──► STFT (√Hann, 43 ms at 48 kHz, 75 % overlap) ──► per-channel spectra
                                                                     │
                    channel sum ◄────────────────────────────────────┘
                        │
    ┌───────────────────┼───────────────────────────┬────────────────────────┐
    ▼                   ▼                           ▼                        ▼
 noise tracker      voicing & pitch            speech-band SNR        L/R difference
 (min. statistics)  (autocorrelation)                                 (stereo only)
    │                   │                           │                        │
    ▼                   ▼                           ▼                        ▼
 MMSE-LSA gain      harmonic comb      ┌── voice activity (+ look-ahead,     centre focus
 (broadband)        (fine structure)   │   60 ms hold, 30 ms release)        (fine structure)
    │                   │              │            │                        │
    └─────────┬─────────┴──────────────┘            │                        │
              ▼                                     ▼                        │
     temporal + frequency smoothing ◄── gate: between words → Background Floor
              │                                                              │
              └───────────────── × fine structure ◄──────────────────────────┘
                                   │
            Amount (g^amount) · floor · Preserve Formants · Listen (g or 1 − g)
                                   │
                one gain per bin, applied to every channel ──► inverse STFT ──► Output Gain
```

One mask is computed from the sum of all channels and applied to each channel, so the stereo (or
5.1) image stays where it was. Latency is the frame plus two look-ahead hops: 3072 samples
(64 ms) at 48 kHz. FilmCraft compensates it.

## The four cues

**1. Stationary background: minimum-statistics noise tracking + MMSE-LSA.** For every frequency
bin, the smoothed power is tracked and its minimum over a sliding 1.5 s window (eight
sub-windows) estimates the noise. Nothing has to be "learned" from a silent section. A
decision-directed a-priori SNR (α = 0.98) feeds the Ephraim–Malah *log-spectral amplitude* gain.
Compared with a plain Wiener gain it costs speech far less at moderate SNR, and the residual noise
it leaves is smooth instead of "musical". *Sensitivity* sets the over-subtraction (1× to 2.5×).

Frames that are partly or wholly digital silence (a clip's start, the pre-roll before it) are
not taken as noise observations. Otherwise the tracker would learn "the background is silence"
and let the real background through for a second and a half at every clip start.

**2. Voicing and pitch.** The power spectrum, band-limited to 60 Hz–1.5 kHz, is turned into an
autocorrelation with one inverse FFT and normalised by the window's own autocorrelation. Its
highest peak between 70 and 400 Hz gives the *voicing* strength (0–1) and the pitch, refined by
parabolic interpolation. In voiced frames, a soft harmonic comb lowers the energy *between* the
voice's harmonics below 4 kHz. Picking the highest peak errs towards half the true pitch,
never double. A half-pitch comb still passes every real harmonic, so a pitch error can't carve
holes into the voice.

**3. Voice activity.** Voicing and the 200 Hz–8 kHz SNR combine into a per-frame speech-presence
probability. Unvoiced consonants (s, f, sh) count through the SNR term. The gate:

- opens up to two hops (21 ms) **before** an onset, using look-ahead, so first consonants are not
  clipped;
- **holds** for 60 ms after the last voiced frame (word endings, short tails);
- then **releases** with a 30 ms time constant down to the *Background Floor*.

Between words, everything is pulled down to the floor. In the test signal this takes the
background 25+ dB down within 100 ms of a word ending, and 46 dB down through FilmCraft's render
path with the default settings.

**4. Centre focus (stereo).** For each bin, the left/right difference is compared with the sum.
Sound that is identical in both channels (a centred voice) passes; sound that is only on one
side, or decorrelated, is lowered in proportion to *Center Focus*.

## From mask to sound

- The **broadband** gain (LSA × gate × a 40–90 Hz low-cut prior) is smoothed over time (8 ms
  attack, 25 ms release) and across frequency.
- The frequency smoothing is **speech-aware**. Between words it averages neighbouring bins, which
  removes isolated "musical noise" peaks. While someone speaks it keeps every gain *peak*: those
  peaks are the voice's harmonics, and averaging them with the low gains between harmonics would
  dull the voice by 1–2 dB.
- The **fine structure** (harmonic comb × centre focus) is applied after the smoothing, for the
  same reason.
- **Preserve Formants** sets a minimum gain of −14 dB in 200 Hz–4 kHz while the gate is open.
- **Amount** scales the reduction in decibels (`gain^amount`), so 50 % is half the dB, never a
  dry/wet mix that would cap the reduction.
- **Listen ▸ Removed Background** outputs `1 − gain`, so voice + background = the input,
  sample-exactly.

Everything runs in place on preallocated buffers: no allocation, no locks, no `unsafe`, no
panics. Non-finite input samples are replaced by silence before they can reach recursive state.
Hostile sample rates (0, negative, NaN, 1 GHz) and channel counts (0, 1000) construct a working
effect.

## Integration points

FilmCraft compiles its effects in; these are the places the extension plugs into. Use this list
if you ever need to port the patch to a much newer FilmCraft by hand.

| File | Change | Why |
|---|---|---|
| `crates/audio-dsp/src/effects/voice_isolation.rs` | **new**: `VoiceIsolator` implementing `AudioEffect` | The DSP. Dependency-free (layer L1), wasm-clean. |
| `crates/audio-dsp/src/effects/mod.rs` | `pub mod`, `pub use`, one `entry!("voice_isolation", …, Restoration, VoiceIsolator)` | The DSP registry: FilmCraft's registry-wide stress tests now cover the effect too. |
| `crates/project/src/effect/audio.rs` | one `audio("voice_isolation", "Voice Isolation", A_NOISE, …)` definition + `VOICE_ISOLATION_OUTPUTS` | Effects panel entry, Effect Controls rows, keyframes, project files, `effects.*` commands. |
| `crates/project/src/effect.rs` | re-export `VOICE_ISOLATION_OUTPUTS` | Used by the editor. |
| `crates/render/src/audio_fx.rs` | one `DIRECT` row: `("voice_isolation", "voice_isolation", 1.5)` | Project parameters map one-to-one onto the DSP; 1.5 s pre-roll after a seek. The render path then handles keyframes (per 256-sample block on an absolute grid), latency compensation, chain caching, mixer inserts and export. |
| `crates/project/src/essential.rs` | `Repair::isolate` slot, effect order, clamping, `effects()` mapping | Essential Sound ▸ Repair ▸ Isolate Voice. `#[serde(default)]` keeps older projects loading. |
| `crates/ui-egui/src/panels/essential_sound.rs` | one `slot(…"repair.isolate"…)` row | The switch and slider. |
| `crates/ui-egui/src/panels/audio_fx_editor.rs` | `has_editor`, the `voice` view, `commit_many` (multi-parameter undo step) | The editor window. |
| `crates/ui-egui/src/i18n/*.tsv` | 25 strings × 9 languages | FilmCraft's i18n tests require every UI string to be translated. |
| `crates/engine/src/voice_isolation_tests.rs`, `crates/ui-egui/tests/voice_isolation_ui.rs` | **new** tests | See [TESTING.md](TESTING.md). |

## What changed from the first draft

The draft this started from (three Gemini-generated snippets) had the right idea: an effect
with these six controls, a separate DSP module and latency compensation. But it was written
against a FilmCraft API that doesn't exist:

| Draft | Problem | Now |
|---|---|---|
| `EffectDef { category: EffectCategory::Audio("…"), params: &[ParamDef::float(…)] }` | FilmCraft has no `EffectCategory` or `ParamDef::float`; definitions are built with `audio(id, name, folder, vec![…])` and `fs`/`b`/`ch` helpers. | A real definition in `effect/audio.rs`, in the existing *Noise Reduction/Restoration* folder. |
| `"voice_isolation" => { … VoiceIsolator::new(…) … }` match arm with a per-sample closure | FilmCraft has no such match. Effects are mapped in `render::audio_fx` and processed by the shared chain. Evaluating six keyframed parameters for **every sample** would also be very slow. | One `DIRECT` row. The host sets parameters per 256-sample block on an absolute grid (identical output however playback cuts the requests). |
| `filmcraft_wiring::render_clip_with_latency_compensation` | Doesn't exist, and would duplicate what the render path already does. | `AudioEffect::latency()` reports the delay; the host compensates it. A test proves the compensation is sample-exact. |
| `if let Ok(isolator) = VoiceIsolator::new(…)` | A failed construction silently left the clip unprocessed with no error. | Construction can't fail: hostile inputs are mapped to safe values. |
| `SpeechEnhancerModel` export (`lib.rs` snippet) | Implied a neural model that wasn't there. | No model. The docs say plainly what the DSP does and doesn't do. |
| *Amount* as a dry/wet mix (implied) | At 85 % a mix can never remove more than 16.5 dB, whatever the floor. | *Amount* scales the reduction in dB; *Background Floor* is the real limit. |
| `noise_floor_db`, `output_gain_db`, `stereo_center_focus` | Didn't follow FilmCraft's parameter naming. | `floor`, `output`, `center_focus` (FilmCraft style), plus `listen`. |
| Center focus default 35 % | Can eat the voice on two-microphone recordings. | 30 %, with the caveat documented and a 0 % recipe. |
| No UI beyond generic sliders, no translations, no tests | | Essential Sound switch, editor window, 9 languages, 19 automated tests on top of FilmCraft's own. |

## Limits

Be realistic about what signal processing can do compared with a cloud neural model such as
ElevenLabs Voice Isolator:

- **Music under dialogue.** Removed between words, reduced during speech (between the voice's
  harmonics, and off-centre with Center Focus). But music that overlaps the voice in pitch and
  position stays partly audible while the person talks. A neural model does better here.
- **Other voices.** A second talker, or a crowd close to the mic, counts as voice. It removes
  *background*, not other people.
- **Reverb on the voice.** Only the reverb tail between words is removed; use DeReverb for the
  rest.
- **Very low SNR** (background as loud as the voice): the voice survives but artefacts become
  audible. Use the *Gentle* preset, or a higher Background Floor.
- **Sudden loud non-stationary sounds** (a door slam, a dog bark) during speech pass partly.
  Between words they're gated.
- **Preview in the editor** uses the settings at the playhead for the whole 3 s window (playback
  follows keyframes exactly).

A neural back end could slot in behind the same effect, parameters and UI later (for example a
small permissively licensed speech-enhancement model run through FilmCraft's `speech` crate). The
DSP would remain the fallback.
