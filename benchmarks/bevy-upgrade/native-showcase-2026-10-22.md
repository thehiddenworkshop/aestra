# Bevy 0.20 native fireworks showcase check

Date: 2026-10-22. Production engine baseline: `32575f76` (Bevy 0.20.0).
The public `aestra-bevy` example was built and executed, not a viewer substitute.
No runtime, effect, material or audio assets were changed for this check.

## Configuration and result

Windows 11 build 26200; Ryzen 7 7800X3D; RTX 4070 SUPER; NVIDIA driver 621.01;
Vulkan. Rust 1.98.1; development build with `fireworks-audio`; high quality;
PlaybackOnly; default audience camera, fixed HDR exposure and bloom. The existing
capture hook waits for delivered screenshots before requesting successful exit.

All seven native runs exited successfully. Logs contain no WARN/ERROR, shader
validation failure, preparation timeout or fallback to CPU presentation. The first
10-second run inherited `RUST_LOG=warn`, so its empty log supplies no INFO counters;
all subsequent runs explicitly used `RUST_LOG=info` and identify the native adapter.

Raw PNGs and per-run logs: `target/bevy-020-showcase-acceptance/`.

| Capture stem | Time | PNG size | Authored flashes admitted | Audio |
| --- | ---: | --- | ---: | --- |
| `default-26s-audio` | 26 s | 1280 × 720 | 13 | Enabled |
| `wispy-2-6s` | 2.6 s | 1280 × 720 | 1 | Disabled |
| `wispy-9-3s` | 9.3 s | 1280 × 720 | 7 | Disabled |
| `wispy-10s` | 10 s | 1280 × 720 | Not logged | Disabled |
| `wispy-18-4s` | 18.4 s | 1280 × 720 | 12 | Disabled |
| `wispy-30s` | 30 s | 3440 × 1369 | 13 | Disabled |
| `wispy-37s` | 37 s | 3440 × 1369 | 13 | Enabled |

Every INFO-enabled capture reports zero invalid/stale flash bindings. Peak
representative lights reached three. The 9.3-second image shows three active lights
and differently colored illumination on earlier smoke. It is an observation, not
an on/off differential oracle; the separately recorded native light probes remain
the stronger functional lighting evidence.

## Visual observations and limitations

- Rocket-path smoke is visible below the first shell at 2.6 seconds, rather than
  all smoke being launched upward from the origin. Star ribbons and additive
  sprites are visible at 10 and 18.4 seconds.
- Earlier clouds persist across later bursts. A dim smoke tail remains at 30
  seconds; the 37-second finished image is clear of visible particles. An empty
  image alone does not certify zero GPU allocations or population counts.
- The existing wisp draft is less spherical than the historical billow candidate,
  but remains localized noisy clouds. Thin continuous star-path smoke, richer
  depth and photographic/art approval remain open. This pass does not establish
  image equivalence with Bevy 0.19 or approve new golden references.
- Captures are asynchronous real-time observations. The two late windows had
  different dimensions; no matched-resolution performance comparison is valid.
  HUD samples ranged from about 16.4 to 33.8 ms, including vsync. They are not
  full-show percentiles, isolated GPU time or a release-build performance guarantee.
  A matched release benchmark is still required.

## Audio and tests

Both complete audio-enabled runs (default 26 s and wispy 37 s) reported **134
voices admitted, 89 dropped, zero pending, 24 peak Bevy spatial sinks**. The local
pack contains all 15 expected WAVs; no missing-bank, decoding or device warnings
were logged. These counters match the earlier 2026-10-05 default-show evidence in
`docs/examples/fireworks.md`. They qualify native binding/decoding and bounded
delivery, not an approved mix or a listening test. The 24-voice budget still drops
requests; it is not a lossless sound route. Launch remains intentionally unbound.
The local third-party WAV pack was neither modified nor added to Git.

All **18 audio-enabled example tests** pass, including occurrence-time propagation
delay, stale/duplicate rejection, voice limits and restart/despawn cleanup. Strict
Clippy passes for `fireworks` and `fireworks_event_chain` with audio enabled.
Logs: `target/bevy-020-showcase-{build,tests,clippy}.log`.

## Reproduce

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc build -p aestra-bevy --example fireworks --features fireworks-audio --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --example fireworks --features fireworks-audio --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc clippy -p aestra-bevy --example fireworks --example fireworks_event_chain --features fireworks-audio --locked --offline -- -D warnings
$env:RUST_LOG = 'info'
$env:AESTRA_EXAMPLE_CAPTURE = Join-Path (Get-Location) 'target/showcase-recheck.png'
$env:AESTRA_EXAMPLE_CAPTURE_SECONDS = '9.3'
& ./target/debug/examples/fireworks.exe --smoke-art-show --tier high --history playback-only --no-audio
```

Omit `--smoke-art-show` for the default show. Omit `--no-audio` for native WAV
binding. Use 26/37 seconds respectively for the completed-show counters.

## Retained capture hashes (SHA-256)

```text
default-26s-audio.png 079AD6DCD3C89E91258A4458C2D8A3800BFF7E9C2B03514ED28DDBC50C3C300E
wispy-2-6s.png        E3228397BBC7FEFA6375A3769C7921F6A9782E98F2D9170772943D85C2443B46
wispy-9-3s.png        8249E5D033CB65FEBD3C20C5760BE33A747A647F1F03383624BC6E030E6E9BA5
wispy-10s.png         749C850F7FF7B95D1552E3537B62244BF953FCD6E1DCD428E9F053E59163A301
wispy-18-4s.png       7DF107963E1B346A28B97A68F48ED58641E844FD88EA4A50D090E033AE880AD3
wispy-30s.png         F8D1760F7AAD4AEC257D195623221B57B1FCB7D9189F6F6AC967A294AB7A2C34
wispy-37s.png         E06B39011D47894EDEC65A120054C27B7CFC8291C708A2D4C7E00DE4E767778A
```

Authored root/material hashes (source files remain unchanged):

```text
fireworks_show.aestra.ron                     DD2BC77CC19AAA84CFCDCCB24A5C1554B168F5EA664541EA4C28882E09A4C011
fireworks_show_wispy_smoke.aestra.ron         FC22EB440F76FC4030A6C67C5B9A71AB45CE5AAC08AA1538976C0F31F9E4979C
fireworks_wispy_smoke.aestra.material.ron     642E3704EBC4F37110E00A1146A1641B6CB53C3E37EB451C4CBB0C4EF99E1395
```
