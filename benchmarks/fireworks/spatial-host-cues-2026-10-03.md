# F6B — nested spatial particle host cues, 2026-10-03

Base revision `def469eb` plus F6B working changes. Bounded native host-API acceptance,
not AAA art, real-time performance, audio playback or finale certification.

## Contract and fixes

The existing `AestraOutputEvent` stream now qualifies native particle routes with the
root player entity, stable clip path and **root** history epoch. Raw child GPU epochs are
validated first; a host never has to retain/find temporary child entities to bind a cue.
`particle: Some(ParticleOutputContext)` carries source effect ID, inherited seed,
root-clock occurrence time and optional world position. The ordinary `event.origin`,
local XYZ `event.value`, source-local simulation `event.tick` and particle-count
`event.magnitude` keep their semantics. Legacy stage/homing/finished outputs have no new
particle metadata and retain their previous routing semantics.

World position = ECS placement at delivery × historical authored ancestor/clip/leaf pose
at the source tick × local event position. Full affine composition retains shear. Missing
placement or nonfinite data is explicitly unavailable. **Arbitrarily moving ECS root
placement is not recorded history**, and velocity is not supplied by this packet. The
two-level historical-motion/nonuniform-scale unit oracle is independent of the production
transform composition; the native show checks static clips/root placement.

Surviving child presentations now suppress reconstruction through their **new** source
time on seek, not their previous time. Newly created children suppress source/ancestor
preroll and events before the root seek boundary. Live advancement preserves compatible
epochs/boundaries. Existing bounded route high-water marks reject stale/duplicate GPU
records; paused reconstruction does not play old cues.

The first native attempt correctly rejected a legacy child `finished` notification as
outside the selected-route contract. The checker now explicitly excludes these completion
notifications without filtering out incorrectly addressed particle cues. Initial checks
also revealed that strobe exported no launch/burst routes: ordinary OnSpawn/OnDeath
FirstPerTick routes were added to its saved source and reviewed fixture builder. The final
gate requires those two routes on **every** shell source, rather than certifying 11/13 clips.
Strobe material flashes are not independent sound events.

## Native acceptance

Same local Windows/RTX 4070 SUPER/Vulkan development setup as F6A, 960×540 audience camera,
fixed 60 Hz, seed `0xf1e0000000000001`, HDR/exposure 0, PlaybackOnly, default fast
transparency. Catch-up pacing is disabled only for this explicit acceptance probe.
Root placement is translation `(12,3,-7)`, Y rotation `0.35`, scale `(0.9,1,0.8)`.
The existing show supplies 13 independent clip seeds, translated/rotated/scaled placements
and four shared compiled sources; no host timer launches particles or sounds.

Each tier executes (0) a full show, (1) a seek to 18 seconds while paused, followed by
remaining live playback, and (2) restart/full playback. Reconstruction is allowed to settle
before unpausing, and asynchronous readbacks drain before switching epochs. Completed
child presentations must be absent after the quiet tail. The consumer checks native GPU
observations, root ownership, path/source/seed/emitter/time/world position, exact per-clip
counts and duplicate identities. Restart/resume comparisons sort by clip/kind/source tick,
not cross-instance readback arrival order.

| Tier | Full messages (each original/restart) | Full launch/main counts | Secondary transitions | Resumed messages | Resumed transitions | Total messages / frames |
| --- | ---: | --- | ---: | ---: | ---: | --- |
| High | 236 | 13 / 13 | 704 | 60 | 193 | 532 / 3860 |
| Medium | 207 | 13 / 13 | 352 | 48 | 97 | 462 / 3861 |
| Low | 146 | 13 / 13 | 176 | 33 | 49 | 325 / 3858 |

All three reports passed with root epochs **1,4,5**. Full passes include all 13 paths;
resumed activity occurs in three paths. Original/restarted secondary totals are
multi-break **320/160/80**, crackle **288/144/72**, crossette **96/48/24**. The remaining
main break plus secondary demand totals 193/97/49 after the seek. Local/world coordinates
match within 0.001 units. No selected cues are missing or duplicated; no stale host messages
were observed in these native runs (synthetic/unit checks exercise their rejection).
Message counts are coalesced packets, **not one sound voice per particle**.

Raw ignored reports: `target/fireworks-f6/f6b-spatial-{high,medium,low}-complete.json`.
Earlier exploratory reports remain local and are not the final acceptance evidence.

## Reproduction and tests

Use a fresh output filename for each tier; failures exit nonzero and write diagnostic JSON.

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f6-show --camera audience --backend gpu --history playback-only --tier high --hdr --exposure 0 --fireworks-cue-check target/fireworks-f6/f6b-spatial-high-review.json
```

Repeat with medium/low and distinct paths. The mode rejects CPU/replay/capture/benchmark
combinations and existing report paths. No audio assets/playback are involved: exported
kinds map to host sound identifiers (`shell_launch`, `shell_burst`, `secondary_burst`,
`crackle`, `crossette_split`) using actual simulation observations.

Validation: viewer **80 passed / 7 ignored fixture exporters**, Bevy integration **42**,
runtime **70**, renderer particle delivery **2** and spatial context **2**; targeted final
negative/CLI checks pass. All-target Clippy for these four crates with warnings denied,
workspace check and formatting pass. Historical transforms, invalid/missing data, stale
epochs, duplicates, seek boundaries and ancestor preroll have automated coverage.

## Limits / next

FirstPerTick is a visual/audio observation, not lossless gameplay authority. The existing
32-tick GPU ring can lose old observations under long readback stalls; EachEvent has bounded
position/count semantics, not a stable identity for every particle. Hosts own voices,
delays, budgets, audio assets/mixing, environment and choreography integration. Native
coverage here is a one-level reusable show; multi-level authored clock/affine composition
is unit-tested, not an additional full GPU multi-level show acceptance.

F6 editor authoring/artistic review, lit/persistent smoke and production finale budgets
remain open. Next engine milestone: **F7 generic transient-light outputs and bounded Bevy
light pooling**; this cue gate does not certify that fireworks illuminate the scene.
