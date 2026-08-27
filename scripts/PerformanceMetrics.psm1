function Assert-MeasurementWindow {
    param(
        [int]$DurationMinutes,
        [int]$IntervalSeconds
    )

    $durationSeconds = $DurationMinutes * 60
    if ($durationSeconds -le $IntervalSeconds) {
        throw "The measurement duration must exceed the sample interval so at least one measured CPU sample can be recorded."
    }
}

function Assert-MeasuredSampleCount {
    param([int]$SampleCount)

    if ($SampleCount -lt 2) {
        throw "The measurement completed without a CPU delta sample; no summary was written."
    }
}

function Get-CpuPercentOneCore {
    param(
        [System.Collections.IDictionary]$CurrentCpuSecondsById,
        [AllowNull()]
        [System.Collections.IDictionary]$PreviousCpuSecondsById,
        [double]$ElapsedSeconds
    )

    if ($null -eq $PreviousCpuSecondsById -or $ElapsedSeconds -le 0) {
        return 0.0
    }

    $cpuSecondsDelta = 0.0
    foreach ($processId in $CurrentCpuSecondsById.Keys) {
        if (-not $PreviousCpuSecondsById.Contains($processId)) {
            continue
        }

        $processDelta = [double]$CurrentCpuSecondsById[$processId] -
            [double]$PreviousCpuSecondsById[$processId]
        if ($processDelta -gt 0) {
            $cpuSecondsDelta += $processDelta
        }
    }

    return ($cpuSecondsDelta / $ElapsedSeconds) * 100.0
}

Export-ModuleMember `
    -Function Assert-MeasurementWindow, Assert-MeasuredSampleCount, Get-CpuPercentOneCore
