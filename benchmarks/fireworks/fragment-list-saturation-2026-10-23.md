# F8.1C4E — fragment-list saturation diagnostics

The opt-in fragment-list experiment now distinguishes **allocation rejection**,
**global fragment loss**, and **local exact-sort saturation**. It is still not
enabled on the public fireworks show. No authored assets, playback/history rules,
sound/light routes or default renderer settings change.

## Bounded diagnostic transport

`ExperimentalOitDiagnostics` is exported by `aestra-bevy` and the renderer. Enable
it explicitly on the app resource with `set_enabled(true)`; poll `snapshot()` for
the latest completed result. `None` means disabled, pending or unavailable, not
zero loss. Camera, target/size, settings, rejection and request removal invalidate
old results at PostUpdate. Disabling invalidates immediately. Generation and
sequence checks reject late callbacks, including failures arriving out of order.
This is asynchronous observation, not a current-frame health/correctness guarantee.

The initial diagnostic supports exactly **one OIT camera**, rendering its full target
without a resolution override. Multiple OIT cameras, sub-viewports and resolution
overrides deliberately produce no sample; this does not qualify their rendering.
Each sample identifies the camera, target size and sort threshold. Counts include
all participants in Bevy's OIT list, not just Aestra effects.

Bevy's node/head/counter buffers lack COPY_SRC usage. Rather than changing or copying
them, a diagnostic compute pass reads them after the transparent pass and **before
Bevy resolves and clears the lists**. It reads the GPU attempt counter and walks at
most `sorted_max + 1` accepted nodes per pixel (currently at most 33). Index bounds
are checked; even a malformed cycle cannot make the diagnostic loop unbounded.
Only four aggregate u32s (16 bytes) cross to the CPU:

- attempted fragments;
- pixels whose accepted lists exceed the exact-sort threshold;
- invalid links encountered within the bounded traversal;
- maximum observed depth, capped at threshold + 1.

Global drops are `attempted.saturating_sub(actual_node_capacity)`. Pixel-threshold
counts describe **accepted** lists and are a lower bound when global loss occurs.
There is no fragment/particle CPU enumeration, blocking poll, or playback readback
when diagnostics are disabled. Three reusable output/staging pairs bound aggregate
storage to 96 bytes; backpressure skips samples rather than growing a queue. After
use these small slots and the pipeline remain resident. Uniform/bind-group and
diagnostic dispatch costs are additional and are not production performance claims.
Bevy's counter is u32: workloads approaching 2^32 attempts/frame are not qualified,
and counter wrap is not detected. Zero invalid links only describes the visited
prefix, not a full linked-list integrity proof.

## Native workload and observed limits

The existing ordinary compiled GPU-player fixture places two red half-alpha sprites
on opposite sides of a blue half-alpha sprite. Both camera directions and spawn
orders run in forward PlaybackOnly, frozen at frame 30, 480×360 sRGB LDR, MSAA off,
no tone mapping/lights/bloom, and no per-pool particle sort. Each app steps through:

| Configuration | GPU observation | Central linear-color error |
| --- | --- | --- |
| Average 4, local sort 8 | No global drops or over-sort pixels | .00104 |
| Average .01, local sort 8 | 1,728 nodes; 103,445–127,796 dropped fragments | .625 |
| Average 4, local sort 1 | No drops; 37,108 over-sort pixels | .25376–.37484 |
| Restore average 4, sort 8 | No drops or over-sort pixels | .00104 |

Attempted coverage stays constant across these capacity changes: 105,173 fragments
from one view and 129,524 from the other. Healthy storage holds 691,200 nodes. Global
exhaustion is **not rejected by the allocation guard**: the small allocation fits
the budget, but loses fragments during drawing. Its zero over-sort pixels cannot
be interpreted as healthy—the lost nodes no longer exist to be counted.

The one-sort-slot case proves that sufficient global storage alone is not enough.
The bounded prefix reports depth 2 (threshold + 1), whereas healthy cases observe
all three layers. Local approximation visibly darkens/miscomposes this test. Inspection
of pinned Bevy 0.20's resolve shader also shows approximate branches multiply stored
premultiplied RGB by alpha again, unlike the exact branch. This is a plausible
contributor, **not an isolated root-cause proof or an upstream fix**. No Bevy shader
is patched here and the local-sort configuration is not accepted as correct.

All sixteen stage/configuration cases check zero visited invalid links, the expected
global/local signals, and restoration. One-byte budget rejection clears telemetry
instead of fabricating a healthy sample. Restoring the budget resumes reporting;
disabling clears it and cannot be revived by an older callback. Global-loss and
approximate-tail image determinism are deliberately not acceptance requirements.

See [raw reports, source hashes and image hashes](fragment-list-saturation-2026-10-23.json)
for final repetitions and numeric approximation errors. These are diagnostic
**expected-degradation** passes, not smoke-art or production compositor acceptance.
Both final runs pass. All 76 corresponding existing healthy/lifecycle images match
byte-for-byte, as do all eight healthy/restored saturation images. None of the four
global-loss or four approximate-tail image pairs match byte-for-byte. That variability
is retained in the evidence, not treated as a stable visual reference; it reinforces
that the degraded paths cannot be accepted through repeatability or zero global drops.

## Reproduce and next gate

Run GPU work alone, using a fresh absolute output directory:

```powershell
$env:AESTRA_CROSS_POOL_IMAGES = 'C:/absolute/fresh-oit-saturation'
cargo test -p aestra-viewer fragment_list_ --locked --offline -- --ignored --test-threads=1
```

This runs the existing 16-case healthy/lifecycle oracle and the new 16-stage
saturation matrix sequentially. Ordinary renderer/viewer tests, strict workspace
and qualification-harness Clippy, formatting and whitespace checks remain required.
Final validation: renderer 169 pass / 3 native ignores; viewer 101 pass / 32 opt-in
ignores; both native tests pass in each of two final fresh runs. Strict all-target
workspace Clippy (`aestra-bench/gpu`) and isolated qualification-harness Clippy
(`editor-qualification`), both formatting checks and whitespace validation pass.
Windows emits existing incremental-cache hard-link fallback warnings. No Linux,
full-workspace test run, or public HDR/live/dense performance measurement is claimed.

Next, qualify **opaque-depth occlusion**, then lit/HDR/textured and mixed additive/
trail/non-OIT draws. Resize/multiple cameras and matched dense/live-paced GPU and
memory costs remain open. Decide whether the compositor needs changes or a different
approach before enabling it on the show or resuming continuous star-path smoke art.
