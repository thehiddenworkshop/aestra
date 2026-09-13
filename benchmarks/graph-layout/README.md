# Local resize solver probes (M4)

Run from the repository root:

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-editor --bin aestra-editor feathers::node_graph::resize -- --nocapture
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-editor --bin aestra-editor resize_timing_profile -- --ignored --nocapture --test-threads=1
```

`resize_profile.rs` is included in the editor's test-only solver module so the probe
uses the actual private implementation without exposing editor internals as a library.
It never touches a project, native window, GPU, or persistent layout.

Fixtures contain 25, 50, 100, 250 and 500 logical 100-unit nodes. A root grows 50 units
wide. Sparse fixtures are no-ops; packed rows cascade right; a frozen first neighbor
gives an impossible anchor conflict. Larger cascades intentionally hit local-work
budgets instead of completing an unrequested graph-wide rearrangement. The normal
test suite asserts operation-count limits; the ignored timing probe prints average
solve time across 100 repetitions, result/conflict and exact work counters.

Defaults allow ordinary small local edits (up to 64 neighbors, 256 translations) while
bounding direct-scan work at 200,000 pair checks. Per-node/total L1 movement limits of
2,048/8,192 logical units also reject excessive shifts. These are conservative internal
policy limits, not a performance guarantee or a reason to enable live pushing early.
M5 must still validate cause lifetimes, owner selection and reversible offsets.

Record the toolchain, profile, revision and host with timings. They exclude measurement,
wire updates, UI layout, frame/input latency, history and overlay application.

## Baseline — 2026-09-13

`85476c9` plus the M4 worktree, Windows x86_64, Rust 1.98.1 MSVC, default `test`
profile (optimized + debuginfo), CPU identifier AMD64 Family 25 Model 97 Stepping 2,
AuthenticAMD. One local run, averages across 100 solves; not
cross-hardware guarantees. Inputs are in memory and no editor window was driven.

| Nodes | No-op average | Cascade average | Cascade result / pair checks | Frozen conflict average |
| ---: | ---: | ---: | --- | ---: |
| 25 | 2.000 µs | 104.741 µs | 24 moves / 7,524 | 0.684 µs |
| 50 | 2.106 µs | 874.233 µs | 49 moves / 61,299 | 0.869 µs |
| 100 | 3.104 µs | 3.119 ms | rejected / 200,000 | 2.335 µs |
| 250 | 13.042 µs | 3.158 ms | rejected / 200,000 | 2.956 µs |
| 500 | 15.751 µs | 3.161 ms | rejected / 200,000 | 5.613 µs |

Every no-op has exactly N−1 pair checks; every frozen conflict stops after one.
The larger cascades reach the pair-check cap before the 64-node cap. They return no
candidate and leave the snapshot unchanged. These measurements support keeping the
first solver deliberately local; M5 may tune policy using real resize neighborhoods.
No spatial-index dependency or global arrangement fallback was added.
