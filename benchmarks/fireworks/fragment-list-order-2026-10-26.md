# F8.1C4M2 — order accepted overflow lists before resolve

The twelve interleaved cases rejected in [C4M1](fragment-list-overflow-2026-10-26.md)
now pass the unchanged .01 linear color bound. An overflow-only GPU merge sort reuses
the collected node links before the premultiplied resolve. This removes the local
early-blend ordering approximation on the qualified full-target path. Global storage
exhaustion and non-participating transparent draws remain separate limitations.
**Public defaults and saved fireworks assets are unchanged.**

The [JSON evidence](fragment-list-order-2026-10-26.json) records 53 source hashes, raw
reports, PNG manifests, per-case costs and the admission decision. Hardware is Windows,
RTX 4070 SUPER/Vulkan, Rust 1.98.1, Bevy 0.20.0 and wgpu 30.0.1. Generated raw artifacts
remain under the named local `target/fireworks-f8` roots.

## Implementation and limits

After transparent collection and optional diagnostics, one compute invocation handles
each pixel. It validates the linked chain, leaves lists within the local threshold
unchanged, and merge-sorts longer lists front to back in place. The existing resolve
then accumulates layers in their known order. Work is O(N log N) for an overflowing
list, with O(1) private state. This avoids a larger private array or repeated quadratic
selection. Validation adds an O(N) scan, including on short lists.

The pass uses the existing 12-byte nodes, heads and native dynamic View/settings
uniforms. It adds pipeline/bind-group state, **no fragment/index/uniform data buffer**
and no production readback. The unchanged 1..32 setting now selects a local fast path,
rather than limiting how many accepted layers receive correct ordering. Native node
capacity and camera/device memory guards still apply. Allocated capacity bounds the
validation scan; malformed or cyclic heads are cleared before resolve. This does not
promise bounded frame latency under pathological single-pixel overdraw.

Only Aestra-approved cameras dispatch this pass. Optional render resources and devices
with too few compute storage bindings are handled without mandatory resource panics.
The bind group is released when no cameras are approved. Full offscreen camera resizing,
two-camera reuse, rejection and opt-in removal/restoration are checked below. Native
unmanaged cameras keep their own resolver. Sub-viewports and resolution overrides skip
this pass and remain unqualified. Cold pipeline preparation can independently leave
sort/resolve on their earlier paths; there is no first-cold-frame color guarantee.

Sorting compares native packed reverse depth/alpha, with canonical color for ties.
It does not establish physical equal-depth semantics or recover depth precision absent
from the existing 24-bit representation. Diagnostics run before sorting: `over_sort_pixels`
now counts lists requiring fallback work, **not necessarily approximate blending**.

## Native correctness

The actual production shader passes 48 sparse lists at three thresholds (144 list
checks) through 4,097 nodes. Ascending, descending and seeded shuffled chains include
packed-depth ties, empty/padded pixels and unreachable canaries. GPU readback in this
isolated test verifies exact expected order, unchanged payload bits, no lost/duplicate
suffix, unchanged short chains and no writes into canaries. Invalid heads and cycles
are cleared in all six corrupt-chain controls. This is test-only synthetic readback.

Both dense image runs pass all 36 analytic and independent split-draw GPU references:
40 homogeneous smoke layers, 40 additive layers and 41 interleaved alpha/emission layers,
opposite cameras, reversed source creation and local thresholds 1/8/32. Maximum linear
errors are .004108/.002887/.002908 respectively. The interleaved error was
.013250–.307343 at C4M1. All 52 captures repeat byte-exactly; the thirteen reference
images (including native ownership reference) and earlier smoke/additive images are
unchanged. The first run preceded optional-resource/capability guards; the shader is
identical, and the second run uses the final renderer source.

All ten existing native probes pass (259.03 seconds), and all 1,694 earlier accepted
non-degraded PNGs remain byte-identical. Global-exhaustion and saturation diagnostic
images are excluded from that historical count. The former three-layer local-approximation
stage now positively asserts the analytic image bound while preserving its historical
filename. Global loss still fails the color oracle as intended.

## Current live costs and capacity

The unchanged C4L harness completes the 24-case matrix in 232.97 seconds: two saved high
workloads, 540p/1080p, ordinary/default/provisioned OIT and two counterbalanced repetitions.
Forward PlaybackOnly runs frames 360..600 with 240 absolute-deadline 60 Hz host updates;
no seek, replay or skipped player frame. Cost lanes disable diagnostic scans; separate
identical capacity lanes enable them. This is the LDR offscreen 6..10-second window,
not the public HDR/audio/window show or a production-finale budget.

