# F8.1C4U — keep live cues during coupled catch-up and first activation

Two coupled-playback classification bugs are fixed. A long forward backlog no
longer suppresses real particle outputs, and initializing trail history at tick
zero no longer silences a first-frame launch. Explicit reconstruction boundaries,
seek/rewind guards, duplicate rejection and restart behavior remain intact.
No particle assets, shader ABI, catch-up budgets or output/light limits change.

The final native gate accounts for **13 launches, 13 main breaks and 13 bound
representative intents in every run**. High and medium accept all 13 flashes;
low accepts 11 with two explicit rejections at its unchanged two-light ceiling.
The earlier failures remain recorded. See the
[raw evidence and hashes](forward-output-cues-2026-10-27.json).

## Proven failures and fixes

The C4T medium close run accepted 12 flashes without a reported realization
rejection and sampled 1.1 seconds of encoding lag. It did not trace source stages,
so its exact historical cause cannot be assigned retrospectively. Inspection
found a concrete mechanism consistent with that failure: `is_live_advance` treats
more than ten ticks from encoded time to requested time as replay. Pacing can
create exactly that gap during uninterrupted forward playback. Event links and
particles advance, but their host outputs are skipped.

A native regression drives the **production lockstep encoder** toward tick 90
with the unchanged four-tick submission budget, draining bounded rings after each
submission. Old code returns no outputs instead of parent death `(tick 4,count 1)`
and accepted child birth `(tick 4,count 12)`. The negative result is retained in
`target/fireworks-f8-output-backlog-before.log`. The fix uses non-backward advance
plus existing synchronization/history guards, rather than clock distance. Each
route still suppresses ticks through its explicit epoch/reconstruction boundary;
coupled homing arrival parameters also honor that boundary per tick.

The regression now exports both cues once, stays silent while paused and while
reconstructing through tick 90, exports a genuine subsequent burst at tick 92, and
permits fresh cues after restart. Reconstructed/restarted particle snapshots equal
the live fixture. This validates producer/delivery semantics, not visible pulse
latency at a 1.5-second backlog.

The first native batch after that fix exposes a **second bug**: low reports all
13 main breaks/intents but only 12 launches, missing clip `fd01c`. Its first trail
history observation arrives at a nonzero clock. Previously, unobserved history
was always a discontinuity, silencing that first launch. A separate regression
uses the real trail observer and renders tick 1 without an earlier paused tick-zero
frame. The interim implementation returns no record; final code returns
`(tick 1,epoch 5,count 1)`. Its failed control is retained in
`target/fireworks-f8-output-activation-before.log`.

Final classification recognizes initially unobserved history **only at joint
particle tick 0**. Existing trail clearing/restoration/recording still runs;
nonzero history, desynchronized stores and rewinds retain their guards. This is
not a relaxed reconstruction boundary or a new replay policy.

## Bounded source-to-binding observations

`TransientLightStatistics` now counts source packets offered to the authored scan,
non-particle packets, missing roots, epoch mismatches, unbound packets and valid
bound intents. Together with existing scan drops, invalid binding, stale/duplicate
intents and realization outcomes, these account for each stage without inferring
every missing light from the final accepted count. Binding-disabled packets are
outside that scan.

The report hook independently audits delivered spatial cues, retaining at most
256 aggregate route keys with counts/first+last tick and maximum root-clock
delivery age. Final shows use 37 keys. A test exercises the bound/omission counter
and repeated-key aggregation. No per-particle CPU mirror, new GPU readback,
blocking production wait or unbounded event history is added.

The checker verifies both conservation equations:

```text
source packets offered = scan drops + classified packets
bound intents = accepted + explicit rejection/stale/duplicate outcomes
```

Source launch/main-break counts are checked independently, so the interim missing
launch fails even though its burst-light accounting passes. Other output-family
counts are recorded descriptively, without claiming lossless capture of every
secondary/crackle event.

## Final native controls

