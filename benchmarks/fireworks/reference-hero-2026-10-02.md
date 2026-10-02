# F4M — reference-driven hero, 2026-10-02

**Technical checks pass; artistic, animation and AAA-density acceptance remain open.**
This is one authored shell, not a recreated photographic show or an F5 secondary-event test.

## User-provided target

All six supplied images were inspected, including temporary AVIF-to-PNG decoding for
inspection. Originals remain in the user's fireworks asset folder; photographs are not
copied into the project or distributed as Aestra assets.

| File | Useful visual target |
| --- | --- |
| `cannes-fireworks-beach-club-night.webp` | Primary palette/layer target: pink and warm gold radial tails, bright heads, irregular secondary spark activity and colored smoke. |
| `0923d3cd566d4ee049313724d7d22d55.avif` | Fine spark-rich white tails, isolated red embers, broad overlapping smoke. |
| `7e7ca217e877e04146c912bd6919622f.avif` | Distinct green/gold/red shells, slender curved tips and launch smoke. |
| `38c4a048bf470076148710005dc45afe.avif` | Fan-shaped launch layers, scattered gold points and blue/pink smoke. |
| `681155aa96812ab7e3311a912b58788c.avif` | Sparse branching structures, bright local centers, varied tail length and scattered points. |
| `1298273-le-feux-d-artifice-de-paris-2026-les-photos-du-spectacle.jpg` | Crossing low-level comet arcs, hot heads and colored trails; the drone formation is not a fireworks-runtime requirement. |

These photographs establish appearance, not calibrated radiance, shutter duration or temporal
behavior. Some streaks may integrate motion during exposure: do not turn them directly into
permanent gameplay ribbons. Water, architecture, sky, lasers, drones, audio playback and show
composition remain host work. Generic transient-light output and lit smoke remain F7/F8.

## Implemented with existing capabilities

- New ordinary project effect with stable identities, seven emitters and ten renderers;
  physical particle capacity 722. One rocket death requests five independent bounded targets:
  384 pink main stars, 96 slower gold stars, 128 loose cooling embers, one compact flash and
  48 smoke particles. No target independently emits a duplicate burst. This is one event
  generation; loose embers are **not** yet crackle or star-generated secondary sparks.
- Independent speed/lifetime/drag/appearance choices separate the layers. Color gradients for
  all seven emitters and star/trail radiance controls remain normal exposed effect parameters.
- Two project material programs: an age-cooling warm-white sprite core inside the colored
  head, and longitudinal ribbon-alpha taper (Stretch U: oldest 0, head 1) layered over the
  existing width/history fade. RGB gains remain clamped to 0..64, outside alpha/coverage.
  Sprite and trail artistic gains are 12 and 6; these are not physical watts.
- Main/inner histories use 60 Hz time sampling, 64 points, bounded owner counts and
  0.7/0.85-second lifetimes. The launch retains its short history. No runtime limit or
  sampling-policy default changed; no new material opcode or bitmap dependency is needed.
- Smaller/shorter flash and more visible unlit smoke. The independent launch plume and
  burst puffs are explicit approximations, **not** moving-particle attachment, transient
  illumination or volumetric transport.
- `f4-reference-hero` selects the saved asset in the viewer. Hero-specific whole-shell
  cameras leave baseline camera transforms untouched. Eye/target (world units):
  close `(0,25,160)/(0,18,0)`, audience `(0,35,200)/(0,20,0)`,
  wide `(0,45,250)/(0,25,0)`.

## Evidence and findings

Final evidence: `target/fireworks-f4/f4m-reference-hero-framed`, six cases / 48 frame PNGs.
Native GPU, RTX 4070 SUPER/Vulkan, dev build, 960×540, seed `0xf1e0000000000001`, fixed
60 Hz, high tier, playback-only, semantic materials, HDR/Tony/exposure 0/bloom 0.15.
Authored floors 0/0 and opt-in sampled floors 2/2 are compared separately.
The manifest records revision `083d11fc` plus dirty source paths/hashes: F4M is uncommitted
during these captures. Older evidence under `f4m-reference-hero` has late close-view cropping
and is superseded by the framed run, not silently overwritten.

All final cases passed the expected main/inner/ember/smoke peak checks. At frame 480,
live particles, occupied/retired histories, evictions and truncation were all measured zero.
This is endpoint/peak evidence, not per-tick demand auditing or live performance certification.
Viewer tests cover fixture reproducibility, normal project resolution/compiler/material
compilation, bounded event targets/history, appearance DAG dependencies and CLI selection.
Runner checks reject each missing hero cohort. Full viewer tests and warnings-as-errors
Clippy pass; two fixture exporters remain deliberately ignored.

Static inspection shows pink/gold separation, brighter heads, a smaller flash and more
visible smoke. The revised cameras contain the visible falling tails at sampled lifecycle
frames. Native-resolution images matter: authored narrow coverage is still dotted at distance,
while floor 2 improves continuity but changes apparent width. Cooling and radiance still need
moving-sequence review; the smoke remains far less rich than Cannes/London and lacks light
interaction. No goldens were approved and no licensed photographic pixels were used in shaders.

## Next bounded step

Proceed to **F5's generic two-generation event-chain and host-cue validation**: secondary
bursts/embers must originate from actual parent-particle events, retain position/velocity
context and work in forward playback without replay history. Host sound assets/mixing stay
outside Aestra. Keep F4 artistic acceptance and F7/F8 smoke/lighting gates explicitly open;
do not add more culling optimization based solely on these stills.
