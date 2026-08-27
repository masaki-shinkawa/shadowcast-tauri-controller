[CmdletBinding()]
param(
    [ValidateRange(1, 1440)]
    [int]$DurationMinutes = 30,

    [ValidateRange(1, 60)]
    [int]$IntervalSeconds = 1,

    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\artifacts\benchmarks"),

    [string]$ProcessName = "shadowcast-tauri-controller"
)

$ErrorActionPreference = "Stop"
$resolvedOutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $resolvedOutputDirectory -Force | Out-Null

$rootProcess = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
    Sort-Object StartTime -Descending |
    Select-Object -First 1

if (-not $rootProcess) {
    throw "Process '$ProcessName' was not found. Start the Tauri app and begin capture first."
}

function Get-ProcessTreeIds {
    param(
        [uint32]$RootProcessId,
        [object[]]$ProcessSnapshot
    )

    $knownIds = [System.Collections.Generic.HashSet[uint32]]::new()
    $pendingIds = [System.Collections.Generic.Queue[uint32]]::new()
    [void]$knownIds.Add($RootProcessId)
    $pendingIds.Enqueue($RootProcessId)

    while ($pendingIds.Count -gt 0) {
        $parentProcessId = $pendingIds.Dequeue()
        foreach ($childProcess in $ProcessSnapshot | Where-Object ParentProcessId -eq $parentProcessId) {
            $childProcessId = [uint32]$childProcess.ProcessId
            if ($knownIds.Add($childProcessId)) {
                $pendingIds.Enqueue($childProcessId)
            }
        }
    }

    return @($knownIds)
}

function Get-Percentile {
    param(
        [double[]]$Values,
        [double]$Percentile
    )

    if ($Values.Count -eq 0) {
        return 0
    }

    $sortedValues = @($Values | Sort-Object)
    $index = [Math]::Ceiling(($Percentile / 100) * $sortedValues.Count) - 1
    return $sortedValues[[Math]::Max(0, $index)]
}

$startedAt = Get-Date
$deadline = $startedAt.AddMinutes($DurationMinutes)
$samples = [System.Collections.Generic.List[object]]::new()
$previousCpuSeconds = $null
$previousSampleAt = $null

Write-Host "Measuring process $($rootProcess.Id) and its WebView2 children for $DurationMinutes minute(s)."

while ((Get-Date) -lt $deadline) {
    $sampledAt = Get-Date
    $processSnapshot = @(Get-CimInstance Win32_Process)
    $processIds = Get-ProcessTreeIds -RootProcessId ([uint32]$rootProcess.Id) -ProcessSnapshot $processSnapshot
    $processes = @(Get-Process -Id $processIds -ErrorAction SilentlyContinue)

    if (-not ($processes | Where-Object Id -eq $rootProcess.Id)) {
        throw "The Tauri process exited before the measurement completed."
    }

    $cpuSeconds = ($processes | Measure-Object CPU -Sum).Sum
    $workingSetBytes = ($processes | Measure-Object WorkingSet64 -Sum).Sum
    $privateBytes = ($processes | Measure-Object PrivateMemorySize64 -Sum).Sum
    $cpuPercent = 0.0

    if ($null -ne $previousCpuSeconds -and $null -ne $previousSampleAt) {
        $sampleSeconds = ($sampledAt - $previousSampleAt).TotalSeconds
        if ($sampleSeconds -gt 0) {
            $cpuPercent = (($cpuSeconds - $previousCpuSeconds) / $sampleSeconds) * 100
        }
    }

    $samples.Add([pscustomobject]@{
        timestamp = $sampledAt.ToString("o")
        elapsed_seconds = [Math]::Round(($sampledAt - $startedAt).TotalSeconds, 3)
        process_count = $processes.Count
        cpu_percent_one_core = [Math]::Round($cpuPercent, 3)
        cpu_percent_machine = [Math]::Round($cpuPercent / [Environment]::ProcessorCount, 3)
        working_set_mib = [Math]::Round($workingSetBytes / 1MB, 3)
        private_mib = [Math]::Round($privateBytes / 1MB, 3)
    })

    $previousCpuSeconds = $cpuSeconds
    $previousSampleAt = $sampledAt
    Start-Sleep -Seconds $IntervalSeconds
}

