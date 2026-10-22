# Windows executable-relative content defaults — 2026-10-22

Follow-up to `release-packaging-performance-2026-10-22.md`, on the real Bevy
0.20 workspace after `32575f76`. This changes application-owned path resolution,
not the VFX runtime, fixtures, quality budgets or performance gates.

## Content contract

```text
package/
  apps/aestra-editor/aestra-editor.exe
  apps/fireworks/fireworks.exe
  assets/                    # including test/ and editor UI assets
  sample-project/            # including extensions/, excluding local .aestra/
  audio/                     # optional, separately licensed WAV pack
```

Editor bundled examples/extensions now resolve from the executable's content root.
The fireworks host defaults to that package's `assets/test` and optional `audio`.
Neither default embeds `env!("CARGO_MANIFEST_DIR")` or searches the original
checkout if package data is missing. Saved/recent editor projects and explicit
example CLI paths retain their existing precedence.

Cargo's runtime manifest variable preserves `cargo run`/`cargo test` with custom
target directories. Without it, standard Cargo executable layouts also work;
nonstandard standalone example layouts require explicit CLI paths. Empty Cargo
manifest values are treated as absent. Existing audio folders are canonicalized
so filesystem discovery and Bevy's asset reader agree on relative overrides.

## Ordinary checks

- Full editor suite: **2849 passed, zero failed, 10 existing opt-in ignored**;
  `target/bevy-020-relocation-editor-tests.log`.
- Audio-enabled example: **23 passed**, default audio-disabled example:
  **19 passed**; `target/bevy-020-relocation-example-tests.log` and
  `target/bevy-020-relocation-no-audio-tests.log`.
- Tests cover moved roots with spaces, missing packaged content despite an
  existing checkout, Cargo/test executable layouts, external Cargo target
  directories, explicit project/effect/audio overrides and `--no-audio`.

## Retained failed native probe

The first staged launch is **not accepted**. On this PowerShell/runtime,
`[Environment]::SetEnvironmentVariable(name, $null, 'Process')` leaves the key
present with an empty string. The initial path resolver interpreted that as a
runtime manifest, selected checkout assets from the launch working directory,
and found no WAVs. It finished with 13 flashes but zero audio voices. The path
assertion correctly failed rather than accepting a plausible rendered frame.

Evidence: `target/bevy-020-relocation/empty-environment-failed-{stdout,stderr}.log`
and `empty-environment-failed-26s.png`. The follow-up adds empty-value regression
coverage and uses `Remove-Item Env:CARGO_MANIFEST_DIR` / `Env:BEVY_ASSET_ROOT` for
the native rerun, explicitly checking that both environment keys are absent.
The earlier linkage/performance report is not independent relocation acceptance.

## Final rebuilt/native results

- Rebuilt both optimized executables with:
  `cargo +1.98.1-x86_64-pc-windows-msvc build --release -p aestra-editor -p aestra-bevy --bins --example fireworks --features aestra-bevy/fireworks-audio --locked --offline`.
  Log: `target/bevy-020-relocation-release-final.log`.
- After the empty-value guard, all **three editor path tests** and **23
  audio-enabled example tests** pass again. Logs:
  `target/bevy-020-relocation-empty-env-tests.log` and
  `target/bevy-020-relocation-example-final-tests.log`.
- Strict all-target Clippy for both packages, with the same audio feature and
  `-D warnings`, passes; `target/bevy-020-relocation-clippy-final.log`.
  Workspace formatting and `git diff --check` pass.

Fresh copies live at `target/bevy-020-relocation/moved package/`, with spaces in
the path. Both launch from its **parent**, not their executable directories.
The original checkout remains present. Cargo/Bevy asset environment keys are
verified absent; PATH contains only Windows/System32 and Windows. No Cargo
launcher or project/audio CLI overrides are used. Editor configuration is
isolated in `target/bevy-020-relocation/editor-config`.

- **Showcase:** exits successfully after its delivered 26-second capture. Startup
  logs name the staged `assets/test` and staged `audio`, not source defaults.
  Native RTX 4070 SUPER/Vulkan, high/PlaybackOnly, 13 authored flashes admitted,
  three peak, zero invalid/stale. Sound: **134 admitted, 89 budget drops, zero
  pending, 24 peak sinks**. No missing-WAV, shader/asset error or panic. The drops
  remain the existing bounded audio policy, not a lossless audio claim.
  Logs: `target/bevy-020-relocation/show-{stdout,stderr}.log`; inspected capture:
  `target/bevy-020-relocation/default-show-26s.png` (1280×720, finished HUD/clear
  scene). This is not an in-flight art comparison or listening test.
- **Editor:** remains alive for 12 seconds, publishes native GPU readiness and
  registers `org.example.aestra-wind` from the **staged sample-project/extensions**.
  No logged asset/shader error or panic. Logs:
  `target/bevy-020-relocation/editor-{stdout,stderr}.log`. Only the process started
  by this check is force-stopped in cleanup. No native interaction, graceful
  shutdown or final UI legibility approval is inferred from startup.
- **Incomplete package:** copied fireworks executable without assets exits **2**
  before creating a window, naming its missing package-local project. The original
  source assets still exist but are not selected. Log:
  `target/bevy-020-relocation/missing-content.log`.

SHA-256 of accepted staged artifacts:

```text
release editor    F61B2C0261D4DB4BCDA653CE5FF64F38FFFBB83C8D60952F40D1982CF5BD2AE5
release fireworks DE120F6D7DE573C1F00C603CB9098C423CCAEA926FD6E59A1FD3BA7AA23643CE
finished capture  079AD6DCD3C89E91258A4458C2D8A3800BFF7E9C2B03514ED28DDBC50C3C300E
```

No recording files are added to Git or licensed for redistribution here. The
previous matched release GPU cost fixture is unchanged and is not rerun for a
path-only fix. This is a local Windows staged-content check, not clean-machine
installer, final UI/art/listening or Linux acceptance.
