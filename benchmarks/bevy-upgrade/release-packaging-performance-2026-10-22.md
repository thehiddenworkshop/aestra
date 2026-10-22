# Bevy 0.20 Windows linkage, staged startup and release costs

Date: 2026-10-22. Engine baseline `32575f76`, with the pending narrow-transform UI
fix in the editor worktree. Rust 1.98.1, Windows 11 build 26200, Ryzen 7 7800X3D,
RTX 4070 SUPER, NVIDIA 621.01, Vulkan. No production source or asset was changed
for these checks. Copied executables/assets and local audio remain under ignored
`target/`, not a published distribution.

## Executables, not just cargo check

Both builds passed:

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc build --release -p aestra-editor -p aestra-viewer -p aestra-bevy --bins --example fireworks --features aestra-bevy/fireworks-audio --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc build -p aestra-editor --features dev-dynamic-linking --locked --offline
```

Build logs are `target/bevy-020-{release-executables,dynamic-executable}-build.log`.
The cold release build took 7m02s; the development link build took 2m59s. These are
individual observed build durations, not controlled incremental-build benchmarks.

MSVC `dumpbin /DEPENDENTS` confirms:

| Executable | Bytes | Bevy/Rust DLL requirement |
| --- | ---: | --- |
| Release editor | 141955584 | Neither Bevy dylib nor Rust std DLL |
| Release viewer | 107516928 | Neither Bevy dylib nor Rust std DLL |
| Release fireworks (WAV enabled) | 104306688 | Neither Bevy dylib nor Rust std DLL |
| Development editor | 44352000 | `bevy_dylib-1aafdd8203dd776d.dll` and `std-44a584f44bc3dd65.dll` |

The development Bevy DLL is 148396032 bytes; its matching Rust std DLL is 849920
bytes. Cargo tree selects `bevy_dylib 0.20.0` only with the explicit development
feature. `cargo editor-dev` and the VS Code editor task already select that feature.
Keep it off for shipping; it is an opt-in feature, not a profile-enforced ban.
The release executables still import Windows/MSVC CRT components including
`VCRUNTIME140.dll`, and GPU execution needs a compatible driver. "Static Bevy"
does not mean no host runtime, assets or installer prerequisites.

## Staged execution

Private layout: `target/bevy-020-windows-package/`.

- Release editor: `release/apps/aestra-editor/aestra-editor.exe`, with UI assets
  at `release/assets/` (matching its existing `../../assets` reader path).
- Development editor: the equivalent `development/` layout with the two matching
  DLLs beside the EXE, rather than relying on Cargo's injected DLL search paths.
- Release fireworks: `release/apps/fireworks/fireworks.exe`, copied VFX project
  at `release/assets/test/`, and a private copy of the existing local WAV pack at
  `release/audio/`. This pack was not added to Git or cleared for redistribution.

Each launch cleared `CARGO_MANIFEST_DIR` and `BEVY_ASSET_ROOT`, restricted PATH to
Windows/System32 and Windows, and used the package root as its working directory.
Both editors used fresh, separate `AESTRA_CONFIG_DIR` locations. They remained
alive through a 12-second observation, published native RTX/Vulkan readiness and
logged no errors. Only those test-owned processes were then force-terminated.
This tests loader/startup behavior, not graceful close, every UI interaction or
an installer on a clean machine.

The copied release fireworks EXE ran with explicit `--project` and `--audio-root`
pointing into the stage, high/PlaybackOnly and the existing 26-second capture hook.
It exited successfully after screenshot delivery: 13 flashes admitted, peak three,
zero invalid/stale; 134 voices admitted, 89 budget drops, zero pending and 24 peak
spatial sinks. No dependency-loader, asset, shader or audio errors were logged.

Logs/import lists: `target/bevy-020-windows-package/*.log`; screenshot:
`release-showcase-26s.png`. A real portability limitation remains: the editor's
default sample-project/extension root and the fireworks example's default asset
paths use compile-time source locations. The editor startup log actually registers
the sample extension from the original checkout. Therefore the staged editor is
**not** certified checkout-independent, and the fireworks host requires its explicit
paths for relocation. A relocatable default-path/installer pass remains separate.

## Matched release GPU matrix

The existing runner and validator were used without weakening thresholds:

```powershell
./benchmarks/fireworks/validate-particle-light-costs.ps1 -SelfTest
$env:RUST_LOG = 'info'
./benchmarks/fireworks/run-particle-light-costs.ps1 -ReportsDirectory target/bevy-020-release-costs -ViewerBinary target/release/aestra-viewer.exe -Repetitions 3 -Tiers high -RequireRetirement
```

All **18 native runs** and the final read-only validation passed. Three repetitions
alternate enabled/control ordering for hero, volley and show. The runner retains
binary/asset hashes, requires unchanged inputs and matches authored identity,
capacity, seeds and delivered event work. Its positive/negative self-tests pass.
No builds or other native test workloads ran concurrently with the matrix.

Conditions: 960×540 offscreen target, audience camera, high tier, HDR/Tony/exposure
0/bloom 0.15, fixed sequential 60 Hz simulation, unpaced PlaybackOnly. There are
120 warmup updates, then 600 measured hero/volley updates or 1680 show updates.
The fixture authors selected-star outputs with a 96-slot global budget; control
uses zero writable GPU-light slots while retaining selection. Show representative
flashes are enabled in both modes. These are the existing cost fixtures, not the
public wispy-smoke/audio/environment workload or a production finale.

Ranges span all three repetitions; milliseconds:

| Probe | Outer render GPU p95, enabled | Control GPU p95 | Enabled minus control run-mean GPU time |
| --- | --- | --- | --- |
| Hero | 1.077–1.103 | 1.056–1.109 | +0.037–0.041 |
| Volley | 1.142–1.199 | 1.129–1.166 | +0.040–0.044 |
| Show | 4.584–4.981 | 4.631–4.679 | +0.041–0.203 |

Each show admits the unchanged **8413 event children** across 13 clip owners and
passes forward admission, no-overflow/rejection/history-loss and cleanup checks.
All show pairs settle their last 60 native cluster-demand acknowledgments to the
same value as control (**one**, not an invented zero), with zero writable adapter
capacity. Measured public index/offset storage stays stable. Private allocations
and total resident VRAM are not certified by this public-buffer check.

Fresh outer-render timing samples per run: hero 447–456, volley 448–463, show
1392–1445. Asynchronous diagnostic distributions are not frame-paired; phase
percentiles must not be summed. These figures exclude whole-app/game CPU timing
and cannot establish universal FPS or a Bevy 0.19→0.20 speedup: the older retained
cost matrix used a development build. No controlled pre-upgrade release comparison
was made here.

Raw matrix: `target/bevy-020-release-costs/manifest.json`, `summary.json` and
`pair01/` through `pair03/`. Manifest `accepted=true`, 18 successful exits,
unchanged inputs, no failure; the retirement gate is `F7E4B3B1`.

## Retained SHA-256

```text
release editor    E02110A82762B300694139AD46E20DE8EDD2404F4DDC49C1545837764DDA84AA
release viewer    C396377A63C2CF457378A7A6D90DF855214BBC6B12F746197A47455032B363F2
release fireworks CBCB7BFFF69AC581F393EC618377BD8211A24A24454EC5501E7942049B29B99D
dev editor        C870D9F6E9F6B3EFFD68FBC5A805096C9009E890B45E1EDBBFE027130CD8EE67
dev Bevy DLL      8CEEEDBC4CD859BE9AB736364F23D8484DEA4F3BCA56DDAC8AACC7B2B04F8CD8
dev Rust std DLL  8DD8EA258A3561A499C438B322B218AF3CB2F4DB1029205ADC94D58FD5B0615C
cost manifest     A0ECE0916E9814BAB23FD668D492188940E9D701247D703348E947A77D5CFE82
cost summary      A43C8411FCF0E50A165ACA1E9D8CCA85B8E06D8994471523F9204CF94FF190E8
```
