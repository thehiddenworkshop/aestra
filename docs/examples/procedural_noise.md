# Procedural material noise

In the Material Graph, open **Add Node → Noise**. There are eight built-in
functions: Value Noise and Gradient Noise, in 2D and 3D, plus an fBm variant of
each. They use the existing typed reusable-function nodes, sockets, editable
defaults and material parameters. They work inside saved project functions too.
No project-local custom shader is required.

New 2D nodes connect UV automatically; 3D nodes connect World Position. Replace
that connection to choose local/world coordinates or another typed position.
Offset is added **before** Scale: `(position + offset) * scale`. To animate a
field coherently, subtract `velocity * Effect Time` from its position. Effect
Time follows playback, so pause/restart does not introduce another clock.

| Input | Default | Numerical contract |
|---|---|---|
| Scale | 4 | Frequency, clamped to 0–4096 |
| Offset | zero vector | Same dimension as Position, in position units |
| Amplitude | 1 | Nonnegative output multiplier |
| Seed | 0 | Float socket, truncated to an integer after clamping to 0–16,777,215 |
| Octaves, fBm only | 4 | Truncated integer, clamped to 1–8 |
| Lacunarity, fBm only | 2 | Frequency multiplier per octave, clamped to 1–4 |
| Gain, fBm only | 0.5 | Amplitude multiplier per octave, clamped to 0–1 |
| Footprint | 0 | Nonnegative sample width in position units, **before Scale** |

Both bases return an unsigned field in 0–1 before Amplitude. fBm divides by the
sum of octave amplitudes, so raising Octaves does not automatically raise output
energy. To obtain signed noise, subtract 0.5 and multiply by 2. Value noise
interpolates hashed lattice values; gradient noise interpolates directional dot
products, both with quintic interpolation. Negative cells use floor, and each
basis octave wraps every 65,536 lattice cells. A whole fBm field is not necessarily
periodic by that amount for arbitrary noninteger lacunarity.

Footprint zero is explicitly unfiltered. Increasing it suppresses unresolved
octaves toward their mean of 0.5, rather than toward black. Each octave transitions
over footprint × frequency = 0.25–0.75. This works in particle, vertex, compute and
fragment evaluation without implicit derivatives. There is **no automatic camera
distance/derivative connection** in this first version; supply Footprint when
needed. An input-scale footprint must match the Position input's units.

Cost grows with visible octaves: four lattice corners per octave in 2D, eight in
3D. Use fewer octaves for large smoke overdraw; a baked texture may be cheaper.
Settings clamp rather than allocate unbounded loops. Finite, reasonably local
coordinates are expected; 32-bit floating point loses fractional detail at very
large world positions. Rebase coordinates before sampling those worlds.

## Smoke study

Open **Noise Smoke Lab** in `sample-project/effects`. **Noise Smoke** is its
material; **Noise Smoke Density** is a reusable graph function containing world
fBm Value Noise 3D and UV fBm Gradient Noise 2D. Its effect parameters expose
scale, seed, octaves, pattern velocity, footprint, density and ambient tint. Open the function
asset to edit the field composition; the built-in nodes also expose their
lacunarity, gain, offset and amplitude sockets.

The material/lab defaults use Scale 2.2 and a brighter ambient tint so the
unit-size editor swatch is readable. The fireworks instance binds Scale 0.22
and a dimmer ambient tint for its larger night-time world. These are ordinary
material overrides, not special thumbnail lighting or a second noise algorithm.

The material uses the shared graph/thumbnail CPU evaluator and native GPU
material compiler. Effect thumbnails and hover previews retain its transitive
function dependency. Their background budget is eight function levels, sixteen
expanded calls, 512 expanded expressions and four canonical noise/smoke kernels
per material. Those are background-preview limits, not playback or owner limits.
Arbitrary custom WESL remains excluded from that effect-preview route.

An opt-in fireworks comparison uses the same material and graph function:

```powershell
cargo run --release --locked -p aestra-demo-fireworks --bin fireworks -- --effect effects/fireworks_noise_smoke.aestra.ron --star-lights --no-audio
```

This study changes smoke shading, preserving the source effect's particle
modules, capacity, events and light outputs. It does not replace the current
fireworks repertoire and is not final smoke art. It is sprite smoke; it does not
provide volume self-shadowing, fluid simulation or particle curl forces.

Regenerate the shipped study assets with:

```powershell
cargo run --locked -p aestra-demo-fireworks --bin author_noise_material_lab
```

## Host API and verification

The serializable settings live in `aestra_core::noise`; CPU callers use:

```rust
let settings = aestra_core::noise::NoiseSettings {
    scale: 0.22,
    seed: 73,
    octaves: 4,
    ..Default::default()
};
let density = aestra_runtime::noise::sample([-1.25, 0.5, 2.0], &settings);
```

The CPU API accepts full `u32` seeds; material float sockets use the exactly
representable 24-bit subset. CPU sampling rejects nonfinite inputs with `None`.
Do not send NaNs/infinities to shader inputs. Value/basis settings and stable
built-in function IDs are versioned numerical contracts; incompatible algorithms
need new IDs. The compiler supplies the matching WESL through its existing
namespaced function lowering, avoiding a new expression opcode or a dependency
from the compiler back to the GPU crate. Only canonical registered built-ins
use the CPU evaluator; project/host overrides cannot impersonate one by ID alone.

Run native CPU/GPU parity explicitly on a machine with a GPU:

```powershell
cargo test --locked -p aestra-bevy-render --test noise_gpu -- --ignored --nocapture
```

The oracle checks all eight production entry functions, 4,096 samples each,
with negative coordinates, eight seeds, eight-octave fBm and explicit filtering.
Tolerance is 0.0002; bit-identical floating point across devices is not promised.
Optional `AESTRA_NOISE_REPORT` writes JSON to an existing absolute destination
directory. Timestamp results are one cold compute dispatch per function, not a
steady-state material or full-frame performance result. Portable shader tests
also validate WGSL, SPIR-V and HLSL. Spatial particle forces/distributions remain
the separate N2/N3 milestones.