Same Windows/RTX 4070 SUPER/Vulkan, Rust 1.98.1, Bevy 0.20.0/wgpu 30.0.1 and dev
application opt-level 1/dependencies opt-level 3 as C4T. Public 1280×720 HDR/Tony/
bloom 0.15/OffMSAA; experimental OIT average4/K32 and unchanged 128 MiB guard.
PlaybackOnly, full 37-second shows plus ten cleanup updates, audio compiled and
`--no-audio`. Windows run sequentially, with no agent-launched builds or other
native GPU probes during the capture windows.

| Final close-camera case | Launch/main-break cues | Bound intents | Accepted flashes | Budget rejects | Maximum main-break source age |
| --- | ---: | ---: | ---: | ---: | ---: |
| Medium repetition1 | 13 / 13 | 13 | 13 | 0 | 116.7 ms |
| Medium repetition2 | 13 / 13 | 13 | 13 | 0 | 100.0 ms |
| Medium repetition3 | 13 / 13 | 13 | 13 | 0 | 100.0 ms |
| High | 13 / 13 | 13 | 13 | 0 | 100.0 ms |
| Low | 13 / 13 | 13 | 11 | 2 | 100.0 ms |

Every final case has zero scan drops, missing-root/epoch/invalid binding, stale or
duplicate intent, expiry, disabled realization and audit-key omission counts.
Per-tier smoke births remain 37,996/18,959/9,313 across 13 owners, with zero observed
source overflow/expansion/destination drops, zero sampled global OIT loss/invalid
links and a full drain. All 40 final captures deliver. The five interim runs and
their 40 captures, including the rejected low launch, are separately retained.

All five final reports pass `check-fireworks-output-audit.py`. The same checker
deliberately returns exit1 on the interim batch. The historical C4T medium failure
is not rewritten or retrospectively marked passed. Final sampled encoding lag is
16.7–33.3 ms, but source age above is root-clock age at main-world observation,
**not display/GPU completion latency**. All image runs retain one global-cache
pending update; zero late pipeline work is not established. These scan/capture
runs are not ordinary cost measurements or stable 60 FPS acceptance.

## Checks, reproduction and limits

Final tests: **184 renderer passed, three existing ignored**, with
`AESTRA_REQUIRE_GPU_CONFORMANCE=1`; 65 host and 34 fireworks-example tests passed.
Strict workspace Clippy with GPU/audio features, workspace formatting/diff checks
and standalone formatting of the included GPU regression pass. Environmental
incremental hard-link fallback notices remain. No full workspace test suite is
claimed beyond these renderer/host/example suites.

```powershell
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test -p aestra-bevy-render --lib --locked --offline -- --test-threads=1
cargo test -p aestra-bevy --lib --example fireworks --features fireworks-audio --locked --offline -- --test-threads=1
cargo build -p aestra-bevy --example fireworks --features fireworks-audio --locked --offline
```

For a fresh native matrix, use `run-star-path-review.ps1` with fresh paths
`ROOT/medium-r1`, `medium-r2`, `medium-r3` (`-Tiers medium -Cases close-lit`),
`ROOT/high` (`-Tiers high -Cases close-lit`) and `ROOT/low` (`-Tiers low -Cases close-lit`).
Then run:

```powershell
python benchmarks/fireworks/check-fireworks-output-audit.py ROOT
```

The checker uses only Python's standard library and requires exactly those five
cells. The evidence records each binary, source inventory, arguments, native exit
code, report/capture hashes and failed controls. No historical evidence is edited
to match current sources.

The 32-tick ring and bounded async transport remain finite. This does not guarantee
lossless outputs when submissions/callback gaps exceed retention. Pulse occurrence
times/durations are unchanged; truly late pulses can still expire. Independent
non-routed legacy homing retains its existing distance heuristic; this fix targets
the coupled event/trail/domain path. Backlogged frames now perform legitimate
exports they previously skipped, so no isolated speedup/cost claim is made.

Next: resume saved smoke art tuning—less regular radial strands and broader
asymmetric wind-drifted haze—under the retained cue/admission/capacity/drain checks.
Selected-star lighting authoring, generic fluid coupling, whole-host tail latency
and default/finale art acceptance remain separate gates.
