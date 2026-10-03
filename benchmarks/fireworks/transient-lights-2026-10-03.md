# F7A — representative transient scene lights, 2026-10-03

Base revision `67d68b43` plus F7A working changes. API/render acceptance on Windows,
NVIDIA GeForce RTX 4070 SUPER, Vulkan, native GPU presentation at 960×540. Not a
real-time lighting-cost benchmark, finale certification or AAA artistic approval.

## Contract and scope

`aestra-runtime` exposes host-neutral `PointLightPulse` and `TransientPointLight`.
Linear RGB is normalized 0..1; brightness is lumens, distance uses host world units.
Intensity/range curves use normalized pulse age and reject empty, unordered, nonfinite,
invalid or over-32-key data. The light occurrence time is independent of delivery time
and of particle/trail lifetime. No new authored schema/compiler path is claimed yet.

The opt-in Bevy adapter accepts root/epoch-qualified intents with a stable duplicate key.
Its world-space, unparented, shadowless point-light proxies inherit root render layers,
reuse entities and have bounded active/allocation/request counts. Global enable,
max-lights, requests/frame, lumen and range caps are host-controlled. Portable ceilings
are 64 lights and 1,024 consumed intents/frame; producer-side message allocation remains
the host's responsibility. Saturation drops new intents with telemetry rather than
evicting a live pulse or queuing an unbounded backlog. Admission follows delivery order;
cross-instance asynchronous GPU arrival does not promise which tied pulse wins a full pool.

Root time drives fade: pause holds brightness; late delivery does not restart peak;
expiry, disabled settings, despawn or incompatible seek/restart epochs clear pulses.
Invalid/stale/future/expired inputs are rejected, not scheduled for later. Missing
proxy components are cleaned up, and external deletion cannot strand a saturated slot.
There is no light-history reconstruction, attached moving light, shadow map or EachEvent
particle-identity contract in this slice.

The viewer's `--transient-lights` flag installs example-host bindings for `main_break`
only: one pulse per packet, regardless of particle count. Four shared sources/13 clip
occurrences use their exposed, clip-overridden star gradient at age 0.5. The pulse uses
500,000 peak lumens, 80-unit range, 0.1 radius and 0.6-second fast fade. The saved show
remains unchanged; no host timer launches the light or particles.

## Native admission and cleanup

The existing F6B checker executes full playback, seek to 18 seconds/resume, then
restart/full playback. Same seed `0xf1e0000000000001`, fixed 60-Hz effect clock,
PlaybackOnly, nonidentity root placement `(12,3,-7)`, Y rotation 0.35, scale `(0.9,1,0.8)`.
Cue checks use the existing default camera response (no `--hdr` flag); this is not a
photographic comparison. Actual root epochs remain 1/4/5. All ordinary spatial cue
assertions still pass, and final light admission/cleanup is checked independently.

| Tier | Light budget | Burst intents | Admitted | Budget drops | Peak / allocated | Final active | Viewer frames |
| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| High | 8 | 27 | 27 | 0 | 3 / 3 | 0 | 3,859 |
| Medium | 4 | 27 | 27 | 0 | 3 / 3 | 0 | 3,859 |
| Low | 2 | 27 | 23 | 4 | 2 / 2 | 0 | 3,862 |

All reports passed with zero invalid/stale/duplicate/expired/disabled intents. Low's
four drops are deliberate light-budget saturation, not missing simulation cues or
particles. Raw local reports: `target/fireworks-f7/f7a-{high,medium,low}.json`.

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --backend gpu --history playback-only --tier high --transient-lights --fireworks-cue-check target/fireworks-f7/f7a-high-review.json
```

Repeat with medium/low and fresh report paths. The checker rejects non-native/replay
modes, capture/benchmark combinations and an already existing report path.

## Actual diffuse illumination, not bloom

Added two neutral rough diffuse receiver cards at shell height to the **viewer host
validation scene**, identically present with lights off/on. Other shell probes and the
saved show are unchanged. Compare default high tier, identity root, audience camera,
HDR, exposure 0, Tony tonemapping, **bloom 0** and no dither at exact frames
75/85/90/100/120. The first main break occurs at source tick 80, world height ≈44.14.

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --backend gpu --history playback-only --camera audience --hdr --bloom 0 --sample-frames 75,85,90,100,120 --capture target/fireworks-f7/receivers-off-final
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --backend gpu --history playback-only --camera audience --hdr --bloom 0 --transient-lights --sample-frames 75,85,90,100,120 --capture target/fireworks-f7/receivers-on-final
cargo test -p aestra-viewer native_receivers_respond_without_bloom_and_return_to_baseline -- --ignored --nocapture
```

The ignored native oracle reads those captures and their recorded settings. It requires
exactly matching before-birth/after-expiry frames, a positive red-channel response on
the left receiver ROI (x 330..365, y 215..240), and monotonic fade across active frames.
This crop is outside the early star footprint at frame 85. Pixel values below are
8-bit display response, not radiometric measurements or threshold targets for all GPUs.

| Frame | Changed image pixels | Maximum absolute channel change | Receiver mean red change |
| --- | ---: | ---: | ---: |
| 75 | 0 | 0 | 0.00 |
| 85 | 3,432 | 20 | +5.55 |
| 90 | 2,678 | 13 | +3.49 |
| 100 | 2,223 | 8 | +2.08 |
| 120 | 0 | 0 | 0.00 |

Contact sheets were visually inspected: the receiver near the first burst gets a
subtle pink tint and fades. The original dark ground alone had only a 1-value response,
so it was insufficient as a clear scene-lighting oracle. Do not inflate this controlled
receiver proof into a claim that the whole show already has its intended lighting look.
Shutdown-only GPU readback closed-channel warnings occurred after successful captures;
all image/report files and the pixel oracle completed.

## Tests and remaining gates

- Runtime library: 72 passed (two new pulse/validation tests).
- Bevy library: 46 passed (four new pool/admission/lifecycle tests).
- Viewer: 82 passed / 8 ignored (seven fixture exporters, one native receiver oracle).
  The receiver oracle was also explicitly run and passed using the captures above.
- Workspace check, scoped all-target Clippy with warnings denied, formatting and
  diff whitespace checks passed. Existing Bevy crate doctest also passed.
- Pool tests cover pause, delayed age, dedup, root isolation, seek/restart, disable,
  root/proxy despawn, partial proxy removal, cap shrink, range/lumen clamps, frame
  overflow discard and hard adapter ceilings. Host tests cover color overrides,
  one representative pulse for large packet magnitude, legacy exclusion and CLI opt-in.

**Next F7B:** persist generic authored representative-light bindings through
compiler/runtime/editor, while retaining host budget/disable authority. Current Aestra
particle smoke remains unlit, and volume integration has its own lighting model;
illuminated particle/volume smoke and measured production lighting budgets are separate
F7/F8 gates. Audio assets/mixing, sky and host scene integration remain host-owned.
