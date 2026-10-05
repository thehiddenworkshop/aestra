# F7E4B3B2A — native allocation observability and no-churn gate

Base commit `2178f7f4` (idle-slot work retirement). This slice adds qualification
instrumentation, not a production allocator or a Bevy dependency patch.

## Method and boundaries

The viewer's opt-in `--particle-light-allocations` requires a headless native
particle-light benchmark. It calls the public locked-wgpu
`Device::generate_allocator_report()` once per **60 render observations**, at most
**64 snapshots**. Disabled, unscheduled, exhausted and unavailable reports remain
distinct; an unsupported backend cannot silently report zero memory.

Snapshots retain the backend's complete reported suballocation names, sizes,
block-local offsets, block membership and allocated/reserved totals. The validator
checks accounting, range coverage, bounds and non-overlap. Named native clustering
metadata/staging allocations can be counted without naming Bevy's private types.
Their measured-snapshot qualification ceiling is **eight allocations each**, not
an enforced runtime limit. Unlabeled Z-slice/scratchpad buffers stay unattributed;
they are not guessed from sizes or offsets.

Public cluster bindings additionally retain hashed **wgpu handle identities**, not
labels or sizes. Adjacent identity changes detect observed same-sized replacements.
No GPU buffer clones are kept, so instrumentation does not artificially extend
resource lifetimes. Fingerprints are not complete allocation-generation identities:
they cannot detect every create/free operation between observations or certify
the absence of hash collisions.

The allocator census is whole-device backend suballocation evidence, including
resources still held for submitted work when reported by that backend. It is
**not total process/driver VRAM**, a cluster-only total, exact submission-fence
retirement, allocator overhead, complete dedicated-allocation coverage or a hard
memory cap. Snapshot block ordering/offsets/names are not stable resource IDs.
No new GPU copies/maps/waits are introduced. The CPU allocator lock/census can
perturb pacing, so these allocation runs must not substitute for ordinary B3A/B3B1
matched-cost evidence.

The repeated matrix keeps three alternating adapter-on/selection-only-control
pairs per high-tier hero, volley and show. Authored sources/seeds/capacities,
materials/trails, selection cap 96, adapter caps 96/0 and show representative flashes
remain unchanged. The existing work/hash/native-exit/overflow-log gates and fixed
show cleanup windows remain required. Full distributions remain in raw reports,
but no instrumented timing improvement is claimed.

## Source-backed churn finding

Locked Bevy **0.19.1** `bevy_pbr/src/cluster/gpu.rs`,
`prepare_clusters_for_gpu_clustering`, constructs a new `ViewClusterBindings` and
`ViewGpuClusteringBuffers` for every view on every call. It reserves/uploads the
index/offset lists and allocates the Z-slice/scratchpad data before replacing the
view's components. This occurs even with stable dimensions/capacities and no active
Aestra light sources. The ordinary selection-only control exercises the same path.

The pilot confirms public list fingerprints change on every measured observation,
despite constant **2097152-byte index / 117504-byte offsets-counts** bindings. Thus
the earlier public size-stability gate was correctly qualified: it could not prove
no allocation churn. The source also explains why private metadata/Z-slice/scratchpad
creation needs a dependency-level fix; their complete generations and fence lifetimes
are still not measured by the census.

Do not hide this by swapping old bindings back *after* native preparation: allocation
has already occurred and private metadata/bind-group consistency could be broken.
The next fix should reuse native per-view buffer containers before allocating,
preserve capacity growth, refresh metadata and safely drop removed views. It needs
a reproducible Bevy dependency-level integration, not edits to the user's Cargo
registry cache or a host-side imitation of private structs.

## Reproduction

```powershell
cargo build --locked -p aestra-viewer
# Fresh directory only. A complete matrix may exit nonzero when its qualification
# gate fails; native reports/logs and the explicit unaccepted summary are retained.
& benchmarks/fireworks/run-particle-light-costs.ps1 -ReportsDirectory target/fireworks-f7/gpu-allocations-fresh -AllocationSnapshots
& benchmarks/fireworks/validate-particle-light-allocations.ps1 -ReportsDirectory target/fireworks-f7/gpu-allocations-fresh -MeasureOnly
& benchmarks/fireworks/validate-particle-light-allocations.ps1 -SelfTest
```

`-MeasureOnly` preserves validated measurements and `accepted:false`; it does not
approve churn. Without it the read-only validator throws after reporting the failed
gate. The runner records a qualification failure separately from a native process
failure, and never marks a rejected matrix accepted.

## Repeated high-tier results — 2026-10-05

RTX 4070 SUPER / Vulkan, 960×540, audience camera, playback-only, fixed authored
inputs and one view. All **18 native processes exit 0**; workload/resource/hash/log
and retirement gates pass. **Allocation qualification fails in all 18 cells**:

| Probe, each GPU/control repetition | Measured binding observations | Replacements of each index/offset list | Measured allocator snapshots |
| --- | ---: | ---: | ---: |
| Hero | 598 | 597 | 10 |
| Volley | 598 | 597 | 10 |
| Show | 1678 | 1677 | 27–28 |

Each public list changes on every adjacent measured observation in enabled **and
control** runs, although both binding sizes remain 2097152 / 117504 bytes. Global
light storage changes zero times in hero/volley and twice in each show; this is
distinct from unconditional per-view recreation and is not itself a zero-growth
gate. The unchanged three show cleanup windows match control at one acknowledged
index and preserve 8,413 admitted children. No-churn failure does not undo B3B1's
work-retirement result.

Measured snapshots contain **1–2 named metadata allocations** (48–96 bytes) and
**2–3 named staging allocations** (96–144 bytes); all remain below the sampled
eight-allocation ceiling. The multiplicity does not prove exact in-flight lifetime
or private Z/scratchpad attribution. Sampled whole-device allocated peaks across
the 18 processes span **114461344–132559920 bytes**; reserved blocks are
**268435456 bytes** throughout measured snapshots. These include other resources,
not only lights. Measured snapshot CPU time spans **0.0245–0.2282 ms**; instrumentation
is explicitly excluded from ordinary matched-cost improvement claims.

Complete raw reports/logs are retained under
`target/fireworks-f7/gpu-allocation-census-2026-10-05`, with hashes and compact census
ranges in [the tracked evidence](particle-light-gpu-allocations-2026-10-05.json).
The original post-run validator threw on an empty startup name-group sum under
PowerShell strict mode. Empty groups now remain zero counts/bytes and fail the
measured named-presence gate. A new cold-census regression passes. All unchanged
native files were revalidated; a regenerated compact summary and manifest explicitly
retain `accepted:false` and that validator recovery. No failing run was discarded.

The initial pilot exited 1 because its output parent directory was missing; no
report was retained and that attempt is explicitly unaccepted. A fresh-directory
pilot completed successfully. This setup error is not an allocator failure or an
acceptance substitute. Earlier B3A abnormal native shutdown evidence also remains
open; instrumentation is not a claim to fix it.

## Remaining qualification

Resolve native per-view recreation, then repeat the no-churn gate. Private unnamed
buffer attribution, exact creation/deferred-retirement traces and deliberately
oversubscribed overflow controls remain open. Public native demand/log observations
and sampled named-private counts are not comprehensive overflow or resident-memory
certification. Medium/low costs/images, general per-view layers/multiple views,
additional hardware/cadences, natural-star art, lit smoke and production finale
remain separate gates.
