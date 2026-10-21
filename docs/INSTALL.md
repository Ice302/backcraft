# Installing Voice Isolation

FilmCraft has no plug-in loader: every effect is compiled into the app. So "installing" this
extension means adding its source to a FilmCraft checkout (one git patch) and building FilmCraft.
The install scripts do that for you; this page explains each step and the manual way.

- [1. Prerequisites](#1-prerequisites)
- [2. Install with the script](#2-install-with-the-script)
- [3. Install by hand](#3-install-by-hand)
- [4. Build and run FilmCraft](#4-build-and-run-filmcraft)
- [5. Check that it works](#5-check-that-it-works)
- [Newer FilmCraft versions](#newer-filmcraft-versions)
- [Uninstalling](#uninstalling)
- [Contributing it upstream](#contributing-it-upstream)

## 1. Prerequisites

| Tool | Why | Get it |
|---|---|---|
| Git | download FilmCraft, apply the patch | Windows: <https://git-scm.com/download/win> · macOS: `xcode-select --install` · Linux: your package manager |
| Rust **1.95 or newer** | FilmCraft's minimum (`rust-version = "1.95"`) | <https://rustup.rs>, then `rustup update stable` |
| C/C++ build tools | the Rust linker on Windows; a few FilmCraft dependencies | Windows: *Visual Studio Build Tools* with **Desktop development with C++** · macOS: Xcode Command Line Tools (`xcode-select --install`) · Linux: a C toolchain (`build-essential` on Debian/Ubuntu) |
| Linux only | audio (ALSA), file dialogs (GTK 3) and windowing headers for the desktop app | Debian/Ubuntu: `sudo apt install pkg-config libasound2-dev libgtk-3-dev libxkbcommon-dev libwayland-dev libx11-dev` |
| Free disk space | FilmCraft's build output (`target/`) | About 17 GB for a default debug build with tests; under 3 GB with `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_INCREMENTAL=0` set (see [TROUBLESHOOTING](TROUBLESHOOTING.md#no-space-left-on-device--the-disk-fills-up)) |

Check Rust:

```text
rustc --version      # must print 1.95.0 or newer
```

If it prints an older version, run `rustup update stable`. If you need to keep an older default
for other projects, install 1.95 beside it instead: `rustup toolchain install 1.95.0`, then use
`cargo +1.95.0 …` for every FilmCraft command (or run `rustup override set 1.95.0` inside the
FilmCraft folder once).

## 2. Install with the script

**Windows (PowerShell):**

```powershell
cd "C:\Users\<you>\filmcraft extensions\backcraft"
powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Clone
```

**macOS / Linux / Git Bash:**

```bash
cd backcraft
chmod +x scripts/*.sh        # once, if the files lost their executable bit
scripts/install.sh --clone
```

What the script does:

1. Checks for git and Rust ≥ 1.95. A missing or old Rust only produces a warning, because you
   need it for building, not for installing.
2. With `-Clone` / `--clone`, and only if the folder does not exist yet, clones
   <https://github.com/storytold/filmcraft> into `~/Developer/filmcraft` and creates a branch
   `voice-isolation` at the tested commit `70f0fc7a75f2681bc18302e67eaf0c9d41c68873`.
3. Checks that the folder is a FilmCraft git checkout, and that the extension is not already in it.
4. Applies `patches/voice-isolation.patch`, after dry runs. If a plain apply doesn't fit (a newer
   FilmCraft, or edits you committed), it tries a three-way merge, first in a temporary git
   worktree. If anything would conflict, it stops and **changes nothing**, naming the files.
5. Lists the changed files and prints the next steps.

Options:

| PowerShell | bash | Meaning |
|---|---|---|
| `-FilmCraftDir <path>` | `<path>` (positional) | Use an existing checkout somewhere else. |
| `-Clone` | `--clone` | Clone FilmCraft first if the folder doesn't exist. |
| `-Test` | `--test` | Run `verify` afterwards (builds FilmCraft and runs all checks). |

The script never touches anything outside the FilmCraft folder and never commits. Your checkout
just has uncommitted changes that you can review with `git diff`.

## 3. Install by hand

```bash
git clone https://github.com/storytold/filmcraft.git ~/Developer/filmcraft
cd ~/Developer/filmcraft
git switch -c voice-isolation 70f0fc7a75f2681bc18302e67eaf0c9d41c68873
git apply --check /path/to/backcraft/patches/voice-isolation.patch   # dry run: no output = fits
git apply /path/to/backcraft/patches/voice-isolation.patch
git status --short
```

You should see these files:

```text
 M crates/audio-dsp/src/effects/mod.rs           registers the effect
 M crates/engine/src/lib.rs                      registers the engine tests
 M crates/project/src/effect.rs                  re-exports the Output choices
 M crates/project/src/effect/audio.rs            the effect definition (Effects panel, Effect Controls)
 M crates/project/src/essential.rs               Essential Sound ▸ Repair ▸ Isolate Voice
 M crates/render/src/audio_fx.rs                 project parameters → DSP (one line)
 M crates/ui-egui/src/i18n/*.tsv                 translations (9 languages)
 M crates/ui-egui/src/panels/audio_fx_editor.rs  the Voice Isolation editor window
 M crates/ui-egui/src/panels/essential_sound.rs  the Isolate Voice switch
?? crates/audio-dsp/src/effects/voice_isolation.rs  the DSP
?? crates/engine/src/voice_isolation_tests.rs       engine integration tests
?? crates/ui-egui/tests/voice_isolation_ui.rs       headless UI tests
```

If `git apply --check` complains, see [TROUBLESHOOTING](TROUBLESHOOTING.md#patch-does-not-apply).
The scripts handle those cases for you and never leave a checkout half-patched.

## 4. Build and run FilmCraft

```bash
cd ~/Developer/filmcraft
cargo run -p filmcraft --release
```

The first release build takes 10–30 minutes, depending on the machine; later builds are
incremental. Use `cargo run -p filmcraft` (debug) while experimenting; it builds faster but plays
back less smoothly.

Optional: FilmCraft can embed extra fonts for Japanese and Chinese UI text from a separate repo.
It works without them. See FilmCraft's `CLAUDE.md` ("Fonts").

## 5. Check that it works

In FilmCraft:

1. **File ▸ New ▸ Demo Project** (or open your own project).
2. **Window ▸ Effects**, search `voice`. *Voice Isolation* should appear under
   **Audio Effects ▸ Noise Reduction/Restoration**.
3. Select an audio clip, **Window ▸ Essential Sound**, click **Dialogue**, open **Repair**.
   **Isolate Voice** should be the first switch.

Automated checks (builds FilmCraft; the first run takes a while):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\verify.ps1          # everything
powershell -ExecutionPolicy Bypass -File scripts\verify.ps1 -Quick   # DSP and engine only
```

```bash
scripts/verify.sh            # everything
scripts/verify.sh --quick    # DSP and engine only
```

It should end with `All checks passed.` [TESTING.md](TESTING.md) lists what each check covers.

## Newer FilmCraft versions

The patch is small and touches stable parts of FilmCraft, so it usually applies to newer
versions too. The script falls back to a three-way merge on its own. If that still fails:

1. Install on the tested commit: `git switch -c voice-isolation 70f0fc7a75f2681bc18302e67eaf0c9d41c68873`
   and apply there, which always works.
2. Then bring it forward with git: commit it (`git add -A && git commit -m "Voice Isolation"`) and
   `git rebase origin/main`. Conflicts, if any, will be in one of the small edits listed in
   [step 3](#3-install-by-hand). Keep both sides, since every edit only *adds* lines.
3. Run `scripts/verify` to confirm the result.

If FilmCraft renamed something the patch relies on (for example the `AudioEffect` trait or the
`DIRECT` table in `crates/render/src/audio_fx.rs`), [HOW-IT-WORKS.md](HOW-IT-WORKS.md#integration-points)
lists every integration point so you can redo it by hand.

## Uninstalling

```powershell
powershell -ExecutionPolicy Bypass -File scripts\uninstall.ps1
```

```bash
scripts/uninstall.sh
```

This reverses the patch, then you rebuild FilmCraft. If you committed the extension, use
`git revert <commit>` instead. Projects that used Voice Isolation still open: FilmCraft keeps
the effect entry in the file and simply doesn't apply it, and Essential Sound ignores the
`isolate` setting.

## Contributing it upstream

The patch follows FilmCraft's contributor rules (no `unwrap`/`panic` outside tests, no `unsafe`,
no new dependencies, no assets, translations for every UI string, rustfmt with the project
config, clippy clean, tests for every path). To offer it to FilmCraft: fork
`storytold/filmcraft`, apply the patch on a branch, run `cargo xtask ci`, and open a pull request.
Read their `AGENTS.md` and `docs/contributing.md` first.
