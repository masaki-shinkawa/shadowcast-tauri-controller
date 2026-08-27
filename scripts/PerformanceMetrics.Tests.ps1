$ErrorActionPreference = "Stop"

Import-Module (Join-Path $PSScriptRoot "PerformanceMetrics.psm1") -Force

function Assert-Equal {
    param(
        [object]$Expected,
        [object]$Actual,
        [string]$Message
    )

    if ($Expected -ne $Actual) {
        throw "$Message Expected '$Expected', got '$Actual'."
    }
}

$stableCpu = Get-CpuPercentOneCore `
    -PreviousCpuSecondsById @{ 10 = 2.0; 20 = 5.0 } `
    -CurrentCpuSecondsById @{ 10 = 2.5; 20 = 5.5 } `
    -ElapsedSeconds 2
Assert-Equal 50 $stableCpu "Stable process CPU deltas should be summed."

$replacementCpu = Get-CpuPercentOneCore `
    -PreviousCpuSecondsById @{ 10 = 2.0; 20 = 50.0 } `
    -CurrentCpuSecondsById @{ 10 = 2.5; 30 = 40.0 } `
    -ElapsedSeconds 1
Assert-Equal 50 $replacementCpu "Exited and replacement processes must not affect the CPU delta."

$decreasedCpu = Get-CpuPercentOneCore `
    -PreviousCpuSecondsById @{ 10 = 3.0 } `
    -CurrentCpuSecondsById @{ 10 = 2.0 } `
    -ElapsedSeconds 1
Assert-Equal 0 $decreasedCpu "A decreasing cumulative counter must not create negative CPU usage."

Assert-MeasurementWindow -DurationMinutes 1 -IntervalSeconds 59

$invalidWindowRejected = $false
try {
    Assert-MeasurementWindow -DurationMinutes 1 -IntervalSeconds 60
} catch {
    $invalidWindowRejected = $true
}
Assert-Equal $true $invalidWindowRejected "A window with no measured sample must be rejected."

Assert-MeasuredSampleCount -SampleCount 2

$missingMeasuredSampleRejected = $false
try {
    Assert-MeasuredSampleCount -SampleCount 1
} catch {
    $missingMeasuredSampleRejected = $true
}
Assert-Equal $true $missingMeasuredSampleRejected "An empty measured sample set must be rejected."

Write-Host "Performance metric tests passed."
