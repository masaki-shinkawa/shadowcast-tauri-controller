param(
    [Parameter(Mandatory)][ValidateSet('Left', 'Right')][string]$Stick,
    [ValidatePattern('^COM\d+$')][string]$Port = 'COM3',
    [ValidateRange(200, 5000)][int]$DirectionHoldMs = 1000,
    [ValidateRange(200, 5000)][int]$CenterHoldMs = 1000,
    [switch]$ScreenReady,
    [switch]$DryRun
)

# Use only on the Switch stick-selection/check screen, without starting calibration.
# No buttons are sent. A finally block attempts neutralization even after a failure.
# This does not provide protection against a disconnected cable or a killed process.
$ErrorActionPreference = 'Stop'
$neutral = '000000000880000880'
function Pack-Stick([int]$x, [int]$y) {
    if ($x -lt 0x200 -or $x -gt 0xE00 -or $y -lt 0x200 -or $y -gt 0xE00) {
        throw 'Stick coordinate outside the intended test range.'
    }
    return '{0:X2}{1:X2}{2:X2}' -f ($x -band 255), (($x -shr 8) -bor (($y -band 15) -shl 4)), ($y -shr 4)
}
function Make-State([int]$x, [int]$y) {
    $packed = Pack-Stick $x $y
    if ($Stick -eq 'Left') { return '000000' + $packed + '000880' }
    return '000000000880' + $packed
}
$motions = @(
    @{Name='Select stick: right'; X=0xE00; Y=0x800; HoldMs=2000; NeutralMs=2000},
    @{Name='Right'; X=0xE00; Y=0x800; HoldMs=$DirectionHoldMs; NeutralMs=$CenterHoldMs},
    @{Name='Up'; X=0x800; Y=0xE00; HoldMs=$DirectionHoldMs; NeutralMs=$CenterHoldMs},
    @{Name='Left'; X=0x200; Y=0x800; HoldMs=$DirectionHoldMs; NeutralMs=$CenterHoldMs},
    @{Name='Down'; X=0x800; Y=0x200; HoldMs=$DirectionHoldMs; NeutralMs=$CenterHoldMs}
)
$sequence = @($motions | ForEach-Object {
    [pscustomobject]@{Name=$_.Name; State=(Make-State $_.X $_.Y); HoldMs=$_.HoldMs; NeutralMs=$_.NeutralMs}
})
if ((Make-State 0x800 0x800) -cne $neutral) { throw 'Neutral encoding check failed.' }
if ($DryRun) { $sequence | ConvertTo-Json; return }
if (-not $ScreenReady) { throw 'First open the stick-selection/check screen, then use -ScreenReady.' }

