# Emission grain for burning trails

The `assets/test` project includes **Emission Grain**, a reusable material
function, and **Grained Cooling Filament**, a ribbon material that calls it.
Open `effects/fireworks_gold_filament_candidate.aestra.ron` in the editor and
select the Main stars trail to inspect its Material Graph.

| Exposed effect control | Default | Meaning |
|---|---:|---|
| Tail burn grain | 0.85 | Uneven emission strength; zero restores uniform brightness |
| Tail grain frequency | 1.8 | Spatial detail per world unit, clamped to 0.01–16 |
| Tail color cooling | 0.55 | Additional warm tint as normalized particle age increases |

The function takes world position, a stable particle/strand seed and normalized
age. Two scales of continuous noise produce uneven burning along a strand;
screen derivatives fade unresolved detail rather than animating bright noise at
a distance. The pattern has no time input, no texture and no particle spawning.
Each strand has an independent phase. It is a world-space pattern: moving
geometry traverses it, so use another coordinate input when the pattern should
move with a surface.

The output is a linear Color multiplier, with alpha one. Multiply an existing
emission color by this output; retain the original opacity and coverage paths.
Grain and cooling both zero give a neutral multiplier. The saved
`fireworks_gold_filament_uniform.aestra.ron` provides this control using the same
trail history, particles and material shader as the candidate. The older
`fireworks_gold_filament_control.aestra.ron` retains the original short-trail look.

Trail normalized age is the recorded particle age at each history sample. The
renderer still supplies its normal retained-history fading. This tint is an art
control, not a thermal simulation or a new clock that cools each deposited point.
No fireworks-specific renderer, editor widget or runtime branch is required.

Trail width also multiplies the recorded particle size. The refined study uses
width 0.24 with an initial head size 0.22 (effective width about 0.053 world
units), trail radiance 52 and ember radiance 3. The first grained draft retained
width 0.075 and radiance 32/6, leaving its modulation barely visible at audience
distance. Inspect native pixels, not just nominal width, when tuning a thin trail.

The candidate remains an isolated visual study. Launch the VS Code task
**Aestra: Review Gold Filament Study** to review it in motion before applying the
look to other families. The offshore variety task keeps its existing shell art.

Native comparisons and launch-water controls are recorded in the
[F8 review](../../benchmarks/fireworks/f8-burn-grain-launch-light-2026-10-31.md).
