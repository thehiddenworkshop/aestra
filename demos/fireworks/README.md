# Fireworks demonstration

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
See [the full guide](../../docs/examples/fireworks.md) for content packaging, audio,
visual candidates and remaining F8 qualification. For a short learning example use
`cargo run -p aestra-bevy --example host_projectile`; `fireworks_event_chain` teaches
particle lifecycle links separately.
