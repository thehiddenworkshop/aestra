# F8.1B — bounded shell-born smoke cohorts

Date: 2026-10-08. NVIDIA GeForce RTX 4070 SUPER / Vulkan.
Toolchain: `1.98.1-x86_64-pc-windows-msvc`.

Historical parent-cue qualification. The current fixture uses child `OnSpawn` cues;
the original failed attempt and reports below remain historical evidence. See the
[F8.1B2 follow-up](child-birth-outputs-2026-10-08.md) for the runtime fix and unchanged-image gate.

The public `aestra-bevy` fireworks example adds `--smoke-cohorts`, resolving saved assets
through the normal project/compiler APIs. The viewer only instruments native regression
images. The normal show and earlier lab assets remain unchanged. This is a **bounded
correctness gate, not complete F8, production-finale performance or AAA smoke-art approval**.

## Workload and controls

Two one-particle ballistic shells launch at 0/2s with vertical speed 22 and gravity -9.81,
then die after two seconds. Four real GPU `OnDeath` links each request 32 children:
red shell → smoke/red stars, blue shell → smoke/blue stars. There is no independent
smoke/star emission or host injection. Total authored smoke demand is 64 into a 96-slot
pool, with 12–14s lives, expansion, drift and fade using the shared F8.1A billow material.
The fixed drift acceleration is not physical wind advection. All tiers use identical smoke
counts; selected-light budgets are 8/4/2 and representative budget is two.

Stars have no renderers; schema-required launch markers have zero opacity. Thus only smoke
can supply visible pixels. Smoke occupies one visible draw, not a cross-draw sorting test.
Native screenshots use Camera3d, 480×360 SDR/no bloom, scale 0.1, playback-only,
DepthBackToFront and seed `0xf83b000000000001`.

- No smoke before the first break (frame 110).
- Independent representative/selected/off/restored images at frames 144 and 264.
- At frame 264, remove **only** the younger smoke link: younger stars/lights remain,
  so the later break must illuminate the older cohort through both light families.
- At frame 420, compare both cohorts with older-only, younger-only and no-smoke-link
  controls. Emitter order/seed and star links remain identical.
- Frozen repetition, density-zero/restoration, fading at frame 900, drain at 1080 and
  owner removal are separate controls. Alpha-sort pairs/owned bytes must retire to zero.
- Asynchronous native link counters must admit 32 children on each of four links, with
  zero source overflow, expansion omission or destination rejection. These are cumulative
  uninterrupted-playback admission counters, **not an instantaneous GPU alive-count census**.

Each visible cohort and independent light family must exceed 100 RGB gain pixels above
3/255; each cohort must also differ from the combined image in at least 100 pixels.
Frozen/restoration/negative images allow at most 1/255 absolute channel difference.
Production selected-light CPU allocation/copy/readback/staging counts remain zero under
the existing native-transport assertions. Screenshot readback is test tooling only.

## Results and rejected attempt

Repeated high/medium/low native runs pass. All 84 corresponding PNGs match byte-for-byte.
At seven seconds the combined smoke produces 1,399 gain pixels; older-only produces 827,
younger-only 712, and removing both smoke links produces zero. Minimum independent flash
contribution is 116 pixels; at the second break representative light illuminates 489 pixels
of **older-only** smoke and selected lights illuminate 617/609/598 at high/medium/low.
Every accepted frozen/density/restoration/drain/retirement negative delta is zero.

The initial high-tier attempt is rejected, not silently overwritten: child-star `OnSpawn`
routes supplied no representative flash, and the first selected-light sample produced only
92 pixels (below 100). Source inspection shows `DomainSpawnPipeline` binds no target event
buffer and its spawn shader does not append child-birth events. The saved fixture now binds
representative flashes to the canonical **parent break `OnDeath`**; selected lights still
follow event-born stars. Child `OnSpawn` delivery remains an explicit runtime-output gap,
including domain/event/input-list births. No runtime code or threshold was weakened.
Light samples move from 0.25 to 0.4 seconds after each nominal break, giving the newly born
smoke time to fade in while the existing 0.6-second flash remains live. This does not prove
immediate first-frame visibility. Transparent markers were also required by existing schema
validation; they contribute no receiver pixels.

The public host capture at seven seconds succeeds (1280×720, public seed
`0xf1e0000000000001`), with playback-only HUD and two persistent clouds. It is not a matched
native image or a performance measurement; the HUD includes vsync. Native fresh-app runs log
existing global-logger/overlay warnings and shutdown closed-readback-channel warnings; all
accepted processes exit successfully without WGPU validation failures.

## Reproduce

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-cohorts --no-audio
```

Set `AESTRA_SMOKE_COHORT_REPORTS` to an **absolute**, fresh output directory and run alone:

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc test --locked -p aestra-viewer saved_shell_deaths_birth_overlapping_smoke_that_receives_later_break_light -- --ignored --nocapture --test-threads=1
```

Public audio-enabled example tests: 13 passed. Viewer ordinary suite: 96 passed, 23 ignored;
the opt-in native gate is executed separately. Formatting and viewer/example Clippy with
warnings denied are checked separately. See
[`smoke-cohorts-2026-10-08.json`](smoke-cohorts-2026-10-08.json) for reports, source hashes,
repeat-image provenance and commands. Raw PNGs/reports remain under `target/fireworks-f8/`.

## Next

Close the generic GPU child-birth output gap before claiming equivalent child `OnSpawn`
host cues; the parent-break cue works for this qualification. Then measure authored live
overlap/full-show costs before migrating saved show smoke. Cross-draw ordering, large-pool
sorting/fragment cost, natural-smoke art, density tiers, self-shadowing, anisotropic
scattering and generic particle/event-to-fluid injection remain unqualified.
