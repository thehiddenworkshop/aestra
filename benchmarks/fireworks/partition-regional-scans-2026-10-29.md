# Regional sort and health scans — 2026-10-29

Active partition rendering now dispatches link sorting and health inspection
over **only the current rectangle**, using its origin for full-target pixel
addressing. Rounded workgroups are clipped against the rectangle extent, so odd
edge tiles cannot inspect or sort their neighbours. Capture and resolve keep the
same scissors, projection, shader quality and accepted layers.

Separate `main` and `region_main` entry points share the same sort/traversal
functions. The ordinary entry points retain their existing binding requirements;
only regional entries consume the rectangle uniform. The region's first local
invocation records attempted fragments even when its origin is away from pixel
zero. Admission waits for the regional sort pipeline as well as the existing
capture/resolve prerequisites.

One aligned immutable rectangle buffer is cached by the admitted rectangle list
and shared by sort and health bindings. It is independent of readback slot
availability, so diagnostic backpressure cannot prevent sorting. Bounds are
rebuilt on resize/grid changes; the three-slot diagnostic pool remains bounded.
For 4×4 on the measured device, bounds add 4 KiB of uniform data outside the
unchanged node/head storage budget. Per-region sort bind groups are prepared
each frame; total CPU preparation/driver allocation is not certified here.

The prior empty-phase path still performs one full-target reset audit. Rejection,
sticky observed loss, memory ceiling, density, owner policy and ordinary default
are unchanged.

## Work and correctness

For the reviewed 1280×720 target with 4×4 tiles:

| Compute workgroups per rendered frame | Full-target scans per tile | Regional scans |
|---|---:|---:|
| Sort, 8×8 | 230,400 | 14,720 |
| Health, 64×1 | 230,400 | 14,400 |

This removes roughly 94% of scan invocations, mostly empty-pixel work. Accepted
fragment traversal, long-list ordering, draw count and repeated geometry remain.
The dispatch reduction is not a corresponding GPU-time speedup.

Three native executions each pass 12 exact display-image comparisons at
480×360/odd 257×193, opposite views and grids 1×1/2×2/4×4. All captured pixels
also match the preceding empty-optimized binary's frozen controls. Regional
attempt totals, long-list pixel totals and maximum capped depth match ordinary
full-target diagnostics exactly. Per-region rows are retained in the ledger.
Overflow/recovery, unsupported-view/foreign rejection, repeated empty resets,
sticky loss and visibility/ordinary-mode restoration controls continue to pass.

The updated isolated raw Rgba16Float oracle is also run explicitly. It uses the
production regional entry rather than injecting its own bounds logic, retaining
the full-list/partitioned HDR equality and deliberately lossy negative control.
This remains separate from native SDR captures of HDR/Reinhard rendering.

## Cost and live qualification

The [JSON ledger](partition-regional-scans-2026-10-29.json) records three alternating
baseline/candidate near-view profile pairs and the separate live image controls.
The baseline executable was preserved before this change; its SHA matches the
preceding empty-phase review. New baseline run manifests describe the current
runner checkout, not a reconstruction of old binary sources.

Profile comparisons use high-tier tuned host overlap, PlaybackOnly, selected
star/flash lighting, 4×4 tiles and the existing explicit 24-owner benchmark policy.
Normal host admission remains uncapped. The aggregate GPU span includes health,
clears and result copies; bounds/bind-group CPU preparation occurs outside that
span. Reported ranges are three per-run p95s, not pooled percentiles or sums of
child spans. Live phase classification is asynchronous; host intervals include
vsync. No whole-game/frame-budget acceptance is inferred from these measurements.

| Aggregate partition GPU p95 | Full scans, three runs | Regional scans, three runs |
|---|---:|---:|
| Active fireworks | 13.699–13.740 ms | **12.702–12.769 ms** |
| Idle | 0.160–0.165 ms | 0.163–0.165 ms |

Active p95 falls about **7%** in the paired runs; the existing empty-phase
improvement is retained. Host interval p95 including vsync changes from
20.258–20.431 to 19.385–19.780 ms. It remains above 16.67 ms, so this does not
meet a 60 FPS budget or establish CPU render-preparation costs independently.

The three live image runs deliver 15 captures. High 4×4 near/orbit observes zero
drops/invalid links at **47,923,200 node/head bytes**. The unchanged 2×2 near
negative control still drops **6,841,223 fragments** at peak and remains failed.
Every image run preserves **374,514 gold-tail ember and 115,283 star-path smoke
births**, with no source overflow, expansion omission or destination rejection.
All six profile and three image runs launch/detonate 12 rockets, accept 12
flashes and drain to zero particles/gameplay rockets/VFX owners. Live captures
are not claimed to be exact frame-paired comparisons.

## Validation and next work

187 renderer tests, strict scoped Clippy and workspace formatting pass. Native
oracles run explicitly rather than treating ordinary-suite GPU ignores as passes.
The unchanged 128 MiB ceiling applies to node/head storage, not total VRAM.

Next measure and reduce repeated capture/geometry work with the same native image,
route and capacity controls. Mixed/custom scenes, general hardware/window
lifecycles, production-finale budgets and final smoke art remain separate gates.
Partitioned transparency remains opt-in.
