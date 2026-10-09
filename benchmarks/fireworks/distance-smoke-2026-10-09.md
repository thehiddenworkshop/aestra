# F8.1C4B — rocket-path smoke deposition

The opt-in public `--smoke-art-show` now leaves smoke behind the moving rocket instead
of launching smoke upward from the origin. This is a runtime/authoring correction,
not final photographic smoke approval. Default and C3 show assets remain unchanged.

## Generic runtime capability

`EventTrigger::OnDistance { spacing, max_per_tick }` works in event links and particle
output routes. It samples each source's resolved fixed-tick motion chord, carrying a
per-particle residual across ticks and checkpoints. There is no initial sample at
birth, and stationary sources emit nothing. Sampling uses effect-local distance;
it is not an exact curved trajectory or world-distance sampler under changing scale.

Spacing must be finite and at least 0.001, with 1–64 samples/source particle/tick.
Routes sharing a source must agree on both settings. Invalid authored or decoded
artifact settings fail validation. Overspeed crossings are dropped and consumed,
not deferred; the existing source-overflow counter includes these drops. Shared
capture remains bounded to 1,024 records/tick, and normal expansion, list-memory
and destination-admission budgets still apply. List sizing now accounts for multiple
samples from a small-capacity source rather than assuming one event per source slot.

GPU persistent state grows from 36 to 40 bytes/slot, including checkpoint copies;
presented particles remain 48 bytes. No new binding, host particle-tracking loop,
full-particle readback or playback-only history allocation is added. The Bevy GPU
backend wires settings automatically. CPU reference callers use
`set_distance_emission(compiled.distance_emission(source_index))`; this does not
claim complete Bevy CPU-renderer event playback. Dedicated editor controls are
deferred; RON/API authoring and loaded-trigger labels are supported. The additive
serialized trigger is rejected by older readers; the artifact envelope is unchanged.

## Saved smoke authoring

The four wispy shell variants link Launch shell → Launch smoke at one-unit spacing,
one child per crossing, limit eight and zero inherited velocity. The old fixed-origin
rate is zero. Smoke lives 8–10 seconds, grows from a thin wake and moves slowly under
shared wind/turbulence. Its clip window is extended to retain the deposited tail.
Capacities, seeds, tier policies and existing sound/light routes are unchanged.
Burst smoke remains the separate C4A draft; continuous star-path smoke is not yet authored.

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-art-show
```

## Native results

Two fresh, sequential all-tier runs of
`authored_wispy_smoke_show_lights_overlaps_and_drains` pass with unchanged pixel gates.
All **39 corresponding PNGs are byte-identical** (78 captures total). At 9.3 seconds,
seven retained children are inspected; at 18.4 seconds, twelve. Each inspected rocket
has **43 accepted and captured smoke births**, with zero expansion omissions,
destination rejections or source overflow. Admission evidence contains 19 observations
per tier/run. Earlier children have already retired at the later observation.

| Tier | 9.3s light/off pixels | 18.4s light/off pixels | 30s tail pixels | 30s live tail particles |
| --- | ---: | ---: | ---: | ---: |
| High | 6,082 | 2,436 | 4,759 | 137 |
| Medium | 4,413 | 1,330 | 2,743 | 67 |
| Low | 1,911 | 677 | 1,328 | 35 |

Frozen/light/density restoration, density-off and natural-drain image differences
are zero. Four retained late children have nonempty all-zero population observations
at 33 seconds. Sort ownership pairs/bytes are zero at 37 seconds. The 30-second
populations are the existing burst tail; launch smoke has already expired.

The final required-native distance conformance test passes: CPU/GPU sample positions
agree, children stay where deposited with zero inherited velocity, host output-route
samples agree, fresh repeats agree and all particles drain. Normal sampling reports
zero drops; an overspeed case deliberately reports **180** dropped crossings over
30 movements and does not accumulate a later backlog. The production GPU checkpoint
test passes backward seek/replay comparisons of residuals, children and presentation.
The full stateful conformance suite (31 tests) and domain-spawn suite (4 tests) also
pass, checking the expanded persistent stride. Artifact contract tests (12), core/
runtime/compiler/artifact/GPU unit tests and strict all-target workspace Clippy pass.

The public `public-2.6s.png` shows a lingering narrow gray rocket wake below the burst.
It is a 1280×720 HDR asynchronous development-window observation, not an exact-frame
reference or performance measurement. The HUD's roughly 16.7 ms includes vsync.
No incremental GPU-cost claim is made for this revision.

The [evidence JSON](distance-smoke-2026-10-09.json) retains six raw reports, all admission
observations and 111 SHA-256 hashes covering 20 source files, 78 native images, 12 raw
report/admission files and the public image. Run 1's report label still says C4A;
run 2 corrects the label and strengthens the per-rocket assertion to exactly 43.
Both use identical effect sources and produce identical captures. The earlier
[C4A evidence](smoke-transparency-art-2026-10-09.md) remains a historical pre-deposition snapshot.

## Reproduce

Use Rust `1.98.1-x86_64-pc-windows-msvc`. Native runs must be sequential and alone.

```powershell
$env:AESTRA_REQUIRE_GPU_CONFORMANCE='1'
cargo test -p aestra-bevy-render --test stateful_conformance gpu_distance_emission_deposits_stationary_smoke_bounds_work_and_drains --locked -- --test-threads=1 --nocapture
cargo test -p aestra-bevy-render --lib distance_smoke_checkpoint_restores_path_remainders_and_children --locked -- --test-threads=1 --nocapture
$env:AESTRA_WISPY_SHOW_IMAGES='C:/absolute/new-output-directory'
cargo test -p aestra-viewer authored_wispy_smoke_show_lights_overlaps_and_drains --locked -- --ignored --test-threads=1 --nocapture
```

## Remaining gates

Fine-tune deposited rocket smoke against the supplied references; author thin radial
star-path smoke, coherent drifting haze and convincing locally lit cloud depth.
Global interleaved/moving-pool transparency and matched paced full-show cost remain
open. This change does not provide OIT, fluid coupling, selected-star smoke lighting
or full F8/AAA acceptance. Keep the default show unchanged until those gates pass.
