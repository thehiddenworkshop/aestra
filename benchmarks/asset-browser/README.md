# Asset Browser 10,000-source acceptance benchmark

Run from the repository root on an otherwise idle host:

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-editor --bin aestra-editor --locked asset_browser_10k_baseline -- --ignored --nocapture --test-threads=1
```

The harness lives here and is included in the editor's test binary to measure its
actual private state projection and retained Feathers UI. It creates a temporary
project (100 folders, 100 zero-byte PNG-classified sources each), never modifies
the user's assets, and cleans up automatically. These are discovery/classification
fixtures, not decoded textures or a semantic-material compilation workload.

Reports include cold snapshot discovery, 64 filter/sort samples after 8 warmups,
headless startup (including a second scan), 64 paging/selection samples after 8
warmups, idle updates, and peak live ECS entities. Correctness assertions enforce
the 192-row cache bound, stable selection entities and unchanged authored state.

This does **not** measure native input latency, GPU rendering, image decoding,
layout/raster time or detached-window behavior. Timings are observations, not
flaky CI thresholds; structural bounds remain assertions. Record the toolchain,
profile, host and code revision with each baseline. Native narrow/wide/high-DPI
and floating-panel acceptance remains a separate manual gate.
