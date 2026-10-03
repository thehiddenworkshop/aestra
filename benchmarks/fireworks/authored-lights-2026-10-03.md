# F7B1 — authored representative scene lights, 2026-10-03

Base revision `65642f31` plus F7B1 working changes. Contract/native-render acceptance
on Windows, NVIDIA GeForce RTX 4070 SUPER, Vulkan, 960×540. This is not a lighting-cost
benchmark, finale-scale certification or AAA artistic approval.

## Implemented boundary

`EffectAsset::point_lights` persists a stable binding ID, particle route ID, pulse
envelope/range/radius/lifetime and optional gradient parameter/sample age. The source
v4 field defaults empty; conversions preserve it. Core validation includes stable
binding/curve IDs, one binding per FirstPerTick route, unambiguous output/emitter pairs,
finite normalized RGB, positive duration/range and ordered curves of at most 32 keys.
EachEvent light bindings are intentionally rejected: current cue packets do not carry
route/trigger IDs or individual particle identity.

Compiler parameter discovery includes colors used only by lights. Exposed colors
use runtime slots; non-exposed defaults lower to literal pulses. Artifact **v6** persists
and validates compiled route/slot references, aggregation, pulse validity and duplicate
binding identities/routes. Old versions must be recompiled, not silently stripped of lights.

The opt-in `AestraTransientLightPlugin` resolves saved bindings automatically. Root
live parameters are delivery-time values, not historical snapshots. Nested source
resolution walks stable clip IDs and uses the final clip's overrides/defaults, even
without a surviving child presentation. Hosts retain global enable, pool/request/lumen/
range budgets and can disable automatic bindings with `authored_bindings: false` for
custom mappings. Input inspection is capped at the requests/frame budget (ceiling 1,024);
overflow is discarded with telemetry. `source_packets_dropped` includes unbound cues,
not necessarily lost light intents. Invalid source/color/position bindings fail closed.

Five reusable shell/volley assets save one 500,000-lumen, 80-unit, 0.1-radius,
0.6-second pulse per representative route, sampling their exposed star gradient at
age 0.5. The viewer no longer maps firework route names/colors/envelopes at runtime;
it only installs the plugin and high/medium/low host budgets. Magnitude does not scale
the light. Neutral diffuse receiver cards remain host validation geometry.

## Native playback, seek and restart

Same F6B show check: 13 clips, full playback, seek to 18 seconds/resume, restart/full
playback. Fixed seed `0xf1e0000000000001`, 60-Hz effect clock, PlaybackOnly,
nonidentity root position `(12,3,-7)`, Y rotation 0.35, scale `(0.9,1,0.8)`.

| Tier | Budget | Burst packets | Admitted | Budget drops | Peak / allocated | Final active |
| --- | ---: | ---: | ---: | ---: | --- | ---: |
| High | 8 | 27 | 27 | 0 | 3 / 3 | 0 |
| Medium | 4 | 27 | 27 | 0 | 3 / 3 | 0 |
| Low | 2 | 27 | 23 | 4 | 2 / 2 | 0 |

All checks pass, with zero binding-invalid/source-scan drops and zero invalid, stale,
duplicate, expired or disabled intents. Low's four admission drops are intentional
pool saturation. Root epochs are 1/4/5. These checks are not lossless-gameplay delivery
proof: the existing FirstPerTick coalescing and 32-tick GPU ring still apply.

Final raw local reports: `target/fireworks-f7/f7b1-high-final.json`,
`f7b1-medium-final.json`, `f7b1-low.json`.

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --backend gpu --history playback-only --tier high --transient-lights --fireworks-cue-check target/fireworks-f7/f7b1-high-review.json
```

Repeat for medium/low with fresh paths. Earlier high/medium checks preceded the final
semantic-ID validation pass; final runs above include it. That pass caught an authored
strobe material/light-curve ID collision, now fixed with independent curve IDs.

## Diffuse response without bloom

Fresh paired captures at frames 75/85/90/100/120, high tier, audience camera, identity
root, HDR, exposure 0, Tony tonemapping, **bloom 0**, no dither. The existing native
pixel oracle passes against `target/fireworks-f7/f7b1/receivers-{off,on}-final`:

| Frame | Changed pixels | Max absolute RGB delta | Left receiver mean red delta |
| --- | ---: | ---: | ---: |
| 75 | 0 | 0 | 0.000 |
| 85 | 3,432 | 20 | +5.546 |
| 90 | 2,678 | 13 | +3.491 |
| 100 | 2,223 | 8 | +2.075 |
| 120 | 0 | 0 | 0.000 |

ROI: x 330..365, y 215..240, outside the early star footprint at frame 85. Display
values are not radiometric measurements. The receiver response matches F7A's custom
host mapper, now driven by the saved authored bindings. Before birth and after expiry,
the paired frames match exactly. The lights-on image was visually inspected; this
controlled receiver setup is not the final show look. Shutdown-only readback channel
warnings followed successful capture completion.

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --backend gpu --history playback-only --camera audience --hdr --bloom 0 --sample-frames 75,85,90,100,120 --capture target/fireworks-f7/f7b1/receivers-off-final
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --backend gpu --history playback-only --camera audience --hdr --bloom 0 --transient-lights --sample-frames 75,85,90,100,120 --capture target/fireworks-f7/f7b1/receivers-on-final
$env:AESTRA_LIGHT_CAPTURE_ROOT = (Resolve-Path 'target/fireworks-f7/f7b1').Path
cargo test -p aestra-viewer native_receivers_respond_without_bloom_and_return_to_baseline -- --ignored --nocapture
```

For the oracle, set `AESTRA_LIGHT_CAPTURE_ROOT` to an absolute directory when running
from a different working directory. Captures are local generated evidence, not committed assets.

## Regression checks and next gate

- Core/runtime/compiler/artifact test suites pass; runtime library remains 72 tests.
- Five new cross-crate contracts cover source/artifact round-trip, live colors, literal
  colors, missing/ambiguous/EachEvent/duplicate routes, invalid pulses/IDs/types/slots,
  non-exposed parameter lowering, nested/repeated paths and malformed source/path rejection.
- Bevy library: 48 passed; six pool/binding tests include automatic generic mapping,
  optional mapping, bounded source inspection/discard and invalid live-color rejection,
  in addition to F7A's lifecycle/admission coverage. Bevy doctest passes.
- Viewer: 82 passed / 8 ignored. Fixture/builders and all three quality tiers agree.
  The ignored native pixel oracle was explicitly run and passed using fresh captures.
- Workspace check, scoped all-target Clippy with warnings denied, formatting and
  whitespace checks pass.

**Next F7B2:** editor controls for route selection, color/gradient source, envelope,
range and lifetime, with undo/redo and validation feedback. No editor light controls
are claimed in F7B1. Lit particle/volume smoke, production lighting budgets and F4's
artistic acceptance remain separate open gates. Installation is still opt-in; current
unlit particle smoke does not respond to these scene lights. Audio mixing/assets, sky
and the example's host scene remain host-owned.
