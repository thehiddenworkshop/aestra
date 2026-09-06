# Trail capacity / history-fill measurements

48 schema-v3 reports captured from `fba2aa3+worktree`, seed 7, on Windows with
AMD Radeon(TM) Graphics (integrated, vendor 4098, device 5710). Each shape/backend
has three runs, each with 8 warmups and 64 alternating A/B pairs per cell.

Matrix: owner capacities **128 / 1,024**, point capacities **16 / 64**, requested
history fill **25 / 100%**, owner occupancies **5 / 50 / 100%**, views **1 / 4 / 8**,
and **Vulkan / DirectX 12**. All **432 A/B image comparisons** passed exact nonblank
equality; all **864 path results** contain 64 total-time samples. Every iteration
checks timestamps and complete indirect commands for every view. Report metadata
and candidate/submitted counts were independently checked against the fixture dimensions.

## Reproduce

Use a new output directory for subsequent captures. Filenames identify owner capacity
(`c`), points including head (`p`), requested fill percent (`f`), backend and run.
Each JSON includes actual retained sample counts/fill, actual owner counts/occupancy,
adapter/driver, raw total and stage samples, and signed percentile/paired savings.

```powershell
foreach ($trailRun in 1..3) {
  foreach ($trailCapacity in @(128,1024)) {
    foreach ($trailPoints in @(16,64)) {
      foreach ($trailFill in @(25,100)) {
        foreach ($trailBackend in @('vulkan','dx12')) {
          $env:WGPU_BACKEND = $trailBackend
          cargo +1.98.1-x86_64-pc-windows-msvc run -p aestra-bench --features gpu --locked -- --gpu-trails sweep --owner-capacity $trailCapacity --history-points $trailPoints --history-fill $trailFill --occupancies 5,50,100 --views 1,4,8 --seed 7 --commit <revision> --out "benchmarks/gpu-baselines/<run>/c$trailCapacity-p$trailPoints-f$trailFill-$trailBackend-$trailRun.json"
          if ($LASTEXITCODE -ne 0) { throw 'Benchmark failed; no valid report for this run' }
        }
      }
    }
  }
}
Remove-Item Env:WGPU_BACKEND
```

## Selected results: every owner active, four views

Ranges are observed **total median savings (%)** across three runs, not confidence
intervals. Positive means compacted rendering is faster including preparation.
Quarter fill is 4/15 samples (26.67%) for 16 points and 16/63 (25.40%) for 64 points.
The head is separate; each retained sample contributes one valid body segment.

| Owner capacity | Points | Retained samples | Vulkan savings | DX12 savings |
| ---: | ---: | ---: | ---: | ---: |
| 128 | 16 | 4 | +30.7 to +31.8 | +33.8 to +37.2 |
| 128 | 16 | 15 | -23.4 to -23.0 | -19.2 to -17.2 |
| 128 | 64 | 16 | +22.9 to +24.0 | +25.2 to +26.2 |
| 128 | 64 | 63 | -20.8 to -20.7 | -14.7 to -12.1 |
| 1,024 | 16 | 4 | +37.0 to +38.8 | +40.7 to +41.6 |
| 1,024 | 16 | 15 | -33.9 to -30.1 | -22.0 to -21.7 |
| 1,024 | 64 | 16 | -7.2 to +5.2 | +24.2 to +26.4 |
| 1,024 | 64 | 63 | -44.8 to -39.9 | -33.1 to -28.4 |

Representative raw reports: [128/16/quarter Vulkan](c128-p16-f25-vulkan-1.json),
[128/16/full Vulkan](c128-p16-f100-vulkan-1.json),
[1,024/64/quarter Vulkan](c1024-p64-f25-vulkan-1.json),
[1,024/64/quarter DX12](c1024-p64-f25-dx12-1.json). Other repeats and all occupancies/
view counts are in the adjacent JSON files using the naming convention above.

Observations:

- Owner occupancy alone does not predict the crossover. At 100% owner occupancy,
  quarter-filled histories often win while full histories lose in the same shape.
- Capacity and history length matter too: four-view Vulkan with quarter-filled
  64-point histories is consistently faster at 128 owners but changes sign at 1,024.
- The earlier one-view regressions are not universal: DX12 at 128 active owners,
  64 points and 16 retained samples saves **3.6–5.0% median / 10.2–12.3% p95**.
  This is a small fixture-specific win, not permission to enable a global rule.
- Tails remain variable. At 1,024 owners, 64 points and quarter fill, four-view DX12
  has positive median savings but p95 savings range **-4.7% to +15.7%**. Four-view
  Vulkan at 1,024 owners / 16 points / quarter fill ranges **-88.0% to +41.4%** in
  p95 savings despite positive medians. Keep raw samples when investigating noise.

## Limits and next gate

The fixture uses uniform history fill, a fixed 1,024 × 512 target per view, flat caps,
all owners visible and a fixed camera family. Shortening history preserves sample
spacing and removes older positions, so pixel coverage changes along with valid
segment count. Increasing point capacity changes trajectory sampling density. These
are controlled production-shader measurements, not whole-application frame timing.

Minimum-shape correctness smokes also passed for one owner with one retained segment
in a 64-point allocation and for the minimum two-point allocation, at one/eight views
on DX12; a partial-history smoke passed on Vulkan. These one-sample diagnostics are
not performance baselines.

No runtime bypass is introduced. Before choosing one, repeat on another physical GPU,
then test mixed history lengths/expiry and varying visibility near the measured
crossover. A candidate policy must account for valid segment density, allocation size,
views and backend costs, with a total-time margin and tail checks. This machine's two
backends are not cross-hardware validation.
