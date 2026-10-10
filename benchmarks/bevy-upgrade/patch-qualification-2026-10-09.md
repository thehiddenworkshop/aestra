# Bevy 0.20 dependency-fix qualification — 2026-10-09

Status: both candidate patches pass real-source headless and native qualification.
The shipping Aestra workspace remains on Bevy 0.19.1. This completes the local-patch
port/qualification prerequisite, not the renderer/editor migration or B20-3 gate.

## Implementation

- `vendor/bevy_input_focus_020`: published 0.20.0 source plus the existing event-time
  keyboard dispatcher and `KeyboardInputSnapshot`. Original pointer-focus acquisition,
  navigation, bubbling and focus-change behavior are preserved.
- `vendor/bevy_pbr_020`: published 0.20.0 source plus retained per-view cluster buffers,
  logical resets, bounded readback ownership and camera/mode cleanup. Keep upstream
  WESL shaders, pipeline constants, generic extraction and clusterable decal counts.
  Fallible mapped ranges in wgpu 30 now join the unmap/discard cleanup path rather
  than panicking or retaining a pending staging owner.
- Both packages retain published licenses and assets. `UPSTREAM_SHA256.json` records
  original per-file hashes and the registry archive checksum. Only the files listed
  in each `AESTRA_PATCH.md` intentionally differ; untouched files are verified before
  qualification. Old 0.19 vendors and the root patch table are unchanged.
- `patches-020/`: independent real-source workspace with its own versioned lockfile,
  exact Bevy version, candidate patch table and test ports. Root exclusions prevent
  accidental inclusion in shipping builds. Unlike the resolution probe, no Rust
  target is a placeholder.
- `run-020-patches.ps1`: verify the graph/provenance, run headless tests, optionally
  build and run both native tests serially in separate processes, and record stable
  source/executable hashes in fresh evidence directories. Existing evidence is never
  overwritten. No accepted summary is produced after a failure.

Resolved inventory: Bevy family 0.20.0, wgpu/Naga 30.0.1, glam 0.33.12, WESL 0.6.0.
The independent lockfile SHA-256 is
`6a5832095dc95e2796d0a8f619ccf67c1459dd7b29d29f6bbf0e3cdbe39237c6`.
Both candidate path packages were selected; no Bevy 0.19 package is in this graph.

## Executed checks

Rust 1.98.1 MSVC, Windows. Native tests identify NVIDIA GeForce RTX 4070 SUPER,
Vulkan, NVIDIA driver 616.92. This qualification uses the default unoptimized test
profile and a minimal headless synchronous render host; no Winit/event-loop or
pipelined-renderer plugin. `bevy_picking` is explicitly enabled to initialize the
pointer state needed by 0.20 text-input systems. This is not a performance profile.

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --all-targets --locked --offline --target-dir target/bevy-020-qualification -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
```

- Nine keyboard tests pass: both exact negative-probe requirements, fast Ctrl+A/V
  through actual 0.20 `TextInputPlugin`, plain text before/after a chord, held left/right
  modifiers and Shift across frames, focus loss/recovery, repeat preservation,
  AZERTY logical selection and missing-target fallback. Deferred snapshot modifiers
  and restored frame-end resources are asserted. Paste is queued, never executed
  against the OS clipboard. The original unpatched fixture is not weakened.
- All 40 upstream input-focus unit tests pass, including new pointer-focus behavior.
- Four cluster unit regressions pass: staging saturation/out-of-order completion/
  failure/duplicate handling, adaptive growth, repeated logical reset and uniform-to-
  storage conversion. Two unrelated upstream PBR unit tests are filtered, not removed.
- Both native lifetime tests pass without ERROR/panic logs; their ignored markers
  remain because they must be explicitly executed alone, as this runner does.
- Strict Clippy passes for all qualification targets; shipping workspace check and
  strict Clippy pass, both formatting checks pass, PowerShell AST and diff checks pass.
  Registry dependency lints are not claimed as a separate strict upstream lint gate.

Native results preserve all original regression assertions:

- Public index allocation stays 262,144 bytes; offsets grow 512 → 4,096 bytes and
  retain their high-water generation after shrinking. Measured empty/active demands
  are 1/16; no invented zero-index assumption.
- Deliberately undersized private capacities recover from 32 Z slices / 64 indices
  to 512 / 8,192. Private generations and capacities remain stable for 240 updates;
  scratchpad logical length stays 128 instead of accumulating.
- Independent cameras, inactive/reactivated/removed views and GPU/CPU/GPU transitions
  behave correctly. Four retired weak owners expire. Private named allocations and
  metadata staging allocations reach zero on CPU mode and final camera removal.
- Observed staging peak: two/view; asserted ceiling: eight/view. This observation is
  not a guarantee of two slots or a total process-VRAM measurement. Test-only submitted-
  work callbacks establish retirement checkpoints without a production GPU wait.

## Retained evidence

Final accepted run: `target/bevy-020-qualification/runs/5bae229ae8df46ae9ab57ec1dc6051c8/`.
It contains metadata, compiler versions, headless/native logs, before/after source
hashes and `summary.json`. Source hashes and the executed binary are unchanged
throughout that run. The shipping lockfile remains
`4cce1b97ca654f193c5c13be19fe1e0b819468feaf036d06786f94e44f51ba01`.

| Evidence | SHA-256 |
| --- | --- |
| Accepted summary | `01cf358e82d33c4b4492c45e5494727e976a90ca8f5c3cc29170225c4a677ac8` |
| Source inventory before/after | `90c49cb3190d9466062a69f99f181e0801754da02a872f92f7c9316e49759a01` |
| Native executable | `a1e864aa1f47590db5a66acd25d9f032fdf9120deb9e4e4af4ec47aed1223687` |
| Public-buffer native log | `936a3a280c93734396d6b97762bc7b7d8378179921712e0b8f5512eba2021417` |
| Private-buffer native log | `a49679709ea63fb3da348f46299a2d2c4a9ab4d63dc14c68042c7129a013f97b` |

Initial failed setup attempts remain in separate run directories: adapter-info logging
needed dereferencing; the minimal host does not install the pipelined-renderer plugin;
text-input systems needed the picking plugin. These were corrected in the fixture,
not hidden by weakening assertions. The final accepted run uses the corrected locked
feature graph. A subsequent lint-only redundant-borrow fix was included before the
final native rerun and hash capture.

## Remaining gates / next step

Proceed to B20-1/B20-2: shader/compiler/render/extraction and required editor API
compatibility. Switch root dependencies and patches coherently when that port is
buildable. Do not delete the working 0.19 patch copies prematurely. Root Cargo patches
are not transitive to downstream users and still need a host distribution decision.

This turn does not qualify the full Aestra renderer/editor on 0.20, alternate graphics
backends/platforms, production pipelining, showcase audio/dynamic linking/static
release, visuals or fireworks performance. The full workspace test suite was not
rerun; the shipping sources are unchanged from the prior isolation step and its editor
test evidence remains in that report. Refresh visual/performance baselines and run
B20-3 before accepting the overall upgrade. No new widgets or authored formats were
introduced into Aestra by this prerequisite step.
