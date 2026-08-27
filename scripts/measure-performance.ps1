[CmdletBinding()]
param(
    [ValidateRange(1, 1440)]
    [int]$DurationMinutes = 30,

    [ValidateRange(1, 60)]
    [int]$IntervalSeconds = 1,

    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\artifacts\benchmarks"),

    [string]$ProcessName = "shadowcast-tauri-controller",

    [switch]$IncludeUiTelemetry
)

$ErrorActionPreference = "Stop"
if ($IncludeUiTelemetry) {
    Add-Type -AssemblyName UIAutomationClient
}
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

function Get-AutomationNames {
    param([IntPtr]$WindowHandle)

    $rootElement = [System.Windows.Automation.AutomationElement]::FromHandle($WindowHandle)
    $elements = $rootElement.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition
    )
    $names = [System.Collections.Generic.List[string]]::new()
    for ($index = 0; $index -lt $elements.Count; $index++) {
        $name = $elements.Item($index).Current.Name
        if ($name) {
            $names.Add($name)
        }
    }
    return $names
}

function Get-TextAfterLabel {
    param(
        [System.Collections.Generic.List[string]]$Names,
        [string]$Label,
        [int]$Offset = 1
    )

    $labelIndex = $Names.LastIndexOf($Label)
    if ($labelIndex -lt 0 -or $labelIndex + $Offset -ge $Names.Count) {
        return $null
    }
    return $Names[$labelIndex + $Offset]
}

function Get-TextAfterPrefix {
    param(
        [System.Collections.Generic.List[string]]$Names,
        [string]$Prefix,
        [int]$Offset = 1
    )

    for ($index = $Names.Count - 1; $index -ge 0; $index--) {
        if ($Names[$index].StartsWith($Prefix) -and $index + $Offset -lt $Names.Count) {
            return $Names[$index + $Offset]
        }
    }
    return $null
}

function Convert-MetricValue {
    param(
        [AllowNull()]
        [string]$Text,
        [string]$Suffix = ""
    )

    if (-not $Text) {
        return 0.0
    }
    $numberText = $Text
    if ($Suffix) {
        $numberText = $numberText.Replace($Suffix, "")
    }
    $numberText = $numberText.Replace(",", "").Trim()
    $number = 0.0
    if ([double]::TryParse(
        $numberText,
        [System.Globalization.NumberStyles]::Float,
        [System.Globalization.CultureInfo]::InvariantCulture,
        [ref]$number
    )) {
        return $number
    }
    return 0.0
}

function Get-MetricStats {
    param([double[]]$Values)

    return [ordered]@{
        average = [Math]::Round(($Values | Measure-Object -Average).Average, 3)
        p95 = [Math]::Round((Get-Percentile -Values $Values -Percentile 95), 3)
        p05 = [Math]::Round((Get-Percentile -Values $Values -Percentile 5), 3)
        minimum = [Math]::Round(($Values | Measure-Object -Minimum).Minimum, 3)
        maximum = [Math]::Round(($Values | Measure-Object -Maximum).Maximum, 3)
    }
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
    $captureState = ""
    $captureFps = 0.0
    $renderedFps = 0.0
    $averageJpegKib = 0.0
    $channelMbps = 0.0
    $receiveToDrawMs = 0.0
    $droppedFrames = 0.0
    $frameCount = 0.0
    $channelSendMs = 0.0
    if ($IncludeUiTelemetry) {
        $automationNames = Get-AutomationNames -WindowHandle $rootProcess.MainWindowHandle
        $captureState = if ($automationNames.Contains("RUNNING")) {
            "running"
        } elseif ($automationNames.Contains("ERROR")) {
            "error"
        } else {
            "stopped"
        }
        $captureFps = Convert-MetricValue (Get-TextAfterLabel $automationNames "CAPTURE FPS")
        $renderedFps = Convert-MetricValue (Get-TextAfterLabel $automationNames "RENDERED FPS")
        $averageJpegKib = Convert-MetricValue (Get-TextAfterLabel $automationNames "JPEG AVERAGE") "KiB"
        $channelMbps = Convert-MetricValue (Get-TextAfterLabel $automationNames "CHANNEL") "Mb/s"
        $receiveToDrawMs = Convert-MetricValue (Get-TextAfterPrefix $automationNames "RECEIVE") "ms"
        $droppedFrames = Convert-MetricValue (Get-TextAfterLabel $automationNames "DROPPED / FRAMES")
        $frameCount = Convert-MetricValue (Get-TextAfterLabel $automationNames "DROPPED / FRAMES" 3)
        $channelSendMs = Convert-MetricValue (Get-TextAfterLabel $automationNames "SEND CALL") "ms"
    }

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
        capture_state = $captureState
        capture_fps = $captureFps
        rendered_fps = $renderedFps
        average_jpeg_kib = $averageJpegKib
        channel_mbps = $channelMbps
        receive_to_draw_ms = $receiveToDrawMs
        dropped_frames = [uint64]$droppedFrames
        frame_count = [uint64]$frameCount
        average_channel_send_ms = $channelSendMs
    })

    $previousCpuSeconds = $cpuSeconds
    $previousSampleAt = $sampledAt
    $remainingMilliseconds = [Math]::Round(
        [Math]::Max(0, ($IntervalSeconds - ((Get-Date) - $sampledAt).TotalSeconds) * 1000)
    )
    if ($remainingMilliseconds -gt 0) {
        Start-Sleep -Milliseconds $remainingMilliseconds
    }
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
$runningSamples = @($measuredSamples | Where-Object capture_state -eq "running")
$captureFpsValues = @($runningSamples | ForEach-Object { [double]$_.capture_fps })
$renderedFpsValues = @($runningSamples | ForEach-Object { [double]$_.rendered_fps })
$jpegKibValues = @($runningSamples | ForEach-Object { [double]$_.average_jpeg_kib })
$channelMbpsValues = @($runningSamples | ForEach-Object { [double]$_.channel_mbps })
$receiveToDrawValues = @($runningSamples | ForEach-Object { [double]$_.receive_to_draw_ms })
$channelSendValues = @($runningSamples | ForEach-Object { [double]$_.average_channel_send_ms })
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
    running_samples = $runningSamples.Count
    ui_telemetry_enabled = [bool]$IncludeUiTelemetry
    final_capture_state = if ($runningSamples.Count) { $lastMeasuredSample.capture_state } else { $null }
    final_frame_count = if ($runningSamples.Count) { [uint64]$lastMeasuredSample.frame_count } else { $null }
    final_dropped_frames = if ($runningSamples.Count) { [uint64]$lastMeasuredSample.dropped_frames } else { $null }
    capture_fps = if ($runningSamples.Count) { Get-MetricStats -Values $captureFpsValues } else { $null }
    rendered_fps = if ($runningSamples.Count) { Get-MetricStats -Values $renderedFpsValues } else { $null }
    average_jpeg_kib = if ($runningSamples.Count) { Get-MetricStats -Values $jpegKibValues } else { $null }
    channel_mbps = if ($runningSamples.Count) { Get-MetricStats -Values $channelMbpsValues } else { $null }
    receive_to_draw_ms = if ($runningSamples.Count) { Get-MetricStats -Values $receiveToDrawValues } else { $null }
    average_channel_send_ms = if ($runningSamples.Count) { Get-MetricStats -Values $channelSendValues } else { $null }
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
