# F7E4B1 — authored same-frame GPU lighting costs, 2026-10-04

Base commit `1a474979` (F7E4A). RTX 4070 SUPER/Vulkan; driver vendor NVIDIA,
not a recorded driver version. Optimized development build on the same development
host as earlier studies, not production-game performance certification.

## Result and scope

The viewer now explicitly chooses `--particle-light-mode gpu|async` with
`--particle-light-bench --particle-light-realization`; async remains default.
GPU mode disables selected-position transport and portable proxies automatically.
`--particle-light-gpu-cap 0` disables only GPU realization: selection remains
enabled at 96/48/24 and independent authored representative flashes remain enabled
in show controls. This differs from the global `--particle-light-cap 0` control.
The adapter cap defaults to the requested global cap, not a new fixed ceiling.

Hero (`f4-reference-hero`), overlapping volley (`f5-secondary-volley`) and thirteen-clip
show (`f6-show`) use the existing explicit F7D2B light-output fixture: 1500 lumens,
range 12, per-output caps 32/16/8. Normal materials, particle/history/event budgets,
transforms, appearance and choreography are unchanged. Each matrix cell runs once,
sequentially, with a fresh matching control. Fixed forward 60 Hz simulation,
playback-only, unpaced catch-up, pipelined rendering, 960×540 offscreen HDR/EV0,
Tony tonemapping/bloom 0.15, audience camera, fast transparency. Warmup 120 ticks,
600 measured ticks for hero/volley; 1680 for the 26-second show including cleanup.
No concurrent build/native GPU benchmark during retained runs.

## Native cluster preallocation matters

Preliminary runs under `target/fireworks-f7/gpu-workloads/` exposed **Bevy native
cluster-list overflow/resizing warnings** at the first busy show overlap. Bevy
warns lighting may be incorrect for a few frames before these buffers grow. These
warnings are not Aestra selector rejection and would be missed by selection-only
validation. Preliminary timings are not the retained acceptance matrix.

For both GPU-enabled and control runs the benchmark host now explicitly requests
initial Z-slice/index-list capacities of **4096/524288**, **2048/262144**, **1024/131072**
for high/medium/low. The entire matrix was rerun under
`target/fireworks-f7/gpu-workloads-preallocated/`; no cluster-resize warnings occurred.
These are measured-scene initial capacities, **not hard limits, actual allocated-byte
measurements, general host recommendations or a total GPU-memory cap**. Bevy can
grow them for other scenes/cameras/caps. Native cluster allocations/readbacks are
separate from Aestra's adapter budget and its disabled selected-position readback.
Actual renderer-memory and overflow/growth qualification remains a next gate.

Native headless runs still warn about missing window-based shadow LOD origin
(all these point lights are shadowless). Some short runs warn of closed readback
channels on shutdown; counter/GPU timing and lifecycle observations pass the
strict gate. No warnings are relabeled as visual acceptance.

## GPU costs

All values are p95 milliseconds from independent fresh diagnostic observations.
They are **not frame-paired**, cannot be summed into a frame, and differences
between single sequential runs do not establish incremental cost or a speedup.
`full_frame` is the outer render-graph timestamp window, not the full app/game or
display latency. The same host preallocation and flashes apply to the controls.

| Workload / tier | Selection | Injection | Native clustering | Render window | Control render window |
|---|---:|---:|---:|---:|---:|
| Hero high | 0.242 | 0.0102 | 0.0952 | 1.508 | 1.315 |
| Hero medium | 0.218 | 0.0092 | 0.0819 | 1.302 | 1.251 |
| Hero low | 0.211 | 0.0102 | 0.0819 | 1.288 | 1.070 |
| Volley high | 0.217 | 0.0154 | 0.0932 | 1.556 | 1.648 |
| Volley medium | 0.182 | 0.0092 | 0.0819 | 1.267 | 1.221 |
| Volley low | 0.163 | 0.0092 | 0.0911 | 1.203 | 0.977 |
| Show high | 1.363 | 0.0205 | 0.3471 | 7.953 | 7.972 |
| Show medium | 1.085 | 0.0195 | 0.2099 | 7.384 | 7.214 |
| Show low | 0.904 | 0.0164 | 0.1812 | 6.267 | 6.428 |

Show slot-maintenance CPU p95 is 0.0180/0.0117/0.0118 ms; source authorization
0.0151/0.0150/0.0143 ms; injection encoding 0.0368/0.0350/0.0333 ms. Their
outer render-window CPU p95 is 11.482/12.068/11.010 ms, not whole-main-loop
timing. The JSON retains every timing distribution, including CPU selection/
clustering, controls, sample counts and raw-report SHA-256.
Slot maintenance and authorization are latest independent wall-time observations;
GPU shared statistics can lag under pipelining. Injection spans include render
qualification, metadata uploads/bindings and dispatch encoding. Native cluster
timings include all host lights. Benchmark counter copies are sixteen bytes per
observation and happen outside the outer render-window span, as in F7D2B.

## Resource and produced-work gates

