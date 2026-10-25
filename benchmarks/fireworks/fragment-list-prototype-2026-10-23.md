# F8.1C4D — bounded fragment-list prototype

Bevy 0.20's fragment-list compositor is now connected to Aestra's native particle
pipeline behind an explicit **experimental camera opt-in**. It fixes the bounded
interleaved/moving-pool image oracles. This is not production transparency, realistic
smoke art, or public-show performance approval. No show asset or default changes.

## Integration and scope

The Bevy adapter wraps the final legacy/semantic alpha fragment, after authored
coverage and lighting, supplies premultiplied RGB to Bevy's `oit_draw`, and discards
the ordinary color output. Zero-alpha fragments are discarded before consuming list
capacity. The portable material compiler, authored format, particle ABI, simulation,
event links and PlaybackOnly history are unchanged. No particle/fragment readback or
CPU draw expansion is introduced in playback.

Only approved 3D sprite/flipbook alpha draws opt in. Semantic draws must have ordinary
Less/LessEqual depth testing and no depth writes. Mesh/ribbon/trail, diagnostic wireframe,
additive, multiply, depth-writing and overlay-style materials retain their existing
paths. The prototype does **not** establish correct ordering between participating
and nonparticipating draws: Bevy resolves the collected fragments after the ordinary
transparent pass. Shader wiring covers legacy and semantic paths; the native image
oracle here qualifies **unlit semantic sprites**, not every eligible material/flipbook.

`ExperimentalOitSettings` is exported by the renderer and Bevy integration for lab
cameras. It is not a new fireworks flag and is not attached to the showcase/editor.
It requires explicit `Msaa::Off`, fragment storage writes and at least 12 storage
bindings. Its initial policy limits average capacity to (0,16] fragments/pixel and
the local sort count to 1–32. Defaults are four fragments/pixel, eight sorted fragments
and a 32 MiB **node/head storage** budget. These are bounded prototype policies, not
new authored particle limits or a guarantee against fragment saturation.

Approval considers the shared maximum target dimensions and average capacity across
opted-in cameras, the strictest camera budget, and the device storage-binding limit.
Invalid settings, unavailable targets, unsupported MSAA/capabilities, unmanaged OIT
cameras or an excessive allocation are rejected with `ExperimentalOitStatus` and
ordinary rendering remains in use. There is no automatic MSAA/quality reduction.
The 32 MiB default intentionally rejects a full 720p/1080p four-fragment allocation;
larger labs must choose a larger budget explicitly. Multi-camera rendering/resize
correctness still needs native qualification even though approval budgets shared storage.
MSAA rejection is checked at frame/schedule boundaries. Hosts must not toggle MSAA
inside an unordered `Update` system while OIT is active: Bevy itself asserts its MSAA
contract in `Update`. Remove the opt-in/settings before such changes. The native
toggle control below changes components between app updates, not inside that schedule.

`ExperimentalOitStatistics::snapshot()` publishes node/head capacities and actual
buffer bytes from CPU ownership metadata. It **does not** count live fragments,
detect dropped fragments or certify the local approximate tail. Ancillary settings,
counter/uniform storage and other renderer resources are outside this budget.

## Native oracle and lifecycle controls

The existing red/blue source-over fixtures run through ordinary compiled GPU players,
forward PlaybackOnly, with frozen 480×360 sRGB LDR captures and no tone mapping, bloom
or lights. Both camera directions and player spawn orders are exercised. Per-pool
sorting is disabled (`Fast`) to ensure the fragment lists, not an additional particle
sort, are responsible for correct compositing. Single-pool calibration and restored
images are checked alongside the analytic central 8×8 linear-color reference.

| Workload | Cases per repetition | Maximum linear reference error |
| --- | ---: | ---: |
| Red pool straddling blue | 4 | .0010396 |
| Particle crossing blue, frames 30/90/150 | 12 | .0030670 |
| Non-OIT straddling baseline | 8 | .1252282–.2487378 |
| Non-OIT post-crossing baseline | 16 | .2498416 |

The first two rows are prototype passes under the unchanged <.01 threshold. The last
two are successful **expected-failure** diagnostics, not accepted compositing. The
non-OIT pre-crossing cases remain correct. Single-pool prototype calibration error is
at most .004596 and visibility restoration is byte-exact.

For each of the four interleaved configurations, the same app also:

1. Lowers the budget to one byte, verifies rejection, checks the incorrect baseline
   reappears and the node/head allocation falls to Bevy's unused 16-byte placeholder.
