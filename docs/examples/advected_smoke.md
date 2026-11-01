# Advected smoke in the editor

Open `sample-project`, then open **Smoke Material Lab** from its effects folder.
This is an ordinary editable effect with one sprite emitter, no fireworks events,
host outputs or dependency on the demonstration executable. It emits for six
seconds, leaves a finite tail, and restarts after fourteen seconds.

| Exposed control | Default | Behavior |
|---|---|---|
| Smoke density | 0.75 | Multiplies material opacity without changing births |
| Smoke edge feather | 0.75 | Softens the envelope and reduces age-driven puff rotation; shader-clamped to 0–1 |
| Smoke billow breakup | 0.65 | Uses shared coarse/detail density to contract puff edges unevenly and fragment older smoke; shader-clamped to 0–1 |
| Smoke depth variation | On | Varies the density independently along all three world axes; Off retains the projected-field control |
| Smoke pattern velocity | `[0.6, 0.12, 0.08]` | Advects density in world units per effect second |

Zero feather retains the original rim/rotation range. Zero velocity anchors the
density field. Changing velocity mid-playback rephases `velocity * effect_time`;
set it before playback or restart after changing it. Particle forces and trajectory
remain separate Motion-module controls. This is procedural sprite smoke.

Zero breakup retains the previous feathered mask. Breakup reuses the existing
three noise octaves; it does not add particles, enlarge the original envelope or
change particle simulation. The effect is deliberately weaker on fresh wake cores
to keep a newly deposited trail connected. It remains a sprite approximation.

Depth variation removes the projected field's oblique repetition direction.
It keeps the same advection, feathering, breakup and fresh-core controls. Both
modes use three noise octaves; the depth mode interpolates eight corners per
octave instead of four. The toggle is a typed Boolean effect parameter, so hosts
can switch it without replacing the compiled material. It changes density sampled
on sprite surfaces; it does not add volumetric depth, self-shadowing or fluid flow.

Open **Smoke Wake Lab** to inspect a moving-source version. Its invisible source
travels on a slanted ballistic path and dies at 1.3 seconds. A distance event
deposits one smoke particle per world-space unit with **zero inherited velocity**;
the smoke emitter has no independent rate or burst. Deposits drift slowly, widen
and dilute for 8–10 seconds before the 14-second loop restarts. The lab uses the
same material and controls as Smoke Material Lab, without a demo or host runtime.
The source trajectory and wider smoke sizes are lab-only readability choices;
the fireworks candidate keeps its existing motion, births and lifetime contracts.

Open **Advected Smoke** in the Material Graph to inspect the complete material.
**Advected Smoke Density** is a reusable function available in the graph palette
and as a function asset. The supplied graph connects UV, seed, normalized age,
world position, effect time, pattern velocity and envelope controls. Density,
ambient response and scene-light gain remain separate graph parameters. The sample
uses brighter ambient response than the night-time demo, so it needs no scene lights.

To reuse it in another project, copy both files from `sample-project/materials`:

- `advected_smoke.aestra.material.ron`
- `advected_smoke_density.aestra.material-function.ron`

Assign the program to a sprite renderer and bind parameters as shown in
`sample-project/effects/smoke_material_lab.aestra.ron`. The function dependency
must accompany the program.

The editor routes this shipped, bounded function to native material thumbnails,
effect thumbnails and effect hover previews because the CPU evaluator cannot
execute custom WESL. Effect thumbnail cache keys include referenced function
definitions, so changing the function invalidates its cached image. Recognition
checks exact shared source, not UUID/name, and limits direct calls to four.
Arbitrary edited custom source retains the existing background-preview restriction;
use the viewport for that source. This is not general automatic custom-shader preview.

The canonical kernel is `crates/aestra-gpu/src/shaders/advected_smoke.wesl`, exported
as `aestra_gpu::shader::ADVECTED_SMOKE_WESL`. The sample and demo functions embed
that same source. The offline author utility regenerates shipped fixtures:

```powershell
cargo run --release --locked -p aestra-demo-fireworks --bin author_fireworks_reference_smoke
```

Regeneration overwrites those fixtures; make project-local copies for your work.
Normal editing/playback does not depend on that utility or the demo.

The [qualification](../../benchmarks/fireworks/f8-smoke-feather-editor-2026-10-30.md)
records editor graph discovery, native thumbnail preparation, native viewport
captures and unchanged demo workloads. Full manual UI and final art acceptance
remain separate.

The [breakup review](../../benchmarks/fireworks/f8-smoke-breakup-editor-2026-10-30.md)
adds zero-breakup controls, rendered thumbnails/hover checks for both editor labs,
and native show capacity, lighting and cost observations.

The [depth review](../../benchmarks/fireworks/f8-smoke-depth-editor-2026-10-30.md)
records projected/depth controls, side/orbit/near views and a separate timing pair.

The [frozen tier review](../../benchmarks/fireworks/f8-frozen-tiers-finale-2026-10-30.md)
checks both editor labs at medium and low without retuning the material. The
associated 32-shell demo stress case exposes runtime/transparency limits; it is
separate from opening or using these reusable smoke assets in the editor.

The [shared spawn-dispatch optimization](../../benchmarks/fireworks/f9-spawn-dispatch-2026-10-30.md)
reduces empty stateful spawn work without changing the material or authoring
controls. Rebuilt editor-viewport captures of both labs match the preceding
version's RGB pixels exactly.
