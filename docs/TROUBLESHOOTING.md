# Troubleshooting

Find your symptom below. Every entry gives the cause and the fix.

- [Installing](#installing)
- [Building](#building)
- [In FilmCraft](#in-filmcraft)
- [Sound quality](#sound-quality)
- [Tests](#tests)
- [Uninstalling](#uninstalling)

## Installing

### `running scripts is disabled on this system` (PowerShell)

Windows blocks unsigned scripts by default. Run the script through a one-off bypass, which
doesn't change any setting:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Clone
```

### `git was not found`

Install Git (Windows: <https://git-scm.com/download/win>), then **open a new terminal** so it
picks up the new `PATH`.

### `No FilmCraft checkout at '…'`

The script looks in `~/Developer/filmcraft` unless told otherwise. Either add `-Clone` /
`--clone` to download FilmCraft there, or point it at your checkout:
`scripts\install.ps1 -FilmCraftDir D:\src\filmcraft` (bash: `scripts/install.sh /path/to/filmcraft`).

### `'…' is not a git checkout`

The patch is applied with git, so FilmCraft must be a `git clone`, not a ZIP download. Clone it:
`git clone https://github.com/storytold/filmcraft.git`.

### `git clone failed` / `RPC failed; curl 56 Recv failure: Connection was reset`

The connection dropped during the download, which is common on unstable networks with a large
repository. Retry, or clone in two smaller steps:

```bash
git clone --filter=blob:none https://github.com/storytold/filmcraft.git ~/Developer/filmcraft
cd ~/Developer/filmcraft
git switch -c voice-isolation 70f0fc7a75f2681bc18302e67eaf0c9d41c68873
```

If that also fails: `git config --global http.version HTTP/1.1` and try again.

### Patch does not apply

The scripts test the patch before touching anything. When it doesn't fit they stop with
"Nothing was changed" and name the files involved. Your checkout is exactly as it was. Causes:

1. **Uncommitted edits of your own** in a file the patch changes (`does not match index`).
   Commit them (`git commit -am "my changes"`) and run the installer again. It will merge the
   extension around your edits. (Don't use `git stash`: `git stash pop` can't put your edits
   back on top of the patched files.)
2. **A conflict** (`conflict: <file>`): your committed edits, or a newer FilmCraft, changed the
   same lines as the extension. Install on a fresh checkout of the tested commit, or follow
   [Newer FilmCraft versions](INSTALL.md#newer-filmcraft-versions) and resolve the conflict in git.
3. **Windows line endings** (`core.autocrlf=true`) are handled automatically and are not a cause.

By hand, the equivalent dry run is `git apply --check <patch>`; nothing is changed if it fails.

### `Voice Isolation is already installed`

It is already in that checkout. To reinstall, run `scripts/uninstall` first.

## Building

### `package 'filmcraft-…' cannot be built because it requires rustc 1.95 or newer`

Your Rust is too old. Run `rustup update stable`. To keep an older default, run
`rustup toolchain install 1.95.0` and either prefix commands (`cargo +1.95.0 run -p filmcraft`) or
run `rustup override set 1.95.0` once inside the FilmCraft folder.

### `linker 'link.exe' not found` (Windows)

The Visual Studio C++ build tools are missing. Install *Build Tools for Visual Studio* with the
**Desktop development with C++** workload, then open a new terminal.

### `LINK : fatal error LNK1104: cannot open file '…\build_script_build-….exe'` (Windows)

The build path is longer than Windows' 260-character limit. This usually happens when FilmCraft
sits in a deep folder. Either move the checkout to a short path (`C:\src\filmcraft`) or put
the build output somewhere short:

```powershell
$env:CARGO_TARGET_DIR = "$env:LOCALAPPDATA\fc-target"
cargo run -p filmcraft --release
```

(`scripts\verify.ps1` does this for you when the path is long.) Antivirus software scanning
freshly linked files can cause the same error. Retrying usually works.

### `LINK : fatal error LNK1140: too many modules for program database` (Windows, tests)

The debug build of FilmCraft's large UI test programs exceeds the MSVC linker's limit. This is not
specific to the extension; FilmCraft's own UI tests hit it too. Build tests without debug info:

```powershell
$env:CARGO_PROFILE_DEV_DEBUG = "0"
cargo test -p filmcraft-ui-egui --test voice_isolation_ui
```

The verify scripts set this automatically on Windows. The app itself (`cargo run`) is unaffected.

### `failed to fetch` / `failed to get 'craft-ui' as a dependency`

Cargo downloads two small FilmCraft libraries from GitHub on the first build. On a flaky
connection, let Cargo use your git instead of its built-in client:

```powershell
$env:CARGO_NET_GIT_FETCH_WITH_CLI = "true"     # bash: export CARGO_NET_GIT_FETCH_WITH_CLI=true
```

### `failed to run custom build command for 'alsa-sys'` / `gtk-sys` (Linux)

The system headers are missing. Debian/Ubuntu:
`sudo apt install pkg-config libasound2-dev libgtk-3-dev libxkbcommon-dev libwayland-dev libx11-dev`.

### `No space left on device` / the disk fills up

A default debug build of FilmCraft with its tests takes about 17 GB. Without debug info and
incremental caches it took 2.7 GB:

```powershell
$env:CARGO_PROFILE_DEV_DEBUG = "0"; $env:CARGO_INCREMENTAL = "0"     # bash: export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
```

`cargo clean` in the FilmCraft folder frees everything (so does deleting the folder
`CARGO_TARGET_DIR` points to). A failed build caused by a full disk can leave broken files, so run
`cargo clean` and rebuild after freeing space.

### The build is slow

The first build compiles FilmCraft's codecs and UI (10–30 minutes); after that only changed
crates rebuild. `cargo run -p filmcraft` (debug) builds faster than `--release`.

## In FilmCraft

### Voice Isolation isn't in the Effects panel

You're running a FilmCraft built without the extension. Common causes: an installed FilmCraft
release opened from the Start menu or Dock, instead of the one you built; or the build ran before
the patch was applied. Run `cargo run -p filmcraft --release` from the patched checkout. Also
check the search box: type `voice`, and make sure the panel isn't filtered to video effects.

### There's no "Isolate Voice" in Essential Sound

Repair only exists for **Dialogue** clips. Select the clip and click **Dialogue** at the top of
Essential Sound. If several clips are selected and their types differ, the Edit tab shows nothing
to edit; select clips of one type.

### No "Edit…" button for the effect in Effect Controls

The button is on the effect's own row (the header with the effect name). Scroll Effect Controls
if the clip has many effects; fold the others with their twirl arrows.

### The editor says "No audio at the playhead."

The 3 s window around the playhead is silent. Move the playhead over the speech in the clip.

### The editor says "This clip has no audio to preview."

The clip is a nested sequence or has no audio stream. Nested sequences aren't analysed by the
preview. Use the effect on the clips inside the nest, or on the nest's track in the mixer.

### The editor view doesn't change while I drag a slider / during playback

That's deliberate, so dragging and playback stay smooth. The view updates when you release the
slider or stop playback.

### Everything I play is silent or sounds like distant noise

*Output* (the editor's *Listen*) is set to **Removed Background**. Set it back to **Isolated
Voice**.

### Audio is out of sync with the picture

It shouldn't be. FilmCraft compensates the effect's 64 ms delay sample-exactly, and a test checks
this. If you hear drift, check the clip's other effects by disabling Voice Isolation (the *fx*
toggle on its row). If the drift remains, the cause is elsewhere.

## Sound quality

| You hear | Try |
|---|---|
| The voice sounds thin, hollow or "underwater" | Lower *Sensitivity* (50 %), lower *Amount*, keep *Preserve Formants* on. |
| Starts or ends of words are cut | Lower *Sensitivity*. Check *Removed Background* for word fragments. |
| Background "breathes" or pumps between words | Raise *Background Floor* (−24 dB) so the background never fully disappears, or lower *Sensitivity*. |
| Twinkly, watery artefacts in the background | Lower *Amount* a little or raise *Background Floor*. |
| Music still audible under the voice | *Music Bed* preset; raise *Center Focus* if the music is in stereo. A centred mono music bed can only be reduced. |
| The voice disappears on a two-microphone recording | Set *Center Focus* to 0 %. |
| Remaining low hum | Add Essential Sound *DeHum* (or the DeHummer effect). |
| Remaining room echo on the voice | Add *Reduce Reverb* (DeReverb) after Voice Isolation. |
| Dialogue too quiet after cleaning | Raise *Output Gain*, or use Essential Sound's *Loudness ▸ Auto-Match*. |
| The first half-second after a cut sounds different | After a jump, the effect re-learns the background from the 1.5 s before the playhead, which only matters when the background changes right at the cut. It is identical in export. |

## Tests

### A test fails with `cannot find value 'VOICE_ISOLATION_OUTPUTS'` or similar

The patch was applied partially. Uninstall, reset the files (see
[Patch does not apply](#patch-does-not-apply)) and install again.

### `untranslated tl! strings: [...]` or `untranslated Japanese strings`

A FilmCraft newer than the tested one added UI strings, or a translation entry was lost in a
merge. Add the listed strings to `crates/ui-egui/src/i18n/es.tsv`, `ja.tsv` and `zh-cn.tsv` (tab
separated: empty context, English, translation).

### Clippy fails in `file_dialogs.rs` or `timeline.rs`

Those are FilmCraft files the extension doesn't touch: newer clippy versions flag lints in them.
The verify scripts allow the two lints that clippy 1.95 reports there
(`nonminimal_bool`, `collapsible_match`). A different clippy may report others in FilmCraft's own
code. Only findings in the files listed in [INSTALL.md](INSTALL.md#3-install-by-hand) concern
the extension.

### A sample-exact test fails only when run together with other tests

FilmCraft's render path caches effect chains process-wide by clip id, and clip ids repeat across
test sessions. The extension's tests are written to be immune to this (see
[TESTING.md](TESTING.md#a-note-on-filmcrafts-effect-chain-cache)); if you write new ones, use a
distinct effect chain or whole-range requests.

## Uninstalling

### `the extension's changes could not be reversed automatically`

You edited the same files afterwards. Look at `git status` and `git diff`. To drop *all* local
changes to FilmCraft's sources, run `git checkout -- crates` followed by
`git clean -fd crates`. That removes your own edits there too, so commit anything you want to keep
first.
