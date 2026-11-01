# Fireworks demonstration

The newer reference frames expose a remaining realism gap: too much energy in
separate heads/glitter, too little connected cooling trail. The isolated
[gold-filament study](../../benchmarks/fireworks/f8-gold-filaments-2026-10-30.md)
now takes priority over adding more types. Run **Aestra: Review Gold Filament Study**
to compare that editor-ready draft. It is not adopted across the variety show;
burn breakup, cooling and temporal appearance still need visual review.

The study now includes a reusable [Emission Grain](../../docs/examples/emission_grain.md)
material function with exposed grain, spatial frequency and color-cooling
controls. `fireworks_gold_filament_uniform.aestra.ron` disables grain/cooling for
a matched control; the older `control` asset retains the original short trails.

Run the **eight-family variety sequence**:

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --launch-pattern variety --scene sea --tier medium --star-lights --experimental-oit --oit-partitions 4
```

The VS Code **Aestra: Run Fireworks Demo** task selects this medium-tier offshore art draft,
with selected-star/flash lighting, PlaybackOnly and experimental 4×4 transparency
under the unchanged 128 MiB guard. It launches 22 rockets across eight families:
point stars, gold tails, cyan rings, amber palms, golden willows, ruby crossettes,
silver crackle and violet strobe. Low fans, paired rings, secondary breaks, a
willow pause and a mixed-height closing phrase give the show distinct sections.
Timing, origin, launch velocity and fuse are host cues in `src/repertoire.rs`;
each family's particles, secondary breaks and smoke live in saved Aestra effects.

`--scene sea` supplies a dark sea, distant shore lights and a small launch barge.
All rockets start at its deck-mounted mortar banks. The host adjusts their launch
velocity to retain the cue's aerial break layout. The low shore camera has three
interactive views (1/2/3). Water now uses eight directional wave bands, filtered
microfacet roughness and water/air Fresnel response. `--water calm` and
`--water rough` select surface states independently of the VFX quality tier.
Its cached material follows the effect clock; pause/restart remain synchronized,
with no per-frame mesh/image creation or extra VFX owners/reflection playback.
The same [water receiver](../../docs/examples/water_preview.md) is available in
the editor's Preview settings. Add `--water-reflections` for the experimental
half-resolution additive particle/trail capture. Alpha smoke is excluded; the
same option is available as **Water reflections (experimental)** in the editor.
The low audience camera can place high aerial reflections below the frame;
the `water` report camera deliberately includes more of the sea.
The [initial offshore review](../../benchmarks/fireworks/f8-offshore-2026-10-31.md)
retains the earlier scrolling-normal baseline.
Burning rockets now carry a warm host-owned point light, illuminating the water
and deck while ascending. It follows the same gameplay transform and disappears
at detonation; `--no-launch-lights` supplies a sea-scene control. Burst and selected
star lighting retain their separate policies. This direct illumination is the
first step toward launch reflections, not a finished realistic water shader.
Omit `--scene sea` or use `--scene calibration` for the earlier stage. The frozen
`--showcase` preset retains its calibration scene and rejects the sea override.

Owner admission stays uncapped. This is a separate art candidate, with native
checks recorded in the [variety review](../../benchmarks/fireworks/f8-variety-2026-10-30.md).
It does not certify performance or replace final visual approval. Audio remains
optional and separately sourced; the existing host detonation sound binding is
retained. No new recordings or secondary-sound mix is supplied by this pass.

The frozen v1 candidate remains available through `--showcase`: high-tier tuned
12-shell overlap, selected-star/flash lighting and 4×4 experimental transparency.
Its conflicting overrides are rejected. Normal CLI defaults and the frozen
32-shell `--launch-pattern finale` workload retain their earlier behavior.

```powershell
cargo run --release --locked -p aestra-demo-fireworks
cargo run --release --locked -p aestra-demo-fireworks -- --cinematic
cargo run --release --locked -p aestra-demo-fireworks --features fireworks-audio -- --audio-root C:/Sounds/fireworks
```

Default: the host schedules five rockets and accepts Enter to launch more. The host
owns origin, initial velocity, gravity, fuse and detonation in `src/host.rs`. A game
would substitute its physics, collision or networking decisions here. Each rocket
has one gameplay entity; Aestra owns its particles. A stationary VFX owner binds to
the rocket and leaves old smoke behind. Stopping emission preserves the tail; a
separate burst starts at the exact host detonation position. No GPU readback drives
gameplay. Root-level world transforms are published before Aestra resolves bindings;
hierarchical hosts must propagate their hierarchy before that boundary.

The six added repertoire assets are `assets/test/effects/fireworks_variety_*.aestra.ron`.
Open the `assets/test` project in the editor to edit them; they render independently
of the demo host, including their own launch and burst. Ring/Cone/Sphere velocity
distributions, cooling curves, distance-deposited smoke and delayed secondary
event links use existing generic Aestra authoring. All six share the current
advected smoke kernel and exposed pattern/feather/breakup/depth controls.
To regenerate only these six saved assets:

```powershell
cargo run --release --locked -p aestra-demo-fireworks --bin author_fireworks_variety
```

Variety uses the reference point-star/gold assets and its saved repertoire; the
`--tuned-smoke` flag does not switch its art. For native image review use
`run-smoke-look-review.ps1 -Mode host-variety -Variants tuned` with a fresh output
folder. There is no control/tuned art comparison defined for this sequence.

Space pauses, R restarts, 1/2/3 changes the camera, L toggles burst lights, M mutes,
Esc exits. Host admission has no default owner-count limit. Qualification runs
can request an explicit `--max-owners 24` budget, including retained visual tails;
this is a demo host policy rather than an Aestra runtime limit.
Instances use PlaybackOnly by default; `--history replay-enabled` opts into VFX
history, without claiming deterministic gameplay replay of this live host controller.

Opt into bounded moving Main-star lights with `--star-lights`. This loads the
separate `fireworks_point_star_lit_smoke` / `fireworks_gold_tail_lit_smoke` art,
without changing the baseline assets. Use `--no-flash-lights` for selected-only
lighting. L toggles flashes and P toggles selected lights independently; P requires
the opt-in plugin at launch. Native selection stays on the GPU, with no selected
position readback, shadows or gameplay decisions. The HUD labels capacity bounds
explicitly; those are not live-light counts.

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --star-lights
cargo run --release --locked -p aestra-demo-fireworks -- --star-lights --no-flash-lights
```

