#requires -Version 7.0
# Read-only F7E2 registration gate. Measurement success is NOT visual acceptance.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReportsDirectory, [switch]$MeasureOnly)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
function Distribution($Values) {
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) { throw 'Missing observations' }
    foreach ($v in $sorted) { if (![double]::IsFinite($v) -or $v -lt 0) { throw 'Invalid observation' } }
    [ordered]@{ samples = $sorted.Count; mean = ($sorted | Measure-Object -Average).Average
        p50 = $sorted[[Math]::Ceiling($sorted.Count*0.50)-1]
        p95 = $sorted[[Math]::Ceiling($sorted.Count*0.95)-1]; max = $sorted[-1] }
}
function Number($Row, [string]$Name) {
    $v = [double]::Parse($Row.$Name, [Globalization.CultureInfo]::InvariantCulture)
    if (![double]::IsFinite($v)) { throw "Invalid $Name" }
    $v
}
function Hash([string]$Name) {
    (Get-FileHash -LiteralPath (Join-Path $ReportsDirectory $Name) -Algorithm SHA256).Hash.ToLowerInvariant()
}
$registration = @(Import-Csv -LiteralPath (Join-Path $ReportsDirectory 'registration.csv'))
$telemetry = @(Import-Csv -LiteralPath (Join-Path $ReportsDirectory 'telemetry.csv'))
$metadata = [ordered]@{}
Import-Csv -LiteralPath (Join-Path $ReportsDirectory 'metadata.csv') | ForEach-Object { $metadata[$_.key] = $_.value }
if ($metadata.pipeline -ne 'pipelined' -or $metadata.history -ne 'playback-only' -or $metadata.cadence_hz -ne '60' -or
    $metadata.target -ne '768x384' -or $metadata.readback_cap -ne '1' -or $metadata.slots -ne '3' -or
    $metadata.light_range -ne '8' -or $metadata.backend -notin @('Vulkan','Dx12','Metal')) { throw 'Invalid native probe setup' }
