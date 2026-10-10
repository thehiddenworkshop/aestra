# B20-0 follow-up — Own the SVG boundary, park optional physics

Date: 2026-10-09. Shipping Bevy remains 0.19.1; engine migration has not started.
This follows explicit user approval of the dependency-isolation strategy. The
[original preflight](preflight-2026-10-09.md) remains historical evidence.

## Implementation

- Replace `bevy_resvg` everywhere in the editor with the Aestra-owned
  `feathers/icon/svg.rs` boundary. It uses resvg 0.47 without text/system-font/raster
  decoder features and a direct tiny-skia 0.12 dependency. Automation curves also
  use tiny-skia directly. No Bevy-specific SVG package remains in the root graph.
- Keep editable SVG icon files and 64px oversampling for the existing 8–28px controls.
  Preserve aspect ratio, center non-square vectors, convert premultiplied raster
  data to native straight-alpha sRGB images, and use linear texture sampling.
- Use a labeled Bevy Image subasset per SVG: repeated icons share the raster and
  AssetServer owns reload/caching/lifetime. Tinting and icon swaps update ImageNode
  before UI layout without rerasterizing, cloning textures or dirtying idle nodes.
  Missing/pending assets render invisibly; removed icons release their texture handles.
  This is an icon loader, not general SVG document rendering: text and image href
  resolution are intentionally disabled. All current icons use self-contained vectors.
- Exclude the Avian/Rapier adapters from the active 15-member workspace. Their source,
  examples and tests remain in standalone manifests pinned to Bevy 0.19.1, including
  the current local patch declarations. Publishing is disabled while parked.
  Ordinary CI still checks their formatting,
  but their lint/build/test coverage is **explicitly suspended**, not silently passing.
  See [availability and restoration](../../bevy/PHYSICS_ADAPTERS.md).
- Core collision contracts, GPU particle collisions and the custom-host physics
  example are unchanged. The old adapters will not work against a migrated 0.20
  core merely because their manifests retain the old Bevy pin.

## Dependency qualification

Re-running `prepare-020-probe.ps1 -Resolve -Offline` on the active manifests succeeds
with `compatible_single_engine_graph: true`: one Bevy 0.20.0 family and wgpu 30.0.1,
with Naga 30.0.1, WESL 0.6.0, encase 0.12.1 and glam 0.33.12. resvg/usvg 0.47.0 and
tiny-skia 0.12.0 do not pull in another engine. All active workspace features are
included, including development dynamic linking, benchmark GPU and showcase audio.

Raw resolution artifacts are under
`target/bevy-0.20-preflight/b3eee1c764dd4a34b1c95d74a40a2c04/`:

- `metadata.json` SHA-256: `de8b99f8da3fa051f70f429438eee33dbb7cb893375e3854110611a8db9351aa`.
- `Cargo.lock` SHA-256: `48a094d9edf4424bdc0047ae82961876cfb111c6f06836d5a0edbca0ece7473d`.
- `probe-inputs.json` records source hashes and the explicitly suspended integrations.

The probe copies manifests and uses placeholder sources: this qualifies dependency
resolution, **not Aestra source compatibility with 0.20**. Keep both real 0.19 fixes;
the same-frame keyboard failure and clustered-light ownership audit still require ports.

## Verification scope

Use Rust 1.98.1 MSVC on Windows. The root toolchain version, static-release policy
and development dynamic-linking feature remain unchanged. Editor tests exercise the
46 shipped icons, straight-alpha conversion, non-square sizing, pending loads, tint
reset, icon swaps, asset changes/removal, idle change detection, real AssetServer
loading/reloading and last-user texture cleanup. Headless tests and pixel assertions
do not replace a manual high-DPI editor visual pass or the full 0.20 renderer gates.

The first sandboxed full-editor run encountered filesystem access-denied failures
in unrelated temporary-file tests, and sandboxed Clippy could not read configuration.
Both checks were rerun outside the sandbox with approval; results are recorded below.

The approved run exposed two drag-preview regressions: a reserved ImageNode used
Bevy's transparent placeholder handle, which the browser mistook for a ready
thumbnail. The placeholder now explicitly uses the default image handle with zero
alpha. Tests verify the SVG fallback marker, default handle and invisible tint
instead of assuming an unloaded SVG has no native image component.

Final results (all Cargo commands use `+1.98.1-x86_64-pc-windows-msvc`):

| Check | Result |
| --- | --- |
| `cargo check --workspace --all-targets --features aestra-bench/gpu --locked --offline` | Pass. |
| `cargo clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings` | Final pass after placeholder fix. |
| `cargo test -p aestra-editor --bin aestra-editor --locked --offline -- --test-threads=1` | 1,168 pass, 0 fail; 10 pre-existing opt-in tests ignored. Includes all six new SVG tests and the drag regressions. |
| `cargo test -p aestra-editor --test architecture --locked --offline` | 3 pass. |
| `cargo check --manifest-path bevy/aestra-bevy-avian/Cargo.toml --lib --offline --target-dir target` | Pass on current 0.19 baseline. |
| `cargo check --manifest-path bevy/aestra-bevy-rapier/Cargo.toml --lib --offline --target-dir target` | Pass on current 0.19 baseline. |
| Root `cargo fmt --all -- --check`, both adapter formatting checks | Pass. |
| CI YAML parse, PowerShell probe syntax, `git diff --check` | Pass. |
| Root `cargo metadata --locked --offline --all-features --filter-platform x86_64-pc-windows-msvc --format-version 1` | 15 members; no bevy_resvg, avian3d or bevy_rapier3d packages. |

Final test log: `target/bevy-0.20-preflight/editor-svg-tests-final.log`, SHA-256
`bc0a071aaa04d9bd9fe44f036e02e81614a672846eedd729c219fcdcd1a47744`.
Shipping lockfile SHA-256:
`4cce1b97ca654f193c5c13be19fe1e0b819468feaf036d06786f94e44f51ba01`.
SVG implementation SHA-256:
`e9a439156f467aa8d861c782deff4522add88ae9b57858f5375fd77ffb6f9f59`.

Adapter integration tests/native collisions were not rerun: the standalone checks
qualify their manifests and types on the old baseline, not restored support. The
full workspace test suite/native GPU visual/performance baselines were not rerun
in this dependency-isolation preparatory step. Next: port the real engine/render sources and
local patches, then qualify the 0.20 migration with those broader gates.