`--tuned-smoke` selects the separate eroding-mask/slow-expansion draft.
`--launch-pattern overlap` selects twelve overlapping host launches, with fixed
lanes, depths, fuse lengths and a gold/blue palette. Neither option is an engine
instance limit. For example:

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --star-lights --tuned-smoke --launch-pattern overlap
```

The [host overlap review](../../benchmarks/fireworks/smoke-look-host-overlap-2026-10-28.md)
records art comparisons, all-tier cleanup and timing outliers. Its runner explicitly
opts into an owner budget; normal demo admission remains uncapped by owner count.

`--cinematic` retains the original authored show. Existing explicit effect, smoke
candidate and lighting-lab flags select cinematic playback too. Qualification
reports support both independently clocked host effects and authored timelines.
Experimental OIT remains opt-in for its existing qualified candidates. The host
default uses ordinary depth-sorted transparency and is not final smoke art approval.

The canonical editable effects/materials are shared with editor/viewer tests in
`../../assets/test`, rather than duplicated into another project. `--project` selects
another asset root; host mode expects its calibrated `fireworks_point_star_smoke`
and `fireworks_gold_tail_smoke` effects. `src/host.rs` splits those sources at load,
applies the authored tier first, and compiles immutable wake/burst plans once.
The original saved art is preserved. Offline authoring utilities are `src/bin/author_*`:

```powershell
cargo run --locked -p aestra-demo-fireworks --bin author_fireworks_shell_smoke
cargo test --locked -p aestra-demo-fireworks --all-targets --features fireworks-audio
```

Optional WAVs retain their separate licensing; no recordings were moved or bundled.
For the separate release pacing, timestamp and image baseline:

```powershell
cargo build --release --locked -p aestra-demo-fireworks
& ./benchmarks/fireworks/run-showcase-baseline.ps1 -OutputDirectory target/fireworks-showcase-baseline
python ./benchmarks/fireworks/summarize-showcase-baseline.py target/fireworks-showcase-baseline --output-prefix target/fireworks-showcase-baseline/summary
```

The runner checks embedded release/optimization metadata, records source/binary
hashes and runs images, excluded warm-up, and three alternating ordinary/profile
pairs. It fixes audio off and the audience view, and adds no owner limit. Partition
health remains enabled in every lane. Read active launch/spark phases separately
from smoke drain/idle; do not use the whole-show percentile as an active-load budget.
Run without other GPU workloads or builds. Profiling includes an explicit
render-graph GPU interval and CPU encoding wall interval; those exclude presentation
and are not sums of nested pass percentiles. Ordinary intervals include vsync.
The [release v1 baseline](../../benchmarks/fireworks/showcase-v1-release-2026-10-29.md)
records three per-phase timing pairs and separate image/health/birth controls.

To investigate shared simulation work, use a separate phase-sampling lane:

```powershell
& ./benchmarks/fireworks/run-smoke-look-review.ps1 -OutputDirectory target/fireworks-simulation-phases -Binary target/release/fireworks.exe -Mode host-finale -Variants tuned -Tiers low -Transparency shared -Partitions 4 -Cost -Instrumented -SimulationPhases
```

This adds `profile.simulation_phase_sampling` for particle ticks, event routing,
presentation and trail recording. The shared `GpuSimulationPhaseTimings` resource
is disabled by default and available through `aestra_bevy::gpu` for other hosts
and editor profiling. Enable it with `set_enabled(true)` and deduplicate `samples()`
by phase/sequence. It rotates one category per frame, sums disjoint segments and
reports partial windows as unknown. Storage is bounded to 1024 pairs and three
in-flight slots; this does not limit effects or owners. Unsupported/busy sampling
skips diagnostic windows without blocking simulation. Extra GPU marker passes
affect timing: compare performance with `-SimulationPhases` omitted, and never
add category percentiles. Domain advance and analytic/trail preparation are outside
these category windows. The [event-pass review](../../benchmarks/fireworks/f9-event-passes-2026-10-30.md)
records the first investigation and shared runtime change.

See [the full guide](../../docs/examples/fireworks.md) for content packaging, audio,
visual candidates and remaining F8 qualification. For a short learning example use
`cargo run -p aestra-bevy --example host_projectile`; `fireworks_event_chain` teaches
particle lifecycle links separately.

### F8 gold smoke reference draft

The explicit `--launch-pattern finale` is a repeatable **32-shell** workload:
alternating point-star/gold rockets launch every 0.15 seconds, followed by finite
wake/burst tails within the 40-second host clock. It is a named test schedule;
omit `--max-owners` to retain unrestricted admission, including manual launches.
It uses the same host trajectories and editable VFX assets as overlap mode.
`--showcase` continues to select its frozen 12-shell v1 pattern.

After generating the reference review project as shown below:

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --launch-pattern finale --tuned-smoke --star-lights --experimental-oit --oit-partitions 4 --project target/fireworks-reference-preview --tier medium --no-audio
```

