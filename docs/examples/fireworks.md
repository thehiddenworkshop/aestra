# Bevy fireworks showcase

`bevy/aestra-bevy/examples/fireworks.rs` is the public integration host. It loads the
same saved effects and materials as the editor, not an implementation imported from
`aestra-viewer`. The viewer remains the capture, regression and benchmark harness.

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks
cargo run --release --locked -p aestra-bevy --example fireworks -- --tier medium
cargo run --release --locked -p aestra-bevy --example fireworks -- --history replay-enabled
```

The default is native GPU presentation, high quality, fixed HDR exposure and bloom,
and **playback-only** history. The 26-second, play-once show uses 13 clips referencing
four reusable shell effects: multi-break, crackle, crossette and strobe. Restart with R.
High/medium/low select authored particle/event/trail budgets at compilation and separate
host representative-light ceilings of 8/4/2. These are policies, not hardware-independent
performance promises. Native additive sprites use a two-pixel minimum footprint and
trails a one-pixel minimum width, with the runtime's energy attenuation; this is
presentation sampling, not changed particle counts or trail history. The HUD reports the active backend and smoothed wall-frame duration
(including vsync), not isolated GPU timings. Use viewer/bench tooling for measured costs.

Controls: Space pauses/resumes; R restarts; 1/2/3 choose close/audience/wide cameras;
L toggles authored burst lights; M mutes sound; Esc exits. Shader preparation holds
the initial player at time zero; preparation errors/timeouts exit instead of silently
consuming the beginning of the show. Representative flash lights use the saved
`point_lights` bindings and the public `AestraTransientLightPlugin`, with shadows off.
The environment is deliberately minimal: dark background, ground and two dark
PBR receiver structures (non-emissive, lit by moon/burst lights).

## Editor to host

1. In the editor, use File → Open Project and select `assets/test`.
2. Open `effects/fireworks_show.aestra.ron` to edit clips, placement, timing and overrides.
3. Open its four shared shell effects or referenced material programs to edit their appearance.
4. Save, then restart the example. It reads saved source files at launch, not stale embedded text.

For a separate project, pass its **asset root**, not necessarily its repository root:

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --project C:/MyShow/assets --effect effects/my_show.aestra.ron
```

`fireworks/project.rs` shows load → resolve → compile → `EffectPlayer::from_project`.
`--effect` is relative to that asset root, or absolute. Dependencies/materials resolve
through `ProjectAssetIndex`. Shipping hosts can compile offline and load an artifact.
No per-particle CPU polling, effect duplication or direct render-backend ownership is required.
The original procedural three-emitter teaching demo is retained as `fireworks_event_chain`.

## Optional host sound

Audio is outside the portable VFX runtime. Enable the example's WAV decoder explicitly:

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks --features fireworks-audio
cargo run --release --locked -p aestra-bevy --example fireworks --features fireworks-audio -- --audio-root C:/MySounds/fireworks
```

The default local folder is `bevy/aestra-bevy/assets/audio/fireworks`. Expected files:
`Firework_Large_01.wav` through `05`, likewise `Medium` and `Small`. Missing groups are
reported and remain silent; `--no-audio` bypasses loading. The local pack's existing
`assets/CREDITS` names Joel Summers. Those user-provided files are not added or modified
by this implementation. Confirm redistribution rights before shipping/bundling a pack;
the repository's source-code license does not grant rights to third-party recordings.

`fireworks/audio.rs` consumes actual `AestraOutputEvent` particle cues:

| Cue | One-shot group |
| --- | --- |
| `main_break` | Large |
| `secondary_break` | Medium |
| `crackle`, `crossette_split` | Small |
| `launch` | Unbound; this pack has no separate launch recording |

This is an illustrative binding, not a final sound mix. Spatial placement uses the
event's world position, with camera listener and approximate 343m/s propagation delay
corrected for GPU delivery latency. Variant selection uses source seed/tick. Epoch,
clip path, kind, emitter and tick qualify deduplication; event magnitude never becomes
one voice per star. Limits are 256 packets/update, 64 pending cues, 24 simultaneous
voice entities and 4096 seen identities/epoch. Saturation drops new requests. Incoming
cues more than one second late or over eight seconds from arrival are ignored. Pending
cues and active voices are cleared on restart/seek/root removal; pause freezes sound,
and the play-once ending allows audible tails. Unloaded/failed files are not played late;
voice entities time out after ten unpaused seconds even without an audio device. Bevy
spatial audio is simple panning, not HRTF/occlusion/acoustic simulation.

## Verification and remaining work

```powershell
cargo test --locked -p aestra-bevy --example fireworks --features fireworks-audio
cargo clippy --locked -p aestra-bevy --example fireworks --example fireworks_event_chain --features fireworks-audio -- -D warnings
```

GPU-free tests resolve all three tiers, shared materials and automatic renderer bindings,
the history API, event routing/positions/delay, stale/duplicate rejection, bounded queues
and voice admission/mute/restart/root cleanup. The existing `AESTRA_EXAMPLE_CAPTURE`
environment hook captures a native window screenshot and exits after image delivery;
`AESTRA_EXAMPLE_CAPTURE_SECONDS` chooses simulation time (default 5, clamped to show end).

Verified on 2026-10-05 (Windows, RTX 4070 SUPER, Vulkan, development build): eight
audio-enabled GPU-free tests, four without the feature, and strict scoped Clippy passed.
Native high/playback-only and low/replay-enabled three-second captures render the saved
show. Two full high/playback-only 26-second audio runs admitted all 13 authored flashes
(peak three, zero invalid/stale bindings) and 134 sound voices, dropping 89 requests;
the final run observed 24 actual Bevy spatial sinks at peak and no queued sounds at end.
This verifies decoding/binding/cleanup, not subjective sound quality. Raw images live in
`target/fireworks-bevy-showcase/`. Short-run shutdown can log Bevy readback-channel-closed
warnings; captures completed and processes exited successfully. An initial warm-up run
incorrectly treated a retryable unloaded shader as fatal; the corrected retry policy has
a regression test and all subsequent native runs passed.

This is the first showcase-host integration, **not final AAA acceptance**. Shared shells
currently use particle smoke and representative burst lights. Continuous selected-star
lighting is not authored/enabled here; F7's benchmark-fixture lights are not silently
injected into saved assets. Lit particle smoke, generic fluid injection, F8.3B matched
costs/real-output smoke qualification, natural-star art, final environment/audio mix and
production-finale density remain separate work. Keep these gates in the fireworks roadmap.
