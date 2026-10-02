# F4L — authored-shell review evidence, 2026-10-02

Technical capture checks passed; **artistic/reference-footage acceptance remains open**.
This is a review of the four bounded F3 prototypes, not AAA-density, animation or finale
certification. No effects, materials, runtime APIs or shipping defaults were changed.

## Reproduction and provenance

```powershell
pwsh -File benchmarks/fireworks/capture-shell-review.ps1 -OutputDirectory target/fireworks-f4/shell-review-new
```

The local evidence is under `target/fireworks-f4/f4l-shell-review/`: 24 cases and 192 individual
frame PNGs, plus contact sheets and reports. These ignored artifacts are reproducible, not
checked-in goldens. The manifest records runtime revision
`2f28e9327aaec4582fe2373a240482f099cd2d9c`, the dirty-worktree inventory and source hashes.
The capture runner/review documentation were new, uncommitted work during the run; unrelated
asset/document changes were preserved.

Windows, RTX 4070 SUPER/Vulkan, development build, high tier, 960×540, fixed 60 Hz/seed,
GPU/playback-only history, semantic materials, HDR/Tony/0 stops/bloom 0.15, stable transparency.
Every shell/camera pair has `authored` (floors 0/0) and `sampled` (2/2) versions. The stable sort
is deliberate for this small visual workload; it does not represent default fast playback.
See [the protocol](README.md#authored-shell-review-matrix--f4l) for exact lifecycle frames.

## Technical observations

- All 24 captures succeeded on the native GPU with compatible materials and the expected
  seed, response, dimensions and exact frame sequence.
- Every case observed a 256-star main-cohort peak; all six Pistil cases also observed the
  96-star inner layer. These are measured emitter peaks, not an audit of every spawn/event.
- At frame 420 (seven-second shells) or 540 (Willow), every report measured zero live
  particles, occupied/retired trails, evictions and truncation. This proves the observed
  endpoint cleanup; it does not certify every intermediate frame or requested/produced work.
- No timing claim is drawn from these exact-frame captures. They reconstruct history for
  deterministic positioning and are not real-time/full-show benchmarks.
- Script checks also reject stale output folders, CPU fallback, wrong frame/policy metadata,
  truncation, unavailable endpoint counters and an incomplete observed star cohort. Planning
  produces 24 distinct cases without writing files; filtered duplicate inputs are deduplicated.

## Static visual findings

Contact-sheet inspection covered close/audience/wide framing for each shell, with selected
authored/sampled pairs compared. These are qualitative observations, not calibrated brightness
measurements or a real-footage comparison. Native-resolution frame PNGs remain the source
for detail review; contact sheets downscale narrow features.

| Shell | Observed response | Remaining gap |
| --- | --- | --- |
| Peony | White launch flash, pink/red expansion, cooling red heads and fade are distinct. Sampling makes the wide launch trail much easier to see. | Late red heads remain faint in the wide view; density, radiance and decay need an agreed photographic target. |
| Chrysanthemum | Warm radial trails expand into falling, cooling arcs. Floor 2 improves continuity of narrow trails versus dotted untreated coverage. | The sampled treatment is visibly thicker; animation, taper/texture quality and artistic width acceptance remain unverified. |
| Pistil | Blue outer stars and a gold inner group are visually separated at audience/wide distance. | Late blue heads are dim; the brief flash is a soft white disk, not yet a reference-tuned burst. |
| Willow | Longer gold arcs and later falling tail decay distinguish it from Chrysanthemum. Floor 2 keeps thin tails readable. | Untreated late tails are dotted; sampled tails can read as uniform lines. Camera-relative width and cooling need temporal/reference review. |

Shared issues:

- The existing F0 close preset aims below the elevated shell burst: frame-80 flash and
  expanding upper shell are cropped, strongly for Peony/Chrysanthemum/Pistil. It is a detail
  crop, **not a valid whole-shell close acceptance view**. Audience framing also touches/crops
  some upper arcs. Fix the review camera composition before approving close references;
  this is viewer/host framing, not missing Aestra simulation or culling functionality.
- Smoke is a faint, localized sprite/plume approximation. This scene has no burst-driven
  lighting interaction; a brighter background/skybox/audio would be host work and is not
  added as a core API requirement by this review. F7/F8 retain their generic lighting/smoke
  integration gates.
- The same short launch and soft circular flash dominate all four prototypes. More realistic
  timing, brightness hierarchy, star variation and smoke response need chosen reference
  footage. The review does not justify new firework-specific runtime concepts.

## Decision

Keep runtime sprite/trail sampling defaults at zero; keep floor 2 as an explicit comparison.
Do not approve goldens, close F4, or claim AAA realism from this matrix. Fix review framing,
select the photographic target, and assess moving sequences before artistic approval.
Production-density/live/full-show and lower-tier gates remain separate. The next runtime
milestone remains F5's generic two-generation event-chain/host-cue validation; no further
open-ended culling optimization is justified by these images alone.