$timestamp = $startedAt.ToString("yyyyMMdd-HHmmss")
$csvPath = Join-Path $resolvedOutputDirectory "resources-$timestamp.csv"
$summaryPath = Join-Path $resolvedOutputDirectory "resources-$timestamp.summary.json"
$samples | Export-Csv -Path $csvPath -NoTypeInformation -Encoding utf8

$measuredSamples = @($samples | Select-Object -Skip 1)
$cpuValues = @($measuredSamples | ForEach-Object { [double]$_.cpu_percent_one_core })
$machineCpuValues = @($measuredSamples | ForEach-Object { [double]$_.cpu_percent_machine })
$workingSetValues = @($measuredSamples | ForEach-Object { [double]$_.working_set_mib })
$privateValues = @($measuredSamples | ForEach-Object { [double]$_.private_mib })
$firstMeasuredSample = $measuredSamples | Select-Object -First 1
$lastMeasuredSample = $measuredSamples | Select-Object -Last 1
$shadowCast = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue |
    Where-Object { $_.FriendlyName -match "ShadowCast|GENKI" } |
    Select-Object Status, Class, FriendlyName, InstanceId

$summary = [ordered]@{
    started_at = $startedAt.ToString("o")
    requested_duration_minutes = $DurationMinutes
    actual_duration_seconds = [Math]::Round(((Get-Date) - $startedAt).TotalSeconds, 3)
    interval_seconds = $IntervalSeconds
    root_process_id = $rootProcess.Id
    logical_processors = [Environment]::ProcessorCount
    cpu_percent_one_core = [ordered]@{
        average = [Math]::Round(($cpuValues | Measure-Object -Average).Average, 3)
        p95 = [Math]::Round((Get-Percentile -Values $cpuValues -Percentile 95), 3)
        maximum = [Math]::Round(($cpuValues | Measure-Object -Maximum).Maximum, 3)
    }
    cpu_percent_machine = [ordered]@{
        average = [Math]::Round(($machineCpuValues | Measure-Object -Average).Average, 3)
        p95 = [Math]::Round((Get-Percentile -Values $machineCpuValues -Percentile 95), 3)
        maximum = [Math]::Round(($machineCpuValues | Measure-Object -Maximum).Maximum, 3)
    }
    working_set_mib = [ordered]@{
        average = [Math]::Round(($workingSetValues | Measure-Object -Average).Average, 3)
        p95 = [Math]::Round((Get-Percentile -Values $workingSetValues -Percentile 95), 3)
        maximum = [Math]::Round(($workingSetValues | Measure-Object -Maximum).Maximum, 3)
        growth = [Math]::Round(
            [double]$lastMeasuredSample.working_set_mib - [double]$firstMeasuredSample.working_set_mib,
            3
        )
    }
    private_mib = [ordered]@{
        average = [Math]::Round(($privateValues | Measure-Object -Average).Average, 3)
        p95 = [Math]::Round((Get-Percentile -Values $privateValues -Percentile 95), 3)
        maximum = [Math]::Round(($privateValues | Measure-Object -Maximum).Maximum, 3)
        growth = [Math]::Round(
            [double]$lastMeasuredSample.private_mib - [double]$firstMeasuredSample.private_mib,
            3
        )
    }
    shadowcast_devices = @($shadowCast)
}

$summary | ConvertTo-Json -Depth 5 | Set-Content -Path $summaryPath -Encoding utf8
Write-Host "Samples: $csvPath"
Write-Host "Summary: $summaryPath"
