# F8.1C4M1 — dense overflow math and remaining ordering error

Follow-up: [C4M2](fragment-list-order-2026-10-26.md) now passes the interleaved cases
with an in-place link sort and records current costs. The results below retain the
earlier math-only implementation and its rejected ordering behavior.

The opt-in Aestra resolve now preserves already-premultiplied RGB in both local
overflow branches. It no longer multiplies smoke coverage twice or discards additive
emission because its extinction alpha is zero. **This completes the color-math/oracle
slice, not interleaved overflow correctness or renderer adoption.** Public defaults
and the fireworks show's compositor remain unchanged.

The [machine-readable evidence](fragment-list-overflow-2026-10-26.json) records source,
raw report and PNG-manifest SHA-256 hashes. Measurements use Windows, RTX 4070 SUPER,
Vulkan, Rust 1.98.1, Bevy 0.20.0 and wgpu 30.0.1. Raw captures are local generated
artifacts under `target/fireworks-f8`, not checked-in image assets.

## Repair and ownership

Bevy's collected node RGB is already premultiplied. Its exact sorted prefix consumes
that RGB directly, but its two overflow branches apply alpha again. The adapter-owned
WESL changes those branches while retaining the native 12-byte node ABI, layouts,
global buffer allocation and bounded early-blend algorithm. No vendor/registry edit,
global native shader override, new fragment storage or CPU fragment transport is used.
The existing 1..32 local limit and default budget remain unchanged.

The replacement pipeline is queued after Bevy's pipeline only on Aestra-approved
cameras. While a specialization compiles, the native resolve keeps clearing lists;
this is not a guarantee of repaired color on the first cold frame. Optional render
resources are handled without requiring a render app in CPU-only hosts.

An ownership control removes the Aestra opt-in and renders actual native
`StandardMaterial` planes. Native local limit 64 supplies the calibrated reference;
native limit 1 retains its large overflow error (.86633), with no Aestra status or
diagnostics. Restoring the managed camera restores the repaired .004108 error. This
checks ownership rather than assuming unmanaged Aestra draws join native OIT.

## Independent dense oracle

Three scenes use flat semantic mesh planes: 40 homogeneous red alpha-.125 layers;
40 amber additive layers; and 41 interleaved layers (20 red, 20 blue, one green
additive). The same seeded frame-30 geometry is tested with opposite cameras,
reversed source creation and local limits 1/8/32: 36 cases per run.

Analytic central 8x8 linear color is independently checked against ordinary GPU
reference draws with one particle per owner in correct depth order. HDR floating
intermediates avoid repeated 8-bit blend quantization, with no tonemapping/bloom;
final screenshots are sRGB. Flat coverage makes alpha explicit. These planes are
math fixtures, not realistic smoke textures or art acceptance.

Global average 16 and a 64 MiB guard deliberately isolate *local* overflow: 2,764,800
nodes / 33,868,800 node+head bytes at 480x360. Every case has over-sort pixels and
zero global drops/invalid links. Attempt, capacity, layer and diagnostic counts match
between the disabled-adapter control and final implementation in all 36 cases.

| Scene | Cases | Before maximum linear error range | After range |
| --- | ---: | ---: | ---: |
| Homogeneous smoke | 12 | .577325–.870438 | .004108 |
| Additive only | 12 | .098022–.487713 | .002887 |
| Interleaved alpha/emission | 12 | .402925–.521732 | .013250–.307343 |

The threshold stays **.01**. All 24 smoke/additive math cases pass in both final
runs. All 12 interleaved cases still fail the image bound. The test's `probe_passed`
accepts the math gate *and observation of the remaining failure*; its
`renderer_correct` is explicitly false. It must not be counted as 36 accepted
compositing cases. The 12 split-draw references are unchanged before/after, with
analytic errors below .004108.

The matched before control disables only `resolve::install(app)` and exits 1 after
writing all cases. The call is restored for both final runs. Final dense runs each
write 52 PNGs: 36 cases, 12 references and four ownership/control images. Forty images
repeat byte-exactly; the 12 interleaved images vary by up to six byte values. Their
approximation depends on fragment arrival order. Repeatability is not an image oracle.

## Regression and reproduction

Use a fresh absolute output root for each run. Run native GPU probes serially with
no competing GPU workload. For example, from the repository root:

```powershell
$env:AESTRA_CROSS_POOL_IMAGES = "$PWD/target/fireworks-f8/overflow-local-repro-1"
cargo test -p aestra-viewer oit_dense_overflow_preserves --locked --offline -- --ignored --test-threads=1
cargo test -p aestra-viewer fragment_list_ --locked --offline -- --ignored --test-threads=1
```

The final `fragment-list-overflow-final-2026-10-26-b` run passes the dense gate and all
ten existing image probes (260.78 seconds for the latter). All 1,694 prior accepted
non-degraded PNGs are unchanged and repeat exactly. Global-exhaustion and saturation
diagnostic images are excluded from that count. The old two-fragment local-overflow
control now positively requires error below .01, records over-sort, and has zero
whole-image linear difference; global loss remains an expected degradation.

Renderer unit tests pass (172, three ignored), viewer tests pass (109, 42 ignored),
and real ShaderCache composition passes (11, one ignored), including all six new
resolve local-limit/depth variants. Strict workspace/all-target GPU Clippy, isolated
shader-composition Clippy, both formatting checks and diff checks pass. Native
depth/prepass, HDR/lighting, texture/flipbook, mixed-draw and camera lifecycle controls
remain covered; the shader-only no-prepass variant is not broader host acceptance.

Earlier fixture attempts remain under the before/after dated roots listed in the
JSON. Rejected attempts used an 8-bit intermediate, demanded exact approximation
repeats, assumed constant soft-sprite coverage, or used an invalid unmanaged-draw
control. The final flat-mesh reference corrects those assumptions without relaxing
the image bound. The earlier in-progress report initially read as false completed
successfully; it was not evidence of a renderer regression.

## Remaining gate

**C4M2:** fix/bound interleaved early-blend ordering or select another bounded
compositor. Raising storage or the local prefix alone cannot repair its order
dependence. Re-run matched live costs/capacity after that choice. The
[C4L timings](fragment-list-live-2026-10-26.md) precede this repair and are not
post-change performance claims. Foreign/custom participation, arbitrary depth,
equal-depth ties, sub-viewports/resolution overrides and window lifecycle remain
outside qualification. Multi-camera overflow diagnostics remain unavailable.
Continuous star-path smoke and paced public-host HDR art review follow the
compositing decision; this change is not photo-approved fireworks smoke.