All 20 admitted cost lanes and 12 capacity lanes complete. Four default 1080p cases
are explicitly rejected by the unchanged 32 MiB guard; their fallback is not reported
as OIT cost. Provisioned node/head storage remains 25.71 MiB at 540p and 102.83 MiB at
1080p. No additional sort-data allocation is introduced.

| Workload / target | Sorted GPU median | Provisioned OIT GPU median | OIT GPU p95 | Link-sort median / p95 |
| --- | ---: | ---: | ---: | ---: |
| Show / 540p | 3.623–3.840 | 4.477–5.505 | 9.665–9.715 | .295–.414 / .427–.609 |
| Show / 1080p | 3.521–3.596 | 4.199–4.279 | 7.955–8.135 | .396–.407 / .599–.628 |
| Four cohorts / 540p | 2.173–2.460 | 3.891–4.534 | 6.685–6.816 | 1.355–1.529 / 2.082–2.224 |
| Four cohorts / 1080p | 2.611–3.643 | 5.303–5.379 | 6.386–6.438 | 1.803–1.839 / 2.575–2.627 |

Values are milliseconds, ranges across two repetitions. Default 540p OIT is also
retained in the JSON: show median 3.852–3.952, four-cohort median 4.044–4.099. The local
threshold changes how much work remains in the resolve; a larger threshold is not
automatically faster. These are whole-render-graph observations, not isolated causal
overhead versus the old resolver. Do not add pass percentiles or subtract separate
medians as frame-aligned measurements. Async samples are deduplicated, not joined to
frame IDs; app.update excludes GPU completion and whole-render timing excludes much
preparation/game/audio/presentation work.

All 2,826 capacity observations have zero global drops/invalid links and exceed the
local threshold. They now require merge-sort work. The raw inherited
`observed_exact_capacity=false` field retains C4L's within-local-prefix/no-drop predicate;
it is not an image-correctness verdict for this implementation. Peak populations match
between paired lanes (617 show, 256 cohorts), all players reach frame 600, pipeline
pending counts are zero, observed checkpoint bytes/unknown windows are zero, and at
most two native fixed ticks are observed per simulation window.

**60 Hz pacing is not accepted.** Host deadline misses vary substantially, including
240/240 in one show/540p/provisioned run, 76 in one cohort/540p/provisioned run, and
96 in a sorted show/1080p control. These remain recorded without attributing them to
an unproven cause. Dense 1080p OIT has zero misses in both runs, but that does not erase
the other results. Setup repeats the existing LogPlugin/global-subscriber, Vulkan
overlay and no-tonemapping/dither notices; no shader is pending in measured windows.

## Reproduction and decision

Use fresh absolute report roots and run native GPU commands serially:

```powershell
cargo test --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --test shader_composition native_overflow_links_ --locked --offline -- --ignored --test-threads=1
$env:AESTRA_CROSS_POOL_IMAGES = "$PWD/target/fireworks-f8/order-local-repro-1"
cargo test -p aestra-viewer oit_dense_overflow_ --locked --offline -- --ignored --test-threads=1
cargo test -p aestra-viewer fragment_list_ --locked --offline -- --ignored --test-threads=1
$env:AESTRA_OIT_LIVE_REPORTS = "$PWD/target/fireworks-f8/order-live-local-repro-1"
cargo test -p aestra-viewer oit_live_paced_matched_cost_and_capacity --locked --offline -- --ignored --test-threads=1
```

Renderer tests pass (172, three ignored), viewer tests pass (109, 42 ignored), and real
shader composition passes (12, two ignored). The isolated GPU contract is run explicitly
in addition. Strict workspace/all-target GPU and isolated shader Clippy, both formatting
checks and diff checks pass. Initial test-only compilation issues with wgpu's mapped
range Result and the shared compiled-source handle were fixed before GPU qualification;
no image or timing threshold was relaxed.

C4M2 closes the observed accepted-list ordering defect and records its costs. It does
not authorize automatic show adoption. Next qualify a guarded opt-in in the actual
public-host HDR smoke show, audit all participating renderer paths, and measure full-show
pacing/costs before migration. Global capacity, foreign/custom transparency, arbitrary
depth, physical equal-depth ties, sub-viewports/overrides, window lifecycle and multi-camera
overflow diagnostics remain open. Continuous star-path smoke and photographic art review
follow; more replay optimization is not the priority.
