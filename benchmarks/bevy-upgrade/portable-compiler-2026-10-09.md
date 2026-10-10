# B20-1 portable shader compiler — 2026-10-09

Scope: the first renderer-migration slice, not a completed Bevy 0.20 upgrade.

## Implementation

- Pin Aestra's direct compiler dependencies to WESL 0.6.0 and Naga 30.0.1.
  Adapt `aestra-gpu/src/shader.rs` to `Compiler::with_resolver` and
  `compile_module`, preserving virtual, package-qualified module resolution.
- Explicitly disable WESL's new visibility policy at this boundary: existing
  authored helper modules remain importable without adding `public` keywords.
  Preserve entry-point names, Naga validation and authored/generated error sources.
- `bevy/aestra-bevy` inherits the same WESL pin for its dev-dependency. Update
  `prepare-020-probe.ps1` to assert the new compiler baseline while still probing
  the remaining engine migration. Do not rewrite historical preflight evidence.
- Add `SHADER_COMPILER_ID` to material-program fingerprints (and therefore pipeline
  keys) and thumbnail disk-cache keys. Compiler pins and composition-policy identity
  must change together. This invalidates old backend-dependent results without an
  authored schema, artifact-envelope or GPU buffer-layout change.
- Add positive coverage for aliased/transitive imports and root-module shadowing,
  plus negative coverage for malformed source, missing imports/entry points and
  shader type errors. Existing portable material, stateful and trail checks remain.
- Review generated-text changes rather than mass-approving snapshots: the simulation
  snapshot changes only three `while (...)` conditions to equivalent `while ...`.
  The generated sprite shader is unchanged. Update the deterministic material
  fingerprint golden because compiler identity is now part of the cache contract.
- Native qualification exposed three pre-existing fluid fixtures allocating/indexing
  nine state words despite the baseline runtime already requiring ten (including
  distance-emission residual). Use `STATEFUL_STATE_STRIDE` in fixture construction,
  allocation and assertions. Preserve all velocity, dead-slot, sampling and spawn
  assertions; production state/shader code is unchanged by this fixture repair.

## Executed checks

Rust 1.98.1 MSVC, Windows. Commands run from the repository root with `--locked
--offline` (the cold dependencies were already available locally):

- `cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-gpu --lib --tests -- --test-threads=1`:
  **105 passed**, including 30 material contracts, 14 shader contracts, stateful
  shaders, architecture, trails, deformation and host bindings. SPIR-V/HLSL
  translation remains checked; no changed image references were approved.
- Fluid contracts: **34 passed**. Native fluid rerun with
  `AESTRA_REQUIRE_GPU_CONFORMANCE=1` and `--test-threads=1`: **46 passed**, seven
  existing benchmark/soak tests ignored. Includes the three repaired fixtures.
  Hardware is mandatory for this accepted rerun; it is not a software skip result.
- Editor thumbnail-cache regressions: **10 passed**, including the new compiler
  invalidation test. Other editor tests were filtered, not claimed as a full run.
- Project library tests: **94 passed**, including source-asset/shader validation
  and filesystem relocation guards using the new direct Naga dependency.
- Native `aestra-bevy-render --test gpu_conformance`: **3 passed**, with
  `AESTRA_REQUIRE_GPU_CONFORMANCE=1`, no ignored tests and serial execution after
  fluid finished. The locked, real-source test binary consumes newly generated
  WGSL through wgpu 29 and compares simulation against the CPU reference, including
  curves, playback sources/parameters, ribbons/trails and seven bundled showcases.
  This verifies the temporary text boundary, not Bevy 0.20 rendering or visual parity.
  Executable: `target/debug/deps/gpu_conformance-0c1ee40675c77f16.exe`, SHA-256
  `6922bbf7f27c1a4d681674c3ddceab0f2cae8cf3ce3599f13cdffc5c8234fae4`.
- `cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu`:
  pass. Strict workspace Clippy and root formatting also pass.
- `prepare-020-probe.ps1 -Resolve -Offline`: the prospective all-features graph
  still resolves one Bevy 0.20.0, wgpu/Naga 30.0.1, WESL 0.6.0 and glam 0.33.12.
  This is **resolution only**, with placeholder sources, not source compatibility.
  Original manifests/lockfile remain unchanged by the probe. PowerShell AST passes.

Shipping lockfile SHA-256:
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
The fresh resolution-only evidence is under
`target/bevy-0.20-preflight/4f74c6002c67411bb08fcccc2039fa78/`; its
`resolution-summary.json` SHA-256 is
`7e5777fc6f793db9af276a61b959a7479f0cae7196d53d0f0599e159407ffc54`.

Full workspace tests, the full editor suite and the Bevy 0.20 visual/performance
gate are not claimed by this slice. Existing full-editor and candidate-patch
qualification reports remain separate historical runs.

## Boundary and remaining work

The shipping graph still has **one Bevy engine, 0.19.1**, using wgpu/Naga 29 and
its internal WESL 0.3.2. Aestra's independent portable compiler uses Naga 30/WESL
0.6 and passes **WGSL strings**, not Naga IR or ECS/math types, to that renderer.
The temporary duplicate compiler libraries disappear with the coherent engine
switch; they are not two engines sharing components. Keep wgpu/glam on the current
engine's versions until the renderer and editor are ported together.

Next: convert Bevy-importing volume/lighting and editor graph shaders to WESL,
then migrate extraction/rendering and required editor APIs. Select the previously
qualified 0.20 patches at the root engine switch. B20-1/B20-3 still require native
visual/performance comparisons; this slice does not establish 0.20 renderer parity.

Costs: affected dependencies need a cold rebuild, and thumbnails regenerate once
under the new compiler key. Existing cache files are not deleted. There is no new
production GPU wait, replay dependency, CPU particle mirror or per-particle ECS work.
