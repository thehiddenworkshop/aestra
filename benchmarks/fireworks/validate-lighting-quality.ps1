#requires -Version 7.0
# Read-only F7F policy/public reuse gate layered on unchanged matched-cost gates.
[CmdletBinding()]
param([Parameter(Mandatory,ParameterSetName='Reports')][string]$ReportsDirectory,
    [Parameter(ParameterSetName='Reports')][ValidateSet('f7f2_low_output24_global24')][string]$FixtureProfile,
    [Parameter(Mandatory,ParameterSetName='SelfTest')][switch]$SelfTest)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
function Policy($Report, [string]$Tier, [string]$Probe) {
    $caps = switch ($Tier) {high {@(8,96)}; medium {@(4,48)}; low {@(2,24)}; default {throw 'Unknown tier'} }
    $p = $Report.presentation.lighting_policy
    if ($null -eq $p -or $p.representative_enabled -ne ($Probe -eq 'show') -or
        $p.particle_enabled -ne $true -or $p.representative_cap -ne $caps[0] -or $p.particle_cap -ne $caps[1] -or
        $p.representative_max_lumens -ne 1000000 -or $p.particle_max_lumens -ne 1000000 -or
        $p.representative_max_range -ne 200 -or $p.particle_max_range -ne 200 -or $p.shadows -ne $false) {
        throw 'Incorrect/missing requested lighting policy'
    }
}
function StableIdentities($Report) {
    $measured = @($Report.cluster_buffers | Where-Object { $_.tick.measured })
    if ($measured.Count -lt $Report.frames-4) { throw 'Missing public identity observations' }
    foreach ($field in @('index_buffer_fingerprint','offsets_counts_buffer_fingerprint')) {
        $ids = @($measured | ForEach-Object { $_.views[0].$field })
        if (@($ids | Where-Object { $null -eq $_ -or $_ -notmatch '^[0-9a-f]{16}$' }).Count -ne 0 -or
            @($ids | Sort-Object -Unique).Count -ne 1) { throw 'Public list identity unavailable or changed' }
    }
}
function FixtureProfileGate($Report, [string]$Expected) {
    $field = $Report.presentation.PSObject.Properties['particle_light_fixture_profile']
    if ($null -eq $field -or $field.Value -ne $Expected) {throw 'Missing/incorrect authored fixture profile'}
}
if ($SelfTest) {
    function Fixture {
        [pscustomobject]@{presentation=[pscustomobject]@{particle_light_fixture_profile='f7f2_low_output24_global24'; lighting_policy=[pscustomobject]@{
            representative_enabled=$true; representative_cap=4; particle_enabled=$true; particle_cap=48
            representative_max_lumens=1000000; particle_max_lumens=1000000
            representative_max_range=200; particle_max_range=200; shadows=$false}}
            frames=5; cluster_buffers=@([pscustomobject]@{tick=[pscustomobject]@{measured=$true}
                views=@([pscustomobject]@{index_buffer_fingerprint='0000000000000001'; offsets_counts_buffer_fingerprint='0000000000000002'})})}
    }
    Policy (Fixture) medium show
    StableIdentities (Fixture)
    FixtureProfileGate (Fixture) 'f7f2_low_output24_global24'
    foreach ($failure in @('missing','particle-cap','representative-cap','disabled','clamp','shadow','unknown-id','replacement','fixture-profile','missing-profile')) {
        $f = Fixture
        switch ($failure) {
            missing {$f.presentation.lighting_policy=$null}
            particle-cap {$f.presentation.lighting_policy.particle_cap=96}
            representative-cap {$f.presentation.lighting_policy.representative_cap=8}
            disabled {$f.presentation.lighting_policy.particle_enabled=$false}
            clamp {$f.presentation.lighting_policy.particle_max_range=400}
            shadow {$f.presentation.lighting_policy.shadows=$true}
            fixture-profile {$f.presentation.particle_light_fixture_profile='old_low_output8'}
            missing-profile {$f.presentation.PSObject.Properties.Remove('particle_light_fixture_profile')}
            unknown-id {$f.cluster_buffers[0].views[0].index_buffer_fingerprint=$null}
            replacement {
                $second=(Fixture).cluster_buffers[0]
                $second.views[0].index_buffer_fingerprint='0000000000000003'
                $f.cluster_buffers += $second
            }
        }
        $rejected=$false
        try {Policy $f medium show; StableIdentities $f; FixtureProfileGate $f 'f7f2_low_output24_global24'} catch {$rejected=$true}
        if (!$rejected) {throw "Negative control accepted: $failure"}
    }
    'Quality policy/identity positive and negative controls passed'
    return
}
$directory=(Resolve-Path -LiteralPath $ReportsDirectory).Path
$manifest=Get-Content -LiteralPath "$directory/manifest.json" -Raw | ConvertFrom-Json
if (!$manifest.accepted -or $manifest.allocator_snapshots) {throw 'Require accepted uninstrumented timing runs'}
$costs=& "$PSScriptRoot/validate-particle-light-costs.ps1" -ReportsDirectory $directory -RequireRetirement | ConvertFrom-Json
$rows=@()
foreach ($run in $manifest.runs) {
    $r=Get-Content -LiteralPath "$directory/$($run.pair)/$($run.stem).json" -Raw | ConvertFrom-Json
    Policy $r $run.tier $run.probe
    StableIdentities $r
    if ($FixtureProfile) {FixtureProfileGate $r $FixtureProfile}
    $rows += [ordered]@{pair=$run.pair; probe=$run.probe; tier=$run.tier; mode=$run.mode
        public_index_and_offsets_replacements=0; report_sha256=$run.report_sha256; log_sha256=$run.log_sha256}
}
[ordered]@{schema=1; milestone='F7F'; accepted=$true; binary_sha256=$manifest.binary_sha256; fixture_profile=$FixtureProfile
    scope='Three or more alternating run-level pairs at stated tiers; requested host policy and observed caps, matched authored work, native demand, unchanged public identities and final cleanup. Uninstrumented outer render-graph costs, not paired frames/whole-game/total VRAM/finale or image acceptance.'
    rows=$rows; costs=$costs.rows} | ConvertTo-Json -Depth 18
