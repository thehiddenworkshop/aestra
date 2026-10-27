# Fireworks video reference review — 2026-10-27

The runtime has a useful working foundation, but the current saved wind-haze
candidate is not yet a visual match for this reference. The next useful step is
one-shell star/tail calibration, followed by smoke relighting. Increasing smoke
density or adding more lights alone would miss the largest visible differences.

## Reference and method

User-provided local file: `Feu d'artifice de la Fête Nationale 🎆🇫🇷.mp4`.
SHA-256: `03c61d7af8d5a6a9ada06457184c8095cafa0dde306ddf01e8eeefdbfd5db3dd`.
OpenCV reports 1920×1080, 4,385 frames and 24.945 FPS (approximately 2:56).
Timestamps below are decoded-video review anchors, not exact physical timings.
Reviewed 30 overview samples and four half-second sequences: 00:03.5–00:09,
00:22–00:27.5, 02:10–02:15.5 and 02:39–02:44.5. This is sampled visual analysis;
audio was not assessed.

Compared against a fresh public `aestra-bevy` fireworks example run using
`--smoke-art-show --effect effects/fireworks_show_wind_haze_smoke.aestra.ron
--experimental-oit --tier high --no-audio`, close camera, visible star smoke and
representative flashes, selected-star lighting off. The reused C4U executable
SHA-256 is `8ab33374275ff20aba199be42db54290e3eb69d6c2d19f8ff40830a1960618d5`.
The 1280×720 HDR/bloom run uses PlaybackOnly; 22 captures cover launch, early
expansion, fade and overlap. This avoids judging the show from an isolated
pre-break screenshot. The 37-second show completed; all 22 captures delivered,
13 flashes were accepted, sampled global drops/invalid links were zero and final
asynchronous alive count was zero. One pending-pipeline update remains recorded.
This instrumented image run does not establish a frame-time budget.

Local analysis artifacts are under ignored
`target/fireworks-reference-video-2026-10-27/`: `metadata.json`, `sequences.json`,
`overview-{1,2}.png`, four `sequence-*.png` sheets, `current-{early,overlap}.png`
and `current/current-close.json` plus full-resolution frames. The source video
and extracted reference images are not copied into version control.

## What the reference changes in the art target

| Reference anchor | Visible behavior | Current candidate comparison |
| --- | --- | --- |
| 00:03.5–00:08 | Numerous small white star heads remain legible as a sphere expands and fades. Thin paths sit beneath the heads. | At example 1.5–2.8s, expansion works, but the pink shell reads more as a sparse set of narrow, sometimes segmented arcs. Bright-head density, head/tail balance and luminous duration need calibration. Camera scale and shell family differ, so this is not a particle-count measurement. |
| 00:22–00:27.5 | Gold branches shed fine sparks, curve downward and cool through a visibly varied tail. | Example 6.5–7s and 19s have recognizable secondary breaks. Small repeated branch clusters are still conspicuous; tail continuity, variation and cooling need a dedicated gold-shell comparison. This is a different target from point-star shells. |
| 02:11.5–02:14 | Fresh white/red bursts reveal fine smoky paths and recolor nearby haze; quiet intervals leave much of the sky dark. | Deposited paths and persistent smoke exist. At example 12s and 20–22s the older field still reads as several organized, mottled umbrellas. It should lose that structure and remain subordinate to the luminous stars. Selected-star lighting is disabled in this control, so this run cannot evaluate its contribution. |
| 02:39–02:44.5 | Different sizes, heights, colors and burn phases overlap. Stars, tails and locally lit smoke remain distinguishable. | The saved 13-clip show exercises overlap, but its family repetition and visual density do not establish a comparable finale. Treat expanded show composition as a later host/art task and measure the actual resulting load. |

The new smoke is substantially closer to a deposited wake than the earlier
isolated round puffs. It still needs breakup and illumination review. The video
does not prove that a fluid solver is necessary, nor that billboard authoring
alone will suffice at every distance. Isolate the remaining failure before
choosing that implementation.

Do not turn every camera halo or long bright streak into particle geometry.
Exposure, lens flare, bloom, compression and resolution affect this recording.
It supports qualitative lifecycle/silhouette comparisons, not absolute luminance,
world scale, exact star counts or physically calibrated opacity. Host scenery,
water reflections, choreography and sound remain separate from Aestra runtime
feature readiness.

## Next implementation and acceptance

1. Author one opt-in point-star shell in the public Bevy example. Use the existing
   sphere velocity distribution, lifetime/drag/gravity, size/color/opacity curves,
   sprite and trail controls first. The current multi-break Main-star lifetime is
   1.2–1.6 seconds; do not mistake its short authored burn for a runtime limit.
   Balance small bright heads against thinner/fainter cooling tails and vary the
   burn without turning every star into a large glow disk. Preserve current
   assets/defaults as controls.
2. Review launch, flash, early expansion, peak spread, cooling and extinction as
   a sequence from audience and close cameras, with fixed exposure/bloom and a
   documented projected shell size. Compare the point-star sequence to
   00:03.5–00:08; separately tune a gold-tail family against 00:22–00:27.5.
   Require an acceptable moving sequence, not just an attractive single image.
3. With the luminous shell established, tune its deposited smoke in isolation:
   density-hidden, representative-light muted and normal controls. Check that
   smoke stays behind the moving stars, gradually loses radial organization,
   drifts/disperses and does not dominate the quiet sky.
4. Then author bounded selected-star light outputs and independent host
   enablement. Compare representative-only, selected-only and combined lighting
   on older smoke against 02:11.5–02:14, keeping simulation/density and light
   budgets fixed. Add a runtime/material capability only for a demonstrated gap.
5. Expand the accepted families into the host show, then rerun all-tier capacity,
   cue accounting, cleanup and separate ordinary performance checks. The video
   finale is an art reference, not evidence that its apparent load fits current
   budgets. Default adoption, production-finale performance and full F8 approval
   remain open.

No runtime, saved asset, capacity limit or default changed for this review.
