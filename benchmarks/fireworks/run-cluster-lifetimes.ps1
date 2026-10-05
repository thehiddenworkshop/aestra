#requires -Version 7.0
# Native dependency qualification only. Run alone; preserve every failed attempt.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReportsDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Test-Path -LiteralPath $ReportsDirectory) { throw 'Use a fresh reports directory' }
New-Item -ItemType Directory -Path $ReportsDirectory | Out-Null
$directory = (Resolve-Path -LiteralPath $ReportsDirectory).Path
$sourcePaths = @('Cargo.toml','Cargo.lock','vendor/bevy_pbr/Cargo.toml',
    'vendor/bevy_pbr/src/cluster/gpu.rs','vendor/bevy_pbr/src/cluster/mod.rs',
    'bevy/aestra-bevy/tests/cluster_buffer_reuse.rs','benchmarks/fireworks/run-cluster-lifetimes.ps1')
function Inputs {
    @($sourcePaths | ForEach-Object {
        [ordered]@{path=$_; sha256=(Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant()}
    })
}
$inputs = Inputs
$manifest = [ordered]@{schema=1; milestone='F7E4B3B2C'; accepted=$false; source_inputs=$inputs;
    source_inputs_unchanged=$false; runs=@(); failure=$null
    scope='Explicit native dependency tests, non-pipelined headless rendering. Private owning generation IDs and named backend live allocations at asynchronous submitted-work checkpoints. Not exact driver free times, total VRAM, arbitrary overload hard bounds, a natural-star image gate or general Aestra multi-view/layer certification.'}
function SaveManifest {
    $manifest | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath "$directory/manifest.json" -Encoding utf8NoBOM
}
SaveManifest
try {
    & cargo test --locked -p aestra-bevy --test cluster_buffer_reuse --no-run --message-format=json 2>&1 |
        Tee-Object -FilePath "$directory/build.log" | Out-Null
    $buildExit = $LASTEXITCODE
    if ($buildExit -ne 0) { throw "Native test build failed: $buildExit" }
    $artifacts = @(Get-Content -LiteralPath "$directory/build.log" | Where-Object { $_.StartsWith('{') } |
        ForEach-Object { $_ | ConvertFrom-Json } | Where-Object {
            $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'cluster_buffer_reuse' -and $null -ne $_.executable
        })
    if ($artifacts.Count -ne 1) { throw 'Native test executable not uniquely identified' }
    $binary = $artifacts[0].executable
    $manifest.binary_sha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    $tests = @('native_cluster_buffers_reuse_grow_reset_and_retire_per_view',
        'native_private_generations_overflow_recover_and_retire')
    foreach ($test in $tests) {
        & $binary $test --exact --ignored --nocapture --test-threads=1 2>&1 |
            Tee-Object -FilePath "$directory/$test.log" | Out-Null
        $nativeExit = $LASTEXITCODE
        $log = Get-Content -LiteralPath "$directory/$test.log" -Raw
        $run = [ordered]@{test=$test; exit_code=$nativeExit;
            log_sha256=(Get-FileHash -LiteralPath "$directory/$test.log" -Algorithm SHA256).Hash.ToLowerInvariant()
            passed=$nativeExit -eq 0 -and $log.Contains('test result: ok. 1 passed; 0 failed')}
        $manifest.runs += $run
        SaveManifest
        if (!$run.passed) { throw "Native test failed: $test exit=$nativeExit" }
        if ($log -match '\bERROR\b|panicked at|Validation Error') { throw "Unexpected native log failure: $test" }
        if ($test -like 'native_private*' -and
            (!$log.Contains('cluster_overflow recovered=true') -or
             !$log.Contains('cluster_private accepted=true windows=10') -or
             !$log.Contains('final_named_allocations=0'))) { throw 'Missing private overflow/retirement result' }
    }
    if ((ConvertTo-Json -InputObject $inputs -Depth 5 -Compress) -ne
        (ConvertTo-Json -InputObject (Inputs) -Depth 5 -Compress) -or
        (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $manifest.binary_sha256) {
        throw 'Sources or executable changed during native tests'
    }
    $manifest.source_inputs_unchanged = $true
    $manifest.accepted = $true
    SaveManifest
} catch {
    $manifest.failure = $_.Exception.Message
    SaveManifest
    throw
}
Write-Host "Accepted native lifetime/overflow qualification: $directory"