$controllerPort = [System.IO.Ports.SerialPort]::new($Port, 460800, [System.IO.Ports.Parity]::None, 8, [System.IO.Ports.StopBits]::One)
$controllerPort.Handshake = [System.IO.Ports.Handshake]::None
$controllerPort.DtrEnable = $true
$controllerPort.RtsEnable = $false
$controllerPort.ReadTimeout = 500
$controllerPort.WriteTimeout = 1000
$controllerPort.NewLine = [string][char]10
$trace = [System.Collections.Generic.List[object]]::new()
$verified = $false
$recordingMayBeOn = $false
function Query-Controller([string]$command, [int]$durationMs=400) {
    $controllerPort.WriteLine($command)
    $reply = [System.Text.StringBuilder]::new()
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    while ($clock.ElapsedMilliseconds -lt $durationMs) {
        Start-Sleep -Milliseconds 25
        [void]$reply.Append($controllerPort.ReadExisting())
    }
    $text = $reply.ToString()
    $trace.Add([pscustomobject]@{Command=$command; Reply=$text})
    return $text
}
try {
    $controllerPort.Open()
    Start-Sleep -Milliseconds 200
    $controllerPort.DiscardInBuffer()
    if ((Query-Controller '+ID ') -notmatch '(?m)^\+2wiCC\r?$') { throw 'Controller identity check failed.' }
    $verified = $true
    if ((Query-Controller '+GCS ') -notmatch '(?m)^\+GCS 1\r?$') { throw 'Controller USB is not connected.' }
    $controllerPort.WriteLine('+SPM RT')
    $controllerPort.WriteLine('+QF ' + $neutral)
    Start-Sleep -Milliseconds 200
    $recordingMayBeOn = $true
    if ((Query-Controller '+REC 1') -notmatch '(?m)^\+REC 1\r?$') { throw 'Recording did not start.' }
    foreach ($motion in $sequence) {
        $controllerPort.WriteLine('+QF ' + $motion.State)
        Start-Sleep -Milliseconds $motion.HoldMs
        $controllerPort.WriteLine('+QF ' + $neutral)
        Start-Sleep -Milliseconds $motion.NeutralMs
        Write-Output ($Stick + ': ' + $motion.Name + ', then centered.')
    }
    if ((Query-Controller '+REC 0') -notmatch '(?m)^\+REC 0\r?$') { throw 'Recording stop was not confirmed.' }
    $recordingMayBeOn = $false
    $recording = [System.Text.StringBuilder]::new()
    $complete = $false
    for ($page=0; $page -lt 10; $page++) {
        $command = if ($page -eq 0) { '+GR 0' } else { '+GR 1' }
        $reply = Query-Controller $command 500
        [void]$recording.Append($reply)
        if ($reply -match '(?m)^\+GR 0\r?$') { $complete=$true; break }
        if ($reply -notmatch '(?m)^\+GR 1\r?$') { break }
    }
    $entryMatches = [regex]::Matches($recording.ToString(), '(?im)^\+R ([0-9A-F]{18})x([0-9A-F]{2})\r?$')
    $entries = @($entryMatches | ForEach-Object {
        [pscustomobject]@{State=$_.Groups[1].Value.ToUpperInvariant(); Frames=[Convert]::ToInt32($_.Groups[2].Value,16)}
    })
    $transitions = [System.Collections.Generic.List[string]]::new()
    foreach ($entry in $entries) {
        if ($transitions.Count -eq 0 -or $transitions[$transitions.Count-1] -ne $entry.State) { $transitions.Add($entry.State) }
    }
    $expected = [System.Collections.Generic.List[string]]::new()
    $expected.Add($neutral)
    foreach ($motion in $sequence) { $expected.Add($motion.State); $expected.Add($neutral) }
    $match = $complete -and (($transitions -join ',') -ceq ($expected -join ','))
    $docsDirectory = Join-Path (Split-Path -Parent $PSScriptRoot) 'docs'
    $auditPath = Join-Path $docsDirectory ('controller-stick-' + $Stick.ToLowerInvariant() + '-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.json')
    [pscustomobject]@{
        Timestamp=(Get-Date -Format o); Port=$Port; Stick=$Stick; Sequence=$sequence
        FirmwareRecordingComplete=$complete; ExactTransitionMatch=$match
        ExpectedTransitions=$expected.ToArray(); RecordedEntries=$entries
        RecordedTransitions=$transitions.ToArray(); RawRecording=$recording.ToString()
        Trace=$trace.ToArray(); ConsoleVisualConfirmation='pending'
    } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $auditPath -Encoding utf8
    [pscustomobject]@{AuditFile=$auditPath; Stick=$Stick; RecordingEntries=$entries.Count; ExactTransitionMatch=$match; FinalRecordedState=($transitions | Select-Object -Last 1)} | ConvertTo-Json -Compress
    if (-not $match) { throw 'Recorded state transitions did not match the test sequence; inspect the audit file.' }
} finally {
    try {
        if ($controllerPort.IsOpen -and $verified) {
            $controllerPort.WriteLine('+QF ' + $neutral)
            if ($recordingMayBeOn) { $controllerPort.WriteLine('+REC 0') }
            Start-Sleep -Milliseconds 200
            Write-Output 'Final neutral command sent.'
        }
    } finally {
        if ($controllerPort.IsOpen) { $controllerPort.Close() }
        $controllerPort.Dispose()
    }
}