2. Restores the budget and verifies correct source-over again.
3. Changes MSAA to four samples while OIT is active; approval removes its OIT settings
   before Bevy's Update-time MSAA assertion. The host's MSAA choice is preserved,
   status reports rejection and baseline compositing returns.
4. Returns to `Msaa::Off`, removes the experimental component, verifies owned Bevy
   settings/status are removed, then re-enables it. The original OIT image returns
   byte-exactly.

Two final fresh native runs (`e`/`f`) pass all 16 prototype cases and these guards;
all **76 corresponding PNGs match byte-for-byte**. The non-OIT baseline matrix was
rerun on the final source. Visual inspection confirms the corrected interleaved blend
differs from the budget-rejected image. This is a blend-order oracle, not a smoke-art
comparison. See the [raw reports, source hashes and capture hashes](fragment-list-prototype-2026-10-23.json).

## Allocation and tiny frozen cost comparison

Windows / RTX 4070 SUPER / Vulkan, Rust 1.98.1, Bevy 0.20.0. The prototype owns
691,200 twelve-byte nodes and 172,800 four-byte heads: **8,985,600 bytes (8.57 MiB)**.
The native allocation equals the approval estimate. This allocation scales with view
dimensions and requested average depth complexity, not just the three live particles.

Matched frozen observations use the same app, particles, clock, camera, resolution,
`Msaa::Off` and `Fast`, first with OIT and then with its budget rejected. After 40
warmup updates, 199–200 distinct GPU timestamp samples are collected. Both final runs:

| Pass | Prototype median / p95, ms | Non-OIT median / p95, ms |
| --- | --- | --- |
| Transparent | .007168 / .007168 | .004096 / .009216 |
| OIT resolve | .007168 / .007168 | no resolve |

Do not sum pass percentiles into a frame percentile or interpret this as a full-show
benchmark. The baseline intentionally produces the wrong blend. Empty-light/cluster
infrastructure, GPU timestamp quantization and capture pumping remain present. Other
CPU validation work may run concurrently, but no second native GPU workload runs.
This tiny scene demonstrates an added fixed time/memory cost, **not a speedup or a
production regression budget**. Dense overlap, live paced workloads, HDR and public
showcase costs have not been measured for this compositor.

## Decision and next gates

Retain this as an opt-in experiment; **do not enable it on the public smoke show**.
The bounded correctness proof warrants further qualification, not adoption. Next:

- Add deliberate global-list saturation and local-sort-threshold probes, with a
  trustworthy overflow/saturation signal. Host allocation-budget rejection is not
  the same as runtime fragment overflow. Bevy can drop fragments when its global
  list fills and approximates beyond its local sort count.
- Exercise opaque depth occlusion, lit semantic smoke/HDR radiance, textured legacy/
  flipbook paths and mixed additive/trail/non-OIT interactions. Preserve dark smoke
  and local burst relighting, rather than hiding failures with additive blending.
- Qualify resize, multiple cameras, opt-out/cleanup and unsupported devices; the
  conservative camera budget guard alone is not rendering acceptance for those cases.
- Compare matched dense frozen and live-paced workloads and actual storage before
  deciding whether this is viable for the public show, requires revision, or should
  be compared with an explicitly approximate weighted approach.

Continuous star-path smoke/shared-wind haze art follows these rendering gates.
Default/C3/wispy saved assets and sound/light routes remain untouched in this slice.

## Reproduce and validation

Run native GPU work alone and choose a fresh absolute directory each time:

```powershell
$env:AESTRA_CROSS_POOL_IMAGES = 'C:/absolute/fresh-oit-probe'
cargo test -p aestra-viewer fragment_list_probe --locked --offline -- --ignored --test-threads=1

# Includes the original expected-failure baselines and the prototype:
$env:AESTRA_CROSS_POOL_IMAGES = 'C:/absolute/fresh-complete-probe'
cargo test -p aestra-viewer cross_pool_ --locked --offline -- --ignored --test-threads=1
```

Renderer ordinary tests: 167 pass / 3 existing native ignores. Viewer: 101 pass /
31 opt-in ignores. Strict workspace all-target Clippy with `aestra-bench/gpu`, the
isolated qualification harness's all-target/editor Clippy, both formatting checks and
whitespace validation pass. A Naga test checks the semantic fragment-position bridge;
the native probe separately exercises the real Bevy imports and resolve pass. The
qualification harness now imports the production opt-in module and keeps its existing
pipeline cases opted out. No existing validation gate is removed. Full workspace test
execution, Linux and the public HDR/art/live-cost runs were not repeated here.