if ($registration.Count -ne 60 -or $telemetry.Count -ne 360) { throw 'Missing samples' }
$runs = @()
$accepted = $true
foreach ($speed in @(25,75,150)) {
    $images = @($registration | Where-Object { (Number $_ speed_m_s) -eq $speed })
    $control = @($images | Where-Object mode -eq control)
    $moving = @($images | Where-Object mode -eq async)
    foreach ($group in @($control, $moving)) {
        if ($group.Count -ne 10 -or (@($group.request_tick | Sort-Object -Unique).Count -ne 10) -or
            (($group.request_tick | ForEach-Object { [int]$_ } | Sort-Object) -join ',') -ne '12,16,20,24,28,32,36,40,44,48') { throw 'Invalid image sample identities' }
        foreach ($image in $group) {
            $star = Number $image star_x_px
            $receiver = Number $image receiver_x_px
            $offset = Number $image signed_offset_px
            $frames = Number $image equivalent_frames
            if ($star -lt 100 -or $star -gt 650 -or $receiver -lt 100 -or $receiver -gt 650 -or
                [Math]::Abs($offset - ($star-$receiver)) -gt 0.0001 -or
                [Math]::Abs($frames - $offset/(768.0/1.7/60)) -gt 0.0001) { throw 'Invalid registration measurement' }
        }
        $orderedImages = @($group | Sort-Object { [int]$_.request_tick })
        for ($index = 1; $index -lt $orderedImages.Count; $index++) {
            $travel = (Number $orderedImages[$index] star_x_px) - (Number $orderedImages[$index-1] star_x_px)
            if ($travel -lt 20 -or $travel -gt 40) { throw 'Frozen/incorrectly paced GPU sprite images' }
        }
    }
    $baseline = ($control | ForEach-Object { Number $_ signed_offset_px } | Measure-Object -Average).Average
    $controlError = Distribution @($control | ForEach-Object { [Math]::Abs((Number $_ signed_offset_px) - $baseline) })
    if ([Math]::Abs($baseline) + $controlError.max -gt 1) { throw 'Control light and GPU sprite are not registered within one pixel' }
    $lagFrames = Distribution @($moving | ForEach-Object { [Math]::Abs(((Number $_ signed_offset_px)-$baseline)/(768.0/1.7/60)) })
    $lagMetres = Distribution @($moving | ForEach-Object { [Math]::Abs(((Number $_ signed_offset_px)-$baseline)/(768.0/($speed*1.7))) })
    # Explicit initial flagship registration budget: p95 <= one 60Hz frame AND
    # <= one-quarter of the authored light range. Not a universal perceptual law.
    $limitMetres = [Math]::Min($speed/60.0, 8.0/4)
    $passes = $lagFrames.p95 -le 1 -and $lagMetres.p95 -le $limitMetres
    $accepted = $accepted -and $passes
    $phases = @()
    foreach ($mode in @('control','async')) {
        $samples = @($telemetry | Where-Object { (Number $_ speed_m_s) -eq $speed -and $_.mode -eq $mode })
        if ($samples.Count -ne 60 -or (@($samples.tick | Sort-Object -Unique).Count -ne 60)) { throw 'Missing paced telemetry' }
        foreach ($s in $samples) {
            foreach ($name in @('tick','interval_ms','update_ms','active','allocated','sequence','frame_lag','accepted_age_ms','pending','staging_bytes','failed','expired')) {
                if ((Number $s $name) -lt 0) { throw 'Negative telemetry value' }
            }
            if ((Number $s tick) -lt 1 -or (Number $s tick) -gt 60) { throw 'Invalid moving tick' }
            if ((Number $s allocated) -gt 1 -or (Number $s active) -gt (Number $s allocated) -or
                (Number $s pending) -gt 3 -or (Number $s staging_bytes) -gt 192 -or (Number $s failed) -ne 0 -or
                (Number $s expired) -ne 0 -or (Number $s frame_lag) -gt 8 -or (Number $s accepted_age_ms) -gt 100 -or
                ($mode -eq 'async' -and (Number $s active) -ne 1) -or
                ($mode -eq 'control' -and ((Number $s allocated) -ne 0 -or (Number $s staging_bytes) -ne 0))) { throw 'Resource/liveness violation' }
        }
        # First interval starts after warmup, so exclude it from cadence assessment.
        $interval = Distribution @($samples | Where-Object { [int]$_.tick -gt 1 } | ForEach-Object { Number $_ interval_ms })
        if ($interval.mean -lt 15 -or $interval.mean -gt 18.5 -or $interval.p95 -gt 22) { throw 'Not real-time 60Hz cadence; rerun without competing GPU/build load' }
        $phases += [ordered]@{ mode = $mode; interval_ms = $interval
            update_ms = Distribution @($samples | ForEach-Object { Number $_ update_ms })
            max_frame_lag = if ($mode -eq 'async') { ($samples | ForEach-Object { Number $_ frame_lag } | Measure-Object -Maximum).Maximum } else { $null }
            last_accepted_age_ms = if ($mode -eq 'async') { Distribution @($samples | ForEach-Object { Number $_ accepted_age_ms }) } else { $null } }
    }
    $runs += [ordered]@{ speed_m_s = $speed; control_error_px = $controlError; lag_frames = $lagFrames
        lag_ms_at_60hz = $lagFrames.p95*1000/60; lag_metres = $lagMetres
        p95_limit_metres = $limitMetres; accepted = $passes; phases = $phases
        control_image_sha256 = Hash "$speed-control-28.png"; async_image_sha256 = Hash "$speed-async-28.png" }
}
[ordered]@{ schema = 1; date = '2026-10-04'; scope = 'F7E2 isolated paced final-image registration, three speeds, ten paired image observations per phase. Screenshot request ticks are labels, not callback/display timestamps. No monitor scanout measurement or full-show load certification.'
    metadata = $metadata; accepted = $accepted; runs = $runs
    registration_sha256 = Hash 'registration.csv'; telemetry_sha256 = Hash 'telemetry.csv'; metadata_sha256 = Hash 'metadata.csv' } | ConvertTo-Json -Depth 9
if (!$accepted -and !$MeasureOnly) { throw 'F7E2 flagship fast-star registration budget FAILED; measurements are valid but async realization is not approved' }
