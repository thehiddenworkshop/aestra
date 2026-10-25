# F8.1C4H — textured alpha and flipbook integration

The experimental compositor passes bounded texture/atlas integration checks in
both final native repetitions. No production renderer fix was needed. The public
fireworks show remains opted out; **this is not smoke-art or production acceptance**.

## Three independent renderer/material paths

The viewer-only fixture reuses the three-particle depth-interleaving oracle: two
particles in one pool straddle a particle in another. Both camera directions and
spawn orders run with `PlaybackOnly`, forward frozen sampling and `Fast` pool order.
Every original particle is separately reproduced at its evaluated world position
in an ordinary single-particle draw, for which `DepthBackToFront` is sufficient.
Collection is disabled for this reference, then restored on the original pools.

The paths are:

- Semantic sprite: `Texture2D` parameter → resolved `Uv0` → `SampleTexture`, sampled
  RGB and alpha, with instance radiance gain and `ScenePointIrradiance`.
- Semantic flipbook: the same material through the native renderer's frame UVs.
- Legacy alpha flipbook: no semantic material instance; exercises the compatibility
  fragment shader, atlas texture binding and coverage path independently.

Images are deterministic test data generated in Rust, not proposed smoke art. Each
32×32 cell has a distinct red/green/blue dominant channel, secondary channels 32/255,
alpha 64/128/192, a transparent notch with nonzero RGB and a lower-opacity band.
Textures use `Rgba8UnormSrgb` and linear sampling. Semantic texture descriptors also
declare sRGB. The sprite uses one cell; flipbooks use a three-cell horizontal atlas,
forward `EffectTime` at 2 fps, looping, without random start.

The independent central 8×8 oracle decodes RGB from sRGB, keeps texture alpha linear,
multiplies alpha by particle opacity .5, orders all three world depths, performs
source-over on linear radiance, then applies fixed Reinhard/exposure. Semantic gain
4 produces radiance above SDR; the legacy path retains gain 1. Screenshots are
480×360 display-referred SDR from an HDR camera, not raw HDR data. The off-center
notch, opacity band, filtered edges and atlas boundaries are included in the separate
**whole-image** reference comparison, not claimed as independent analytic texel tests.

## State changes and stale-output controls

Each semantic path has 12 phases per camera/spawn combination:

1. Frames 10, 40 and 70 select atlas frames 0, 1 and 2; they must visibly differ.
   At frame 100 the atlas loops to frame 0. The sprite deliberately stays static.
2. Replace both primary images' contents under the **same handles**, then restore.
3. Change the instance texture parameter to a different asset/handle, then restore
   the primary texture while changing radiance gain to 2.
4. Restore gain 4 and change exposure by +1 stop.
5. Restore exposure and enable a controlled white host point light at `(-2,1,3)`,
   24,000 lumens, range 16, radius .5, shadows off. This fixture uses the red irradiance
   channel as a scalar contribution; it is a native-light callback integration check,
   **not a physical scattering model**. Center response must increase by > .01.
6. Make every primary texel fully transparent. The result must be completely black
   and the GPU attempted-fragment counter **zero**, despite nonzero texture RGB.
7. Restore the original textures/light/exposure and remove instance material
   overrides, returning to automatic bindings.

The legacy path runs nine phases: it omits semantic parameter and lighting phases.
Its atlas, image changes, exposure and zero-alpha controls remain active. All original
players retain their compiled project identity and remain paused at the sampled frame;
changing texture/material resources does not reset or replace playback.

Every phase compares OIT with the normal sorted reference at < .01 maximum linear
RGB error across **every pixel**. Every non-lit center also matches the independent
analytic prediction within .01. Restoring OIT, looping, image restoration and final
state restoration must return within one byte. All diagnostics must have zero global
drops, over-sort pixels and visited invalid links. Normal CPU light-position transport
stays disabled. The renderer does not enumerate fragments/particles on the CPU;
readbacks here are explicit screenshot/aggregate diagnostic tooling.

## Final evidence

Windows / NVIDIA RTX 4070 SUPER / Vulkan, Rust 1.98.1, Bevy 0.20.0, MSAA off,
fixed Reinhard, bloom zero, dither off:

| Measurement | Both final repetitions |
| --- | ---: |
| Semantic sprite / semantic flipbook / legacy flipbook cases | 48 / 48 / 36 |
| Total cases | 132 per run |
| Analytic center maximum linear error | .003342 |
| Whole-image split-draw maximum linear error | .007450 |
| Minimum controlled-light center contrast | .013678 |
| Fully transparent cases with zero attempted fragments | 12 per run |
| OIT and state restoration maximum byte error | 0 |
| Corresponding PNGs matching | 396 / 396 |

Final roots are `target/fireworks-f8/fragment-list-textures-2026-10-25-b` and `-c`.
The `-b` run also passes all five prior native probes (healthy/lifecycle, saturation,
opaque depth, HDR and saved-material lighting). All 168 prior healthy/depth/HDR/lit
PNGs match the preceding final C4G run byte-exactly. Deliberately saturated images
are not included in that repeatability claim: their fragment loss is an expected,
arrival-order-dependent degradation, not accepted exact blending.

The earlier `-a` texture run passed before the explicit light-contrast and automatic
binding-restoration assertions were added; it is not the final validation source.
See [reports, hashes and validation](fragment-list-textures-2026-10-25.json).

Ordinary viewer tests pass (105 passed, 36 intentionally ignored), along with strict
workspace/all-target Clippy using `aestra-bench/gpu`, formatting and diff checks.
Rust emits existing incremental-cache hard-link fallback-to-copy warnings; these
are not source lint failures. No full workspace test or Linux run is claimed.

## Reproduce and remaining scope

Use a fresh absolute output root. Run GPU probes alone and sequentially:

```powershell
$env:AESTRA_CROSS_POOL_IMAGES='C:/absolute/fresh/texture-probe'
cargo test -p aestra-viewer fragment_list_textures_and_flipbooks --locked --offline -- --ignored --test-threads=1
```

Use filter `fragment_list_` to run the full six-probe regression. Ordinary viewer
tests also validate fixture compilation and distinct frame/texture predictions;
the native test is intentionally ignored in ordinary headless CI.

Only viewer test fixtures, documentation and evidence change. No portable ABI,
production shader, public API, dependency, show asset or default changes. The shared
sheet helper's default behavior is preserved; the new optional configuration supplies
only test-specific assets/renderers. This does not qualify filesystem loading/watching,
compressed textures, mip chains, reverse/ping-pong/random/particle-age playback,
near-plane/alpha-mask depth, mixed additive/trail/non-OIT ordering, multiple cameras,
resize, other display transforms/bloom, Linux or dense/live-paced show costs.

Next: **mixed additive/trail/non-OIT interactions**. Keep the compositor experimental
until that and the remaining gates justify adoption or a different approach. Continuous
star-path smoke, wind-drifted haze and public-show visual/performance review follow
the compositor decision; these colored diagnostic quads are not that art pass.
