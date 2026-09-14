# Client Runtime Unification — Dogfooding `EffectPlayer` in the Editor

Status: design (no code yet). Grew out of a crate-layout question ("keep `aestra-bevy-render`
or fold it into `aestra-bevy`?") and the two goals behind it:

1. **Dogfood the client API** — the editor should host effects through the same public path a
   game does, so the flagship app exercises `EffectPlayer` rather than a private lower layer.
2. **One reference implementation** — an author writing `aestra-godot` or `aestra-unity` should
   have a single, clear place to read, not two competing "how to host an effect" stories.

## The finding (feasibility pass)

The crates are **not** merge candidates — the layering is sound:

```
aestra-editor ──────────────► aestra-bevy-render   (render backend: GPU pipelines, materials)
aestra-viewer ──► aestra-bevy ──► aestra-bevy-render
```

- `crates/aestra-bevy-render` (~9.4k LoC): the rendering backend. `AestraRenderPlugin`,
  `PresentedEffect`, GPU compute/statistics. Used by **both** the editor and the client plugin.
- `bevy/aestra-bevy` (~1.9k LoC): the batteries-included client plugin. `AestraPlugin`,
  `EffectPlayer`, project reconciliation, choreography events, profiling. Used by `aestra-viewer`.

Merging would force the editor to swallow the whole playback runtime it doesn't use. **Keep them
separate.** (Optional tidy: the two Bevy-facing crates live in different directories —
`crates/aestra-bevy-render` vs `bevy/aestra-bevy`. Co-locating them is a low-risk directory move,
orthogonal to this plan.)

The *real* issue the goals expose is that the editor re-implements playback instead of consuming
the client. Both `aestra-bevy` and the editor build **two parallel drivers over the same
`aestra_runtime` primitives** (`PlaybackClock` + `EffectInstance` + `SimulationSeekMode`/`SeekPlan`):

| Concern | `aestra-bevy` (client) | editor (bespoke) |
|---|---|---|
| Single-effect driver | `EffectPlayer` (`clock`, `instance`) | `EditorSession` (`clock`, `preview`) — [`session.rs:64`](../../apps/aestra-editor/src/session.rs) |
| advance / seek / step / seek-modes | `EffectPlayer::advance/seek/seek_frame/step_*` | `EditorSession::advance_playback/seek_time/step_frame` |
| player → render bridge | `sync_player_presentations` | `configured_preview_instance` — [`viewport.rs:2119`](../../apps/aestra-editor/src/viewport.rs) |
| **project clip reconciliation** | `project::sync_project_instances` | `sync_rendered_preview` — [`viewport.rs:2703`](../../apps/aestra-editor/src/viewport.rs) |

So both the single-effect player *and* the multi-clip project reconciliation exist twice, and the
client's copies are exercised only by `aestra-viewer` and unit tests.

### What `EffectPlayer` already covers

`seek`, `seek_simulation_time`, `set_playback_time` (documented as *"driven by an external clock"* —
exactly the editor's timeline case), `seek_frame`, `step_forward/back`, `restart`, `set_parameter`,
`set_seed`, `checkpoint/restore_checkpoint` (single slot), choreography drain, and
`from_project` + `sync_project_instances`. The editor's 3D preview is **single-view** (one
`PreviewRenderCamera`, `Single<…>`), so the `EffectPlayer`↔`PresentedEffect` 1:1 model fits the
single-effect case cleanly.

### Gaps that block a naïve "just call `EffectPlayer`" migration

These are load-bearing — each is real work or a decision, and each is a place the public API grows
to meet a demanding host (the payoff of dogfooding):

- **G1 — Live-editable project vs. compiled artifact.** `EffectPlayer::from_project` consumes an
  immutable `Arc<CompiledEffectProject>`; `sync_rendered_preview` drives from the *mutable*
  `EditorPreviewProject` + `TimelineState`. Adopting the plugin path needs the live timeline
  compiled to a `CompiledEffectProject` at edit granularity (a possible per-frame cost the current
  design avoids).
- **G2 — Authoring fast-paths.** `compiled_effects_differ_only_by_emitter_transforms` lets dragging
  an emitter mutate transforms **without restarting the sim** ([`viewport.rs:2737`](../../apps/aestra-editor/src/viewport.rs)),
  plus history-epoch discontinuity detection. `sync_project_instances` resets more bluntly.
- **G3 — Checkpoint store vs. single checkpoint.** `EditorSession` holds
  `CheckpointStore<EffectInstance>` for fast backward scrubbing; `EffectPlayer` exposes one slot.
- **G4 — No "swap compiled effect, keep position".** Live recompile replaces the `CompiledEffect`
  under the player while preserving the frame; `EffectPlayer` has no `replace_effect(…, preserve)`.
- **G5 — `AestraPlugin` bundles auto-advance** (`play_effects`); the editor is scrub/pause-dominated
  and drives its own clock. Needs `EffectPlayer` used as a state holder via `set_playback_time`
  with auto-advance disabled (the API is designed for this, but it's a coordination point).

`EditorSession` is also the *document* (undo, materials, dirty, save). Only its ~200-line playback
slice overlaps `EffectPlayer` — this plan touches that slice, not the document.

## Milestones

Sequenced low-risk → high-risk. The viewport is the riskiest code in the app, so every step stays
green and reviewable, and nothing lands that regresses authoring feel.

### M-CR1 — a canonical reference client — **done**
*Goal 2, cheap, no editor risk.* Rather than shrink `aestra-viewer` (a 1.4k-line capture/regression
harness — a poor first read), the reference lives with the crate that binding authors depend on:
- `bevy/aestra-bevy/examples/minimal_player.rs` — the full load-and-play path in ~40 lines
  (`cargo run -p aestra-bevy --example minimal_player`).
- Crate-level rustdoc on `aestra-bevy` documenting the four-step client pipeline, with a `no_run`
  minimal-host snippet and an explicit note that this is the one place for `aestra-godot` /
  `aestra-unity` authors.
- `bevy/aestra-bevy/README.md` — a "where to look" table pointing at the example, the docs, and
  `aestra-viewer` as the advanced real-world host.

`aestra-viewer` stays as the demanding real-world host (capture, diagnostics, GPU bench), referenced
from the docs — not gutted. **Exit met:** an integrator can build a host by reading the example and
crate docs alone.

### M-CR2 — Unify the single-effect driver
*Foundational dedup, moderate risk.* Make the editor's single preview delegate to `EffectPlayer`
(as an owned field of the preview runtime, not necessarily a Bevy component yet), driven via
`set_playback_time`/`seek_frame` from the editor's timeline. Delete `EditorSession`'s bespoke
`advance_playback`/`seek_time`/`step_frame` clock math in favor of the player's. **Exit:** editing +
scrubbing a single effect goes through `EffectPlayer`; `aestra-viewer` and editor share one driver;
all editor playback tests green.

**Progress:**
- **G4 — done.** `EffectPlayer::replace_effect(Arc<CompiledEffect>, preserve_position)` added +
  tested (keeps seed; replays to the current frame or restarts). Live recompile can now swap the
  compiled effect under the player.
- **G5 — confirmed.** `EffectPlayer::set_playback_time` already exists ("synchronize sequential
  playback driven by an external clock") — the path the editor timeline drives. No code needed.
- **Remaining (the risky part).** Replacing `EditorSession.clock` (`PlaybackClock`) + `.preview`
  (`EffectInstance`) with an owned `EffectPlayer` touches ~48 `.clock` refs across 9 files plus the
  `.preview` reads, and the presentation path (`session.preview` → `PresentedEffect`). This is the
  fragile editor core; it needs build-and-scrub verification between steps, so it should land as its
  own carefully-staged effort (introduce the owned player → migrate one method at a time → remove the
  old fields), not a single blind rewrite.

### M-CR3 — Preserve scrub performance
*Addresses G3.* Extend `EffectPlayer` with an optional checkpoint cache (behind the same
`CheckpointStore` type the editor uses) so backward scrubbing keeps its speed, and move the editor
onto it. **Exit:** backward-scrub latency ≥ today's; the cache is part of the public player.

### M-CR4 — Unify project clip reconciliation
*Highest risk, addresses G1 + G2.* Replace `sync_rendered_preview` with `EffectPlayer::from_project`
+ `sync_project_instances`, after: (a) deciding the live-timeline → `CompiledEffectProject`
compilation cadence (G1), and (b) porting the emitter-transform fast-path and history-epoch handling
(G2) into the shared reconciler as opt-in behavior. **Exit:** the editor's multi-clip preview and
`aestra-viewer`'s project playback share one reconciler with no authoring-feel regression.

## Decision checkpoint

M-CR1 alone satisfies goal 2 and is worth doing regardless. M-CR2 is the smallest step that delivers
real dogfooding (goal 1) for the common single-effect case. M-CR3–4 are only worth it if the
duplication cost or a concrete API bug bites — reassess after M-CR2 with real diff/latency data
rather than committing the full arc up front.
