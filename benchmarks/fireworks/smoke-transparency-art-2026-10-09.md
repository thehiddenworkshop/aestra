# F8.1C4A — bounded transparency oracle and wispy smoke art draft

The public example now has a separate editable `--smoke-art-show` draft. Its smoke is
less spherical in the HDR comparison, but this is **not** final AAA smoke, full cross-draw
transparency or production performance acceptance. C3 and the default show stay unchanged.

## Native separated-pool source-over oracle

`apps/aestra-viewer/src/volume_light_images/cross_draw_smoke.rs` creates ordinary compiled
single-particle Alpha materials: pure red and blue, each with alpha 0.5. Separate players
sit at world z=+2/-2. The camera at z=+16/-16 reverses which sheet is nearest; both spawn
orders are tested in fresh apps. The 480×360 LDR target uses `Tonemapping::None`, no bloom
and no lights. Screenshot readbacks are test tooling, not a runtime sorting path.

For the central 8×8 pixels, sRGB bytes are decoded to linear. Single-sheet captures must
each recover alpha 0.5 ±0.01. Near-red source-over is `(a, 0, b*(1-a))`, near-blue is
`(a*(1-b), 0, b)`. All four cases have maximum linear error **0.0001677722** against that
reference, versus **0.2469424456** against the deliberately reversed ordering. Both-sheet
restoration is exact. The accepted thresholds are <0.01 correct-order error, >0.2
wrong-order error and at most one byte of restoration error.
Two fresh native launches match all 16 PNGs exactly.

This is an actual blend-order oracle, not just matching two screenshots. It certifies
separated sheets only. It does **not** qualify overlapping/moving emitter-local depth
bounds, intersecting pools, arbitrary cameras/layers or a global particle order.

An ordinary algebraic regression records the architectural limit: far-red, middle-blue,
near-red particles at alpha 0.5 give exact linear `(R=.625, B=.25)`. Combining both reds
into one draw gives alpha .75; the two possible whole-draw orders give `(.75,.125)` or
`(.375,.5)`. Neither is correct even with perfect pool centers. This is an algebraic
counterexample, **not** a native interleaved-pool measurement. No global sort/OIT, shader
blend change or renderer workaround is introduced in this slice.