Observed selected peaks: hero **96/48/24**, volley **64/32/16**, show **96/48/24**.
Selected counts respect both global and per-output caps; all observed counter
algebra and source qualifications pass. Slot bounds are **96/48/24** in enabled
runs, **zero** in controls. Capacities/written-capacity are upper bounds, not an
exact count of illuminated GPU lights.

Maximum adapter logical bytes (reserved native light records + own metadata):
hero **9328/4720/2416**, volley **9312/4704/2400**, show **9744/5136/2832**.
These satisfy the independent 1 MiB adapter budget; qualified manifests separately
retain their 1 MiB limit. Selector reserved-byte peaks: hero **51600/27024/14736**,
volley **125552/30256/9424**, show **314960/111888/54064**, below the separate 64 MiB
selector budget. These are neither total resident/in-flight GPU memory nor
native cluster allocation measurements.

All eighteen reports have zero selected-position submissions/staging/pending maps
and zero portable proxies; controls have zero adapter dispatches/slots/buffer bytes
and no injection timing span while still producing selected sets. Enabled runs
have advancing dispatch observations, no shader/selection/adapter rejections or
invalid sources during the measured window. Normal transparent fragment work is
present. The show reaches 31 simultaneous source runs and independent flash peaks
**3/3/2**, within caps 8/4/2. Both enabled and controls preserve the strict thirteen-
clip admission/cleanup gate: **8413/3317/1217** admitted children, no event overflow,
omitted expansion, destination rejection, trail eviction/truncation or replay
checkpoints; final particle/history/clip counts, selected written-capacity and
representative active lights are zero. Reserved GPU slot entities remain bounded
for reuse; explicit adapter/global disable removes them.

## Reproduce

PowerShell 7, from the repository root; build first, then run native probes alone:

```powershell
cargo build --locked -p aestra-viewer
New-Item -ItemType Directory -Force target/fireworks-f7/gpu-workloads-preallocated | Out-Null
$probes = @{hero='f4-reference-hero'; volley='f5-secondary-volley'; show='f6-show'}
foreach ($probe in @('hero','volley','show')) {
    foreach ($tier in @('high','medium','low')) {
        foreach ($mode in @('gpu','control')) {
            $extra = @()
            if ($mode -eq 'control') { $extra += @('--particle-light-gpu-cap','0') }
            if ($probe -eq 'show') { $extra += '--transient-lights' }
            & target/debug/aestra-viewer.exe --fireworks-f0 --fireworks-f0-probe $probes[$probe] `
                --camera audience --backend gpu --history playback-only --hdr --particle-light-bench `
                --particle-light-realization --particle-light-mode gpu --headless-bench --tier $tier `
                --gpu-bench "target/fireworks-f7/gpu-workloads-preallocated/$probe-$mode-$tier.json" @extra
            if ($LASTEXITCODE -ne 0) { throw "Failed $probe/$tier/$mode" }
        }
    }
}
./benchmarks/fireworks/validate-particle-light-gpu-workloads.ps1 -ReportsDirectory target/fireworks-f7/gpu-workloads-preallocated
# Preserve the narrow paced native receiver/registration gate separately:
$env:AESTRA_GPU_LIGHT_ADAPTER_REPORTS = 'target/fireworks-f7/gpu-workloads-registration'
cargo test --locked -p aestra-bevy --test particle_light_latency bounded_gpu_adapter -- --ignored --nocapture
./benchmarks/fireworks/validate-particle-light-latency.ps1 -ReportsDirectory target/fireworks-f7/gpu-workloads-registration -GpuAdapter
Remove-Item Env:AESTRA_GPU_LIGHT_ADAPTER_REPORTS
```

The read-only validator rejects missing evidence rather than substituting zeros,
and invokes the existing show gate for both enabled and control reports.
Regression checks: viewer tests, Bevy adapter library tests, affected-crate
Clippy with `-D warnings`, workspace/all-target check, formatting and diff checks.
The fresh isolated native regression passes at 25/75/150 m/s, retaining the same
GPU spatial p95 residuals **0.00276/0.01145/0.03745 metres**, below the
0.4167/1.25/2-metre gate. These are equivalent spatial offsets, not measured
sub-millisecond scanout latency. Lifecycle/budget/independent-host-light image
assertions also pass. Fresh hashes and cadence distributions are retained under
`paced_registration_regression` in the JSON; earlier F7E4A evidence is untouched.

## Next: F7E4B2

**F7E4B1's profiling/current authored-load cost slice is implemented; F7E4B is not
fully closed.** The existing isolated 60 Hz native final-image gate still applies,
but this unpaced matrix is not authored perspective/bloom/high-density receiver
registration approval. Add matched authored receiver-on/off images and paced
registration evidence; repeat work-matched costs and qualify actual native cluster
memory/overflow/growth. Preserve default-layer restrictions until general per-view
layer/multi-view acceptance is proven. Additional hardware/cadences, editor
lighting, lit particle/volume smoke and production-finale certification remain open.
