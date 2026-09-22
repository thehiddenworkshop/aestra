# M9 — analytic vs stateful GPU cost: crossover findings

Evidence for the hybrid execution policy, produced by `aestra-bench --gpu-sim` (roadmap M9). **This is
data, not a policy.** The runtime does not switch backends based on it; the guiding rule stays "use the
least stateful execution model that preserves the effect's semantics, and let measurements decide
between equivalent implementations."

## What is measured

The same semantic workload — one continuous single-emitter sprite effect (sphere spawn volume, ranged
speed/lifetime, gravity + drag + turbulence) — is run two ways on real GPU compute, and the per-frame
GPU time is measured with timestamp queries (median of 96 frames after 24 warmup frames):

- **analytic** — the production `simulate` kernel (`reset` + `simulate`), which recomputes every slot
  from scratch each frame (full-capacity dispatch).
- **stateful** — the M6 death loop (`death_integrate` + `spawn`, one fixed tick) plus `present`
  extraction, over persistent state.

Swept over capacity ∈ {4 096, 32 768, 131 072} × occupancy ∈ {5 %, 25 %, 100 %}. Reproduce with:

```bash
cargo run -p aestra-bench --bin aestra-bench --features gpu -- --gpu-sim --frames 96 --warmup 24
```

## Crossover matrix (NVIDIA GeForce RTX 4070 SUPER, Vulkan)

| capacity | occupancy | analytic (median) | stateful (median) | stateful / analytic | cheaper |
| -------: | --------: | ----------------: | ----------------: | ------------------: | :------ |
|    4 096 |       5 % |          8 352 ns |          7 040 ns |               0.84× | **stateful** |
|    4 096 |      25 % |          7 712 ns |          7 136 ns |               0.93× | **stateful** |
|    4 096 |     100 % |          6 144 ns |          7 424 ns |               1.21× | analytic |
|   32 768 |       5 % |         13 568 ns |         16 352 ns |               1.21× | analytic |
|   32 768 |      25 % |         16 128 ns |         17 536 ns |               1.09× | analytic |
|   32 768 |     100 % |         14 176 ns |         21 280 ns |               1.50× | analytic |
|  131 072 |       5 % |         31 776 ns |         46 336 ns |               1.46× | analytic |
|  131 072 |      25 % |         54 848 ns |         49 888 ns |               0.91× | **stateful** |
|  131 072 |     100 % |         31 360 ns |         64 288 ns |               2.05× | analytic |

Per-slot persistent memory: analytic **0 bytes** (recomputed each frame), stateful **40 bytes**
(9-float state + 1 free-list word), plus the checkpoint store. Dispatches per frame: analytic **2**,
stateful **3**.

## The two questions M9 was meant to answer

**For which workloads is analytic cheaper than stateful (on this hardware)?**
For this simple effect, analytic wins in most cells. Stateful only wins at small capacity with low
occupancy (few live particles, where the death loop's cheap Euler step beats the analytic
reconstruction and dispatch overhead dominates both) and one large-capacity/partial-fill cell. This is
expected: the stateful path does *more* per frame here — it maintains persistent state (death + spawn)
**and** presents, while analytic recomputes and presents in one kernel. The value of stateful is
enabling history-dependent features analytic *cannot* express (collision, constraints, fluids), not
raw per-frame speed for effects analytic already handles.

**How much does sparse capacity hurt the analytic path?**
A lot: analytic pays ~full-capacity cost regardless of occupancy. At capacity 131 072 the analytic
median is essentially identical at 5 % and 100 % occupancy (31 776 ns vs 31 360 ns) — a 5 %-occupied
effect wastes ~95 % of the dispatch on not-yet-spawned / dead slots. (Curiously the 25 % cell is the
*most* expensive at 54 848 ns: the analytic emission reconstruction — inverting cumulative emission per
slot — costs the most at partial fill, where many slots are near their spawn boundary. That
non-monotonicity is itself a candidate for the "compact active worklist" analytic optimization the
roadmap lists.)

## Caveats

- **One GPU, one effect shape.** These numbers are the RTX 4070 SUPER on Vulkan with a moderate-cost
  analytic effect. The crossover moves with hardware and with how expensive the analytic reconstruction
  is: a curve-heavy effect (many `sample_curve` / gradient evaluations per particle per frame) makes
  analytic dearer and shifts more cells to stateful. Richer analytic workloads and more hardware are
  the obvious next sweeps.
- **Steady-state per-frame only.** Seek/checkpoint costs (restore + replay) are not in this table; the
  M7 store amortizes them across a scrub but they are a separate axis to measure.
- **Do not auto-switch.** Per the roadmap, this is a documented crossover for humans, not a runtime
  heuristic keyed off a single device's result.
