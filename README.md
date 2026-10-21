# BackCraft — Voice Isolation for FilmCraft

One switch that keeps the dialogue and removes everything around it: room tone, hiss, HVAC,
hum, rumble, traffic, crowd walla and, partly, music beds. It is a native FilmCraft audio effect,
so it works everywhere FilmCraft audio effects work:

- **Essential Sound ▸ Dialogue ▸ Repair ▸ Isolate Voice**: one switch and one slider, the
  quickest way in.
- **Effects panel ▸ Audio Effects ▸ Noise Reduction/Restoration ▸ Voice Isolation**: drag it
  onto a clip, then tweak and keyframe it in Effect Controls.
- **Voice Isolation editor** (Effect Controls ▸ *Edit…*): a before/after view of 3 s around the
  playhead, a voice-activity strip, presets (Gentle / Balanced / Strong / Music Bed), an
  *Isolated Voice ↔ Removed Background* listen switch and every setting.
- **Audio Track Mixer insert**: isolate a whole dialogue track.
- **Agents and scripts**: everything is a FilmCraft command (`effects.apply`,
  `effects.setParam`, `essentialSound.set`, `mixer.addInsert`) with automation ids for every
  control.

Playback, export, undo, keyframes, project files and latency compensation all use FilmCraft's
normal effect path, so nothing behaves differently from a built-in effect.

> **How it compares with ElevenLabs Voice Isolator.** ElevenLabs runs a large neural network in
> the cloud. This extension is signal processing that runs locally, in real time (about 30× faster
> than real time on one CPU core), with no upload, account or model download. On noise, hum, room
> tone and crowd it does a clean job. On loud music under dialogue it reduces the music but does
> not remove it completely the way a neural model can. See
> [docs/HOW-IT-WORKS.md](docs/HOW-IT-WORKS.md#limits).

![The Voice Isolation editor in FilmCraft](docs/images/editor.png)

## Quick start

You need **Git**, **Rust 1.95 or newer** (`rustup update stable`) and, on Windows, the
**Visual Studio C++ Build Tools**. Full steps: [docs/INSTALL.md](docs/INSTALL.md).

**Windows (PowerShell):**

```powershell
cd "C:\Users\<you>\filmcraft extensions\backcraft"
powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Clone -Test
cd $HOME\Developer\filmcraft
cargo run -p filmcraft --release
```

**macOS / Linux:**

```bash
cd backcraft
scripts/install.sh --clone --test
cd ~/Developer/filmcraft && cargo run -p filmcraft --release
```

`-Clone` / `--clone` downloads FilmCraft to `~/Developer/filmcraft` at the version this extension
was tested with. If you already have a checkout, pass its path instead:
`scripts\install.ps1 -FilmCraftDir D:\src\filmcraft`. `-Test` / `--test` builds everything and
runs the extension's checks; the first build takes 10–30 minutes.

Then in FilmCraft, select a dialogue clip and in **Essential Sound** click **Dialogue**, open
**Repair** and switch on **Isolate Voice**. That's it. The [user guide](docs/USER-GUIDE.md)
covers everything else.

## What's in this folder

| Path | What it is |
|---|---|
| [`patches/voice-isolation.patch`](patches/voice-isolation.patch) | The extension: one git patch against FilmCraft (new files and small edits). |
| [`source/`](source/) | Readable copies of the new source files (the patch is the source of truth). |
| [`scripts/`](scripts/) | `install`, `uninstall` and `verify` for PowerShell and bash. |
| [`docs/INSTALL.md`](docs/INSTALL.md) | Setup, step by step, including manual install, updating and uninstalling. |
| [`docs/USER-GUIDE.md`](docs/USER-GUIDE.md) | Using it in FilmCraft: every control, presets, workflows, automation. |
| [`docs/HOW-IT-WORKS.md`](docs/HOW-IT-WORKS.md) | The algorithm, the FilmCraft integration, design decisions and limits. |
| [`docs/TESTING.md`](docs/TESTING.md) | What is tested, how to run it, and the latest results. |
| [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md) | Every error you might see, with the fix. |
| [`docs/images/`](docs/images/) | Screenshots of the editor and panels (taken by the UI tests). |

## Compatibility

- **FilmCraft:** tested at commit `70f0fc7a` (2026-10-20, workspace version 0.10.0). Newer
  versions usually still take the patch; see [Newer FilmCraft versions](docs/INSTALL.md#newer-filmcraft-versions).
- **Platforms:** anywhere FilmCraft builds: Windows, macOS, Linux, and the web build (FilmCraft's
  `cargo xtask wasm` check passes with the extension).
- **Projects:** a project that uses Voice Isolation still opens in a FilmCraft without the
  extension. The effect is kept in the file but not applied there.
- **Sample rates:** 8 kHz to 384 kHz; mono, stereo and 5.1 (one mask for all channels, so the
  image is preserved).

## License

MIT OR Apache-2.0, the same as FilmCraft, so the patch can be contributed upstream unchanged.
The code is original. No third-party code, models or assets are included.