The initial [tier/finale qualification](../../benchmarks/fireworks/f8-frozen-tiers-finale-2026-10-30.md)
retains the high-tier capacity failure and all-tier timing misses. This command
is a stress case, not a certified 60 FPS preset.

The shared editor material now also exposes **Smoke depth variation**. Enabled,
it uses a three-dimensional density field; disabled, it retains the projected
control. **Smoke Material Lab** and **Smoke Wake Lab** in `sample-project` expose
the same toggle, alongside density, feather, breakup and pattern velocity. See
the [editor guide](../../docs/examples/advected_smoke.md) and
[depth review](../../benchmarks/fireworks/f8-smoke-depth-editor-2026-10-30.md).

The separate filament-smoke candidate changes smoke masks and size/opacity curves,
using a coarse world-space density field shared by overlapping sprites and a soft
fresh core on launch/burst puffs. Dense star paths retain erosion without the core.
The pattern now advects using effect time and the exposed `Smoke pattern velocity`
vector (default `[0.6, 0.12, 0.08]` world units per effect second). This changes
procedural density only; particle motion still comes from the authored modules.
Zero velocity restores the anchored pattern. Set velocity at spawn; changing it
later rephases `world - velocity * effect_time` rather than integrating weather.
The clock follows playback, so the pattern pauses/restarts with the effect.
`Smoke edge feather` softens the rim and reduces age-driven rotation. Its default
is 0.75; zero retains the previous envelope. The editor's
[Smoke Material Lab](../../docs/examples/advected_smoke.md) shares this kernel and
adds editable density plus a complete material graph/function.
It preserves particle births, simulation, lifetimes and lighting. The frozen
v1 showcase assets remain unchanged. Preview a complete gold shell, the point-star
family, or the saved gold-then-blue calibration pair:

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show --effect effects/fireworks_gold_tail_reference_smoke.aestra.ron --experimental-oit --star-lights --no-audio
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show --effect effects/fireworks_point_star_reference_smoke.aestra.ron --experimental-oit --star-lights --no-audio
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show --effect effects/fireworks_shell_reference_smoke.aestra.ron --experimental-oit --star-lights --no-audio
```

To review the candidate with gameplay-owned overlap, create a fresh project overlay
and load it through the existing project API:

```powershell
cargo run --release --locked -p aestra-demo-fireworks --bin author_fireworks_reference_smoke -- --review-project target/fireworks-reference-preview
cargo run --release --locked -p aestra-demo-fireworks -- --showcase --project target/fireworks-reference-preview --no-audio
```

The overlay directory must not exist and must be outside the source project. Its
gold and point-star effects retain their existing IDs so host/pair references resolve; the source
project is never replaced. Running the author without arguments regenerates the
five checked-in candidate assets. The point-star source retains its warm-white
default palette; blue is an unchanged override in the pair and host overlap.
The review runner accepts `-ProjectRoot` and
fingerprints that actual asset tree.

The [first F8 reference review](../../benchmarks/fireworks/f8-gold-smoke-reference-2026-10-29.md)
records the earlier UV-noise draft. The
[motion follow-up](../../benchmarks/fireworks/f8-coherent-smoke-motion-2026-10-29.md)
compares that draft with the current world-space revision, preserving rejected
capacity runs and separate costs. This draft is not the new default or final AAA
acceptance. The [mixed-family review](../../benchmarks/fireworks/f8-blue-gold-smoke-2026-10-29.md)
extends the same material to the point-star family, with narrower smoke paths,
unchanged gold art, preserved failed overlap evidence and separate costs.
The [advection review](../../benchmarks/fireworks/f8-smoke-advection-2026-10-29.md)
compares moving and zero-velocity patterns under identical workloads. Remaining
billboard texture, physically coupled flow and final art acceptance stay open.
The [editor/runtime follow-up](../../benchmarks/fireworks/f8-smoke-feather-editor-2026-10-30.md)
records the feather control, sample-project integration, graph/thumbnail tests and
native editor-viewport proof.

The [shared procedural-noise study](../../docs/examples/procedural_noise.md)
adds a separate `effects/fireworks_noise_smoke.aestra.ron` comparison, using
the editor's built-in Noise nodes inside a reusable graph function. Open
**Noise Smoke Lab** in `sample-project` to author the same material. The study
preserves the source particle workload; it does not replace the repertoire.

The [Spatial Flow study](../../docs/examples/spatial_flow.md) adds
`effects/fireworks_spatial_flow.aestra.ron` and a matched zero-strength control.
The same shared Noise/Curl module is available in the editor's Particle Update
palette; **Spatial Flow Wake Lab** demonstrates deposited smoke independently of
the host choreography. See the [N2 native review](../../benchmarks/fireworks/n2-spatial-flow-2026-11-01.md)
for parity, per-field GPU costs, preserved births and current World/replay limits.

### Shared launch layouts (N3)

Open **Particle Sampling Lab** in the editor's sample project. Its ordinary
Particle Sampling module controls independent position/direction layouts, seed,
bounded jitter and direct-emission pattern length. Variable-size event/input
bursts use requested fan-out per source, without an owner ceiling or new history.
See the [sampling guide](../../docs/examples/particle_sampling.md).

The opt-in `effects/fireworks_sampling_control.aestra.ron` and
`effects/fireworks_sampling.aestra.ron` studies in `assets/test` compare Uniform
and Stratified main-star directions on the same gold cooling-filament shell.
Materials, ballistics, ranges, capacities and event settings are unchanged;
downstream distance-trigger demand can vary with the new trajectories.
Regenerate with `cargo run --locked -p aestra-demo-fireworks --bin author_particle_sampling_study`.
