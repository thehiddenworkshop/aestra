# Trail compaction occupancy / view sweep

Six reports from `83e2558+worktree`, seed 7, on Windows / AMD Radeon(TM) Graphics
(integrated, vendor 4098, device 5710): three runs each on Vulkan and DirectX 12.
Backends alternate between runs. Each report contains 32 A/B comparisons and 64 path
results, with 8 warmups and 64 measured pairs per cell. All 192 comparisons passed
exact nonblank image equivalence; every sampled view passed indirect-command and
timestamp validation. Image readback is outside the timed window.

Reproduce each run (set the backend and output filename accordingly):

```powershell
$env:WGPU_BACKEND = 'vulkan'
cargo +1.98.1-x86_64-pc-windows-msvc run -p aestra-bench --features gpu --locked -- --gpu-trails sweep --seed 7 --commit <revision> --out benchmarks/gpu-baselines/<run>/sweep-vulkan-1.json
Remove-Item Env:WGPU_BACKEND
```

Raw reports: [Vulkan 1](sweep-vulkan-1.json), [2](sweep-vulkan-2.json),
[3](sweep-vulkan-3.json); [DirectX 12 1](sweep-dx12-1.json),
[2](sweep-dx12-2.json), [3](sweep-dx12-3.json).

## Observations

The table gives the range of **total median savings (%)** across three runs.
Positive means compaction is faster, including its preparation cost. These ranges
are observed extrema, not confidence intervals. One- and two-view total medians
favor full-range drawing at every sampled occupancy on both backends.

| Requested occupancy (owners) | Vulkan, 4 views | DX12, 4 views | Vulkan, 8 views | DX12, 8 views |
| --- | ---: | ---: | ---: | ---: |
| 1% (10) | +4.4 to +6.0 | +15.6 to +19.3 | +49.1 to +51.5 | +46.8 to +49.1 |
| 2% (20) | +3.1 to +4.1 | +21.6 to +22.9 | +39.1 to +47.0 | +45.6 to +47.7 |
| 5% (51) | -10.3 to -8.8 | +14.2 to +16.4 | +33.0 to +33.4 | +36.9 to +38.4 |
| 10% (102) | -12.5 to -9.5 | +13.5 to +14.7 | +26.2 to +29.6 | +30.2 to +32.5 |
| 25% (256) | -18.6 to -12.7 | +9.1 to +12.2 | +6.0 to +16.0 | +18.8 to +20.5 |
| 50% (512) | -16.1 to -12.4 | -6.0 to -2.9 | +0.5 to +6.8 | -1.1 to +5.9 |
| 75% (768) | -40.0 to -28.7 | -22.8 to -21.2 | -19.6 to -9.3 | -13.9 to -6.5 |
| 100% (1,024) | -41.6 to -40.1 | -35.3 to -30.0 | -26.6 to -22.9 | -22.4 to -19.6 |

Sampled median crossover brackets:

- Four views: between 20 and 51 owners (~2–5%) on Vulkan, versus 256 and
  512 owners (25–50%) on DX12.
- Eight views: Vulkan changes sign between 512 and 768 owners (50–75%).
  DX12 is already uncertain at 512 owners: median savings change sign across runs.
- No sampled median win at one or two views; this does not prove none exists
  outside this fixture or between the sampled points.

Tail behavior prevents treating median wins as guaranteed improvements. For example,
four-view Vulkan at 20 owners has p95 savings from -65.2% to +57.2%; DX12 at 10
owners ranges from -165.9% to +22.2%. Eight-view Vulkan at 512 owners has only
+0.5% to +6.8% median savings despite consistently positive p95 savings (+1.1%
to +2.5%). Keep the raw paired measurements when refining these marginal cases.

## Limits / next gate

This is a controlled production-shader rendering fixture, not whole-frame timing:
1,024 owner slots, 64 history points, flat caps, 1,024 × 512 targets and a fixed
camera family. Increasing occupancy also increases pixel coverage/alpha overdraw.
Active owners all have 63 valid segments, so this does not measure partially filled
histories, other capacities, resolutions, culling distributions or adapters.

No runtime bypass is added. Before selecting one, refine the sampled brackets and
validate different capacities/history fill and at least one other GPU. A policy
needs a robust total-time margin and acceptable tails, not just fewer instances or
an assumed monotonic occupancy threshold.