## Saved public HDR art draft

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-art-show
```

The new root and four shell variants reference one shared lit-wisp material/function.
They preserve C3's 13 clip times/transforms/seeds/overrides, event and sound/light routes,
capacities, tier budgets, 12–14-second smoke lives, 18-second child windows and 37-second
show duration. Non-burst-smoke emitter definitions remain equal; launch smoke also uses
the new shared material. Representative caps remain 8/4/2, selected-star lighting stays
disabled, and history stays playback-only. Flags exclude C3/smoke labs; explicit effect
paths still override the flag's default.

Only smoke appearance/motion changes:

- Burst origins: point → sphere radius 9; outward speed 1–2 → 2.5–5.
- Drift/gravity `(0.08,0.16,0)` → `(0.35,0.12,0.08)`, drag .6 → .35, turbulence .35 → .8.
- Size keys: 5/12/18 → 2/7.5/12 at normalized ages 0/.35/1.
- Opacity: retain .2 at age .12, add .18 at .7, then fade to zero at 1.
- Seeded three-octave mask: frequency 6.2, seeded rotation, anisotropy (.75,1.45),
  noise-warped/torn edge and stronger internal density variation.
- Ambient: `(.08,.085,.10)` → `(.055,.060,.070)`; scene gain .02 and Density 1 unchanged.

Public 1280×720 HDR/bloom captures on Windows/RTX 4070 SUPER/Vulkan compare the original
gray balls at about 10.9 seconds with the final dispersed, broken-up wisps at 10.9 seconds.
A final 9.3-second image shows the older cloud colored by later bursts. These are
**asynchronous real-time development-build window captures**, not fixed-frame regression
references. The HUD reads approximately 16.7 ms including vsync; this is not isolated
GPU timing, an entire-show percentile or a production 60-fps guarantee. No audio runs or
matched performance benchmark were added in this slice. C3's old costs cannot be assigned
to the changed art workload.

The shape is a better art starting point, not photographic approval: it still reads as
a localized burst cloud rather than the fine continuous star-path smoke in the references.
The current particle material is not a volumetric self-shadowing cloud.

### User-provided reference targets

The six additional reference photos supplied on 2026-10-09 refine the acceptance target;
they are visual references, not repository assets or licensed textures. The last image
most clearly separates thin radial star-path smoke, wider rocket exhaust and older
drifting haze. The Paris/harbor views show an asymmetric accumulated layer rather than
one isolated spherical puff per burst. The Sydney/tower views show strong, spatially
varying warm/red illumination of that haze, with dark unlit regions still retained.

The next art pass must therefore show smoke deposited behind moving stars, initially
thin then widening/curling; coherent shared wind across overlapping cohorts; older haze
that persists and is relit by later bursts; and local colored light falloff rather than
uniform gray or emissive smoke. Clouds must not retain circular/quad outlines or look
like unrelated stationary noise patches. Judge both bright-burst and quiet-tail phases
from audience and close cameras. Photographic exposure integrates moving sparks over
time, so do not equate long bright streaks with opaque runtime smoke or globally increase
ambient brightness to match the photographs. City/water illumination remains host-side.

## Unchanged native lifecycle/lighting gates

The C3 test helper is reused for the new saved root/material, with the same thresholds.
It keeps actual authored representative outputs, full audience camera, fixed-grid forward
playback, 480×360 LDR and no host-injected smoke. Captures freeze at root times 9.3/18.4/30/33/37.

| Tier | 9.3s lit/off changed pixels | 18.4s lit/off changed pixels | 30s tail changed pixels |
| --- | ---: | ---: | ---: |
| High | 5,717 | 2,436 | 4,759 |
| Medium | 4,001 | 1,330 | 2,743 |
| Low | 1,710 | 677 | 1,328 |

All differences exceed the unchanged 100-pixel gate. Frozen/light/density restoration
and density-off/natural-drain controls are byte-exact. Four still-owned late children
have nonempty, all-zero population observations at 33s; sort pair/buffer ownership is
zero at 37s. Tail populations stay 137/67/35 across high/medium/low, not extra particles
added to pass the visibility check. This is bounded lifecycle/lighting qualification,
not inter-draw order or final show-art approval.
Two fresh all-tier native launches pass and match all 39 PNGs exactly. The
[evidence JSON](smoke-transparency-art-2026-10-09.json) retains all 11 raw reports,
13 source hashes and 154 image hashes, including rejected iterations and public captures.

Two failed iterations are retained, not overwritten:

1. `wispy-show-controls-2026-10-09`: size 2/6/9, ambient .035/.038/.045 and original
   linear opacity fade. High tail had only **41** changed pixels; lighting/cleanup passed.
2. `wispy-show-controls-refined-2026-10-09`: size 2/7.5/12, ambient .055/.060/.070,
   original fade. High tail **379** passed, medium tail **75** failed; low was not run.

The final density plateau fixes the failed persistence check without weakening its
threshold, changing lifetimes or increasing budgets. Initial HDR draft images remain
separate from final HDR images. Evidence JSON records raw reports, hashes and limits.

## Reproduce

Use Rust `1.98.1-x86_64-pc-windows-msvc`; native tests run alone, sequentially, with fresh
absolute output directories. Substitute a different output directory for repetition.

```powershell
$env:AESTRA_CROSS_DRAW_IMAGES = 'C:\path\to\aestra\target\fireworks-f8\fresh-cross-draw'
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-viewer separate_alpha_pools_match_source_over_from_both_views_and_spawn_orders --locked -- --ignored --test-threads=1
$env:AESTRA_WISPY_SHOW_IMAGES = 'C:\path\to\aestra\target\fireworks-f8\fresh-wispy-show'
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-viewer authored_wispy_smoke_show_lights_overlaps_and_drains --locked -- --ignored --test-threads=1
$env:AESTRA_EXAMPLE_CAPTURE = 'C:\path\to\aestra\target\wispy-hdr.png'
$env:AESTRA_EXAMPLE_CAPTURE_SECONDS = '10.9'
cargo +1.98.1-x86_64-pc-windows-msvc run --locked -p aestra-bevy --example fireworks -- --smoke-art-show --no-audio
```

Ordinary tests: public audio-enabled example **18 passed**; viewer **100 passed, 28
native/export tests ignored**. Strict scoped Clippy (`-D warnings`) and formatting pass.
Bevy readback-channel-closed messages during public-window shutdown and Vulkan overlay
layer warnings occurred; captures completed and processes exited successfully.

**Next:** native interleaved/moving-pool counterexamples and a measured global-compositing
decision; continuous smoke deposition along star paths, then public HDR art/paced-cost
review against the references. Keep the original controls, playback-only defaults and
explicit opt-ins. F8 remains in progress; generic fluid injection, continuous selected-star
lighting, production-finale scalability and final art are not closed by this slice.
