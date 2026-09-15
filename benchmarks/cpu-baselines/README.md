# CPU performance baselines

Recorded performance references for the Aestra CPU runtime, captured by `aestra-bench`. These are
**reference artifacts, not test gates**: they are machine-specific and are *not* asserted by
`cargo test`. Timing that varies by hardware must never fail CI, so it lives here as data to compare
against by hand or with tooling — never as a `#[test]` assertion.

## `S0_foundation.json`

The S0 foundation performance baseline (see `docs/new/aestra_foundation_tasks_S0_S1.md`, task S0-B):
all eight `aestra-bench` CPU scenarios (`b001`–`b008`), so the shared-foundation refactor (S1) has a
before-picture to measure against. Each report embeds its own provenance — `commit`, `seed`, and a
`hardware` block (CPU logical cores, OS, arch, backend) — so a baseline is always self-describing.

Captured with a release build:

```
cargo run -p aestra-bench --release -- --all --out benchmarks/cpu-baselines/S0_foundation.json --commit <sha>
```

Regenerate **deliberately** on your own reference hardware — the committed file reflects whatever
machine produced it, so compare only against baselines taken on the same hardware. A debug build is
meaningless here; always use `--release`.

### What to watch

`b004_sparse_large` (500k capacity, ~1% occupancy) is the load-bearing scenario: it exposes the
capacity-bound cost of analytic simulation — work scales with *configured slots*, not *alive
particles*. In the S0 capture it costs ~1.1k ns per 1k slots but ~112k ns per 1k alive (~100×). That
gap is the central motivation for the hybrid-simulation roadmap's active-worklist / per-emitter
dispatch experiments (its §25) and for keeping the hybrid execution-class decision benchmark-driven.

The GPU trail experiments write their own reports under `benchmarks/gpu-baselines/`
(`aestra-bench --features gpu --gpu-trails …`); this directory is the CPU lane only.
