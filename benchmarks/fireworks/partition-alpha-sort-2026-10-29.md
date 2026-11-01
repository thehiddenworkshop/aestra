# Partition capture: remove redundant particle permutations

Approved partitioned fragment-list capture no longer prepares per-view alpha
sprite/flipbook depth permutations. Exact per-pixel list sorting already provides
the required order. This removes redundant compute, merge scratch, index buffers
and sorted vertex bindings; it does not change simulation or partition geometry.

Pipeline selection and permutation preparation share the same eligibility
predicate. Ordinary views, unpartitioned experimental OIT, unsupported material
states and unapproved cameras retain their previous sorting behavior. A requested
partition phase still fails closed while rejected or pending. The existing
per-view retention pass retires obsolete permutations and recreates them when
ordinary sorting is needed again. Particle identity is read through alive indices,
not inferred from render-instance order.

## Correctness

The native sprite oracle uses per-particle random color, two pools and opposite
cameras. All six 1×1/2×2/4×4 partition captures exactly match SDR screenshots of
the full-list HDR/Reinhard reference. Two reference permutations own 18,912 bytes;
partition capture owns zero and dispatches no particle sort. A simultaneous
ordinary camera keeps its two permutations and identical pixels; removing the
partition request restores the original view's sorting and image.

The existing native phase oracle also passes all 12 exact image cases and
regional health comparisons, overflow/recovery, foreign/invalid-view rejection,
empty resets, sticky loss, visibility, resize and ordinary restoration. Its PNG
pixels match the preceding regional-scan binary. This change does not modify
the per-pixel sorting or raw-HDR fragment ABI.

## Live qualification

The [JSON ledger](partition-alpha-sort-2026-10-29.json) retains executable/source
hashes, native results, separate real-time image controls and three measured
baseline/candidate runs. Initial baseline-1 overlapped a lint build and is retained
as an excluded warm-up; candidate-1 is also retained as warm-up. Accepted profile
runs have no concurrent Cargo builds. Baseline manifests fingerprint the current
checkout, not a reconstruction of old binary sources.

Measurements use the high-tier tuned host overlap, near view, PlaybackOnly,
selected star/flash lights and 4×4 partitions. The explicit 24-owner policy belongs
to this benchmark; normal host admission remains uncapped. Node/head storage
remains 47,923,200 bytes under the unchanged 128 MiB ceiling. The new report
records peak/last CPU-visible particle-sort buffer ownership independently of
OIT list buffers; this is not a total-VRAM or driver-residency measurement.

| Per-run p95, three runs | Baseline | Candidate |
|---|---:|---:|
| Active particle-sort GPU scope | 1.485–1.588 ms | **No dispatch/scope** |
| Active aggregate partition GPU scope | 12.770–12.810 ms | 12.751–12.870 ms |
| Host intervals including vsync | 19.539–24.735 ms | 18.127–20.714 ms |

The candidate removes a separately measured 1.5 ms-class GPU pass; it does not
accelerate the remaining partition algorithm. These are ranges of independent
per-run quantiles, not additive durations or a reconstructed total GPU frame.
Host intervals vary and still exceed 16.67 ms. All candidate live runs record
zero peak/last particle-sort pairs and buffer bytes.

The three image runs supply 15 captures. Near/orbit 4×4 observes zero fragment
loss/invalid links; the 2×2 near negative still loses **6,841,456 fragments** at
peak and remains rejected. Every run preserves **374,514 gold-tail ember** and
**115,283 star-path smoke births**, without source overflow, expansion omission
or destination rejection. All measured profile/image runs launch/detonate 12
rockets, accept 12 flashes and drain to zero particles/gameplay rockets/VFX owners.
Live captures are not exact frame-paired images; deterministic native controls
supply the exact image comparisons.

187 renderer tests pass (four native GPU tests remain explicitly ignored by the
ordinary suite); both relevant viewer GPU regressions ran explicitly. Strict
all-target Clippy for renderer/viewer/demo and workspace formatting pass.

## Limits and next work

This removes preprocessing before capture, not repeated per-tile vertex/material
work. No whole-frame/finale or final smoke-art acceptance is inferred. Ordinary
transparency remains the default and the partitioned path retains its existing
scene/view restrictions. Continue with conservative per-region geometry admission
or binning, preserving exact native images, birth demand and negative capacity
controls. Fix window startup admission before further timing changes: two image
attempts rejected zero estimated shared capacity before playback; their reports
and logs remain failed, with fresh retries recorded separately.
