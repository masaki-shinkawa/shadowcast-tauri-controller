param(
    [Parameter(Mandatory)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$EvidencePath,

    [Parameter(Mandatory)]
    [ValidateSet('A', 'B', 'X', 'Y', 'UP', 'DOWN', 'LEFT', 'RIGHT', 'L', 'R', 'ZL', 'ZR', 'PLUS', 'MINUS', 'L_STICK', 'R_STICK')]
    [string]$Button,

    [ValidateRange(1, 2000)]
    [int]$HoldMs = 200,

    [switch]$Approved
)

# This script is intentionally limited to one approved button input. It verifies
# the failed run, a fresh live frame, the controller identity, and Switch USB
# connectivity. A finally block attempts to return every input to neutral.
$ErrorActionPreference = 'Stop'
if (-not $Approved) {
    throw 'Recovery Trial requires the user approval recorded by passing -Approved.'
}

$neutral = '000000000880000880'
$states = @{
    A       = '080000000880000880'
    B       = '040000000880000880'
    X       = '020000000880000880'
    Y       = '010000000880000880'
    UP      = '000002000880000880'
    DOWN    = '000001000880000880'
    LEFT    = '000008000880000880'
    RIGHT   = '000004000880000880'
    L       = '000040000880000880'
    R       = '400000000880000880'
    ZL      = '000080000880000880'
    ZR      = '800000000880000880'
    PLUS    = '000200000880000880'
    MINUS   = '000100000880000880'
    L_STICK = '000800000880000880'
    R_STICK = '000400000880000880'
}

$resolvedEvidence = (Resolve-Path -LiteralPath $EvidencePath).Path
$evidence = Get-Content -LiteralPath $resolvedEvidence -Raw -Encoding utf8 | ConvertFrom-Json
$runDirectory = (Resolve-Path -LiteralPath ([string]$evidence.runDirectory)).Path
$manifestPath = Join-Path $runDirectory 'manifest.json'
$eventsPath = Join-Path $runDirectory 'events.jsonl'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
if ($manifest.state -ne 'error') {
    throw "Recovery Trial is allowed only for an error run; found '$($manifest.state)'."
}
if ([string]$manifest.runId -cne [string]$evidence.runId) {
    throw 'Evidence and run manifest refer to different run ids.'
}

$diagnosticsRoot = Split-Path -Parent $runDirectory
$resolvedRoot = (Resolve-Path -LiteralPath $diagnosticsRoot).Path
if (-not $runDirectory.StartsWith($resolvedRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Run directory is outside the diagnostics root.'
}

$liveDirectory = if ($evidence.liveDirectory) { [string]$evidence.liveDirectory } else { Join-Path $resolvedRoot 'live' }
$liveStatePath = Join-Path $liveDirectory 'state.json'
$liveImagePath = Join-Path $liveDirectory 'latest.jpg'
if (-not (Test-Path -LiteralPath $liveStatePath -PathType Leaf) -or -not (Test-Path -LiteralPath $liveImagePath -PathType Leaf)) {
    throw 'Live screenshot or scene state is unavailable. Keep capture and analysis running.'
}

$screenshotsDirectory = Join-Path $runDirectory 'screenshots'
if (-not (Test-Path -LiteralPath $screenshotsDirectory -PathType Container)) {
    New-Item -ItemType Directory -Path $screenshotsDirectory | Out-Null
}
$diagnosticsBytes = (Get-ChildItem -LiteralPath $resolvedRoot -File -Recurse | Measure-Object -Property Length -Sum).Sum
$liveImageBytes = (Get-Item -LiteralPath $liveImagePath).Length
if ([long]$diagnosticsBytes + (2 * [long]$liveImageBytes) -gt 500MB) {
    throw 'The 500 MiB diagnostics limit has no room for Recovery Trial evidence. No input was sent.'
}
$trialAtMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$beforeRelative = "screenshots/recovery-$trialAtMs-before.jpg"
$afterRelative = "screenshots/recovery-$trialAtMs-after.jpg"

function Save-LiveSnapshot([string]$Destination) {
    for ($attempt = 0; $attempt -lt 5; $attempt++) {
        try {
            $stateBefore = Get-Content -LiteralPath $liveStatePath -Raw -Encoding utf8 | ConvertFrom-Json
            $sourceImage = if ($stateBefore.imageFile) {
                Join-Path $liveDirectory ([string]$stateBefore.imageFile)
            }
            else {
                $liveImagePath
            }
            Copy-Item -LiteralPath $sourceImage -Destination $Destination -Force
            $stateAfter = Get-Content -LiteralPath $liveStatePath -Raw -Encoding utf8 | ConvertFrom-Json
            if ([long]$stateBefore.frameNumber -eq [long]$stateAfter.frameNumber) {
                return $stateAfter
            }
        }
        catch {
            if ($attempt -eq 4) { throw }
        }
        Start-Sleep -Milliseconds 100
    }
    throw 'Could not capture a consistent live screenshot and scene state.'
}

$beforeState = Save-LiveSnapshot (Join-Path $runDirectory $beforeRelative)
$nowMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
if (($nowMs - [long]$beforeState.capturedAtMs) -gt 5000) {
    throw 'The live diagnostic frame is older than five seconds. No input was sent.'
}

$controllerPort = [System.IO.Ports.SerialPort]::new(
    [string]$manifest.controllerPort,
    460800,
    [System.IO.Ports.Parity]::None,
    8,
    [System.IO.Ports.StopBits]::One
)
$controllerPort.Handshake = [System.IO.Ports.Handshake]::None
$controllerPort.DtrEnable = $true
$controllerPort.RtsEnable = $false
$controllerPort.ReadTimeout = 50
$controllerPort.WriteTimeout = 1000
$controllerPort.NewLine = [string][char]10
$verified = $false
$inputSent = $false
$controllerError = $null
$neutralizationError = $null

function Send-Line([string]$Command) {
    $controllerPort.WriteLine($Command)
}

function Query-Controller([string]$Command, [int]$DurationMs = 500) {
    Send-Line $Command
    $reply = [System.Text.StringBuilder]::new()
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    while ($clock.ElapsedMilliseconds -lt $DurationMs) {
        Start-Sleep -Milliseconds 25
        [void]$reply.Append($controllerPort.ReadExisting())
    }
    return $reply.ToString()
}

try {
    $controllerPort.Open()
    Start-Sleep -Milliseconds 200
    $controllerPort.DiscardInBuffer()
    if ((Query-Controller '+ID ') -notmatch '(?m)^\+2wiCC\r?$') {
        throw 'Controller identity check failed. No input was sent.'
    }
    if ((Query-Controller '+GCS ') -notmatch '(?m)^\+GCS 1\r?$') {
        throw 'Controller USB is not connected. No input was sent.'
    }
    $verified = $true
    Send-Line '+SPM RT'
    Send-Line ('+QF ' + $neutral)
    Send-Line ('+QF ' + $states[$Button])
    $inputSent = $true
    Start-Sleep -Milliseconds $HoldMs
    Send-Line ('+QF ' + $neutral)
}
catch {
    if (-not $inputSent) { throw }
    $controllerError = $_.Exception.Message
}
finally {
    try {
        if ($controllerPort.IsOpen -and $verified) {
            Send-Line ('+QF ' + $neutral)
            Start-Sleep -Milliseconds 200
        }
    }
    catch {
        $neutralizationError = $_.Exception.Message
    }
    finally {
        if ($controllerPort.IsOpen) { $controllerPort.Close() }
        $controllerPort.Dispose()
    }
}

if (-not $inputSent) {
    throw 'Recovery Trial ended without sending an input.'
}

$afterState = $null
$afterCaptureError = $null
try {
    Start-Sleep -Milliseconds 1500
    $afterState = Save-LiveSnapshot (Join-Path $runDirectory $afterRelative)
    if ([long]$afterState.capturedAtMs -le [long]$beforeState.capturedAtMs) {
        throw 'No newer diagnostic frame became available.'
    }
}
catch {
    $afterCaptureError = $_.Exception.Message
}

$event = [ordered]@{
    sequence = $null
    atMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    type = 'recovery_trial'
    payload = [ordered]@{
        approvalRecorded = $true
        runId = [string]$manifest.runId
        button = $Button
        holdMs = $HoldMs
        controllerPort = [string]$manifest.controllerPort
        controllerError = $controllerError
        neutralizationError = $neutralizationError
        beforeSceneDetection = $beforeState.sceneDetection
        afterSceneDetection = if ($afterState) { $afterState.sceneDetection } else { $null }
        afterCaptureError = $afterCaptureError
    }
    screenshots = [ordered]@{
        before = $beforeRelative
        after = if ($afterState) { $afterRelative } else { $null }
    }
}
$json = $event | ConvertTo-Json -Depth 20 -Compress
$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::AppendAllText($eventsPath, $json + [Environment]::NewLine, $utf8NoBom)

if ($neutralizationError) {
    throw "The input was sent, but final neutralization could not be confirmed: $neutralizationError"
}
if ($controllerError) {
    throw "The input may have been partially sent; controller error: $controllerError"
}
if ($afterCaptureError) {
    throw "The input was sent and neutralized, but post-input evidence failed: $afterCaptureError"
}

[pscustomobject]@{
    RunId = [string]$manifest.runId
    Button = $Button
    HoldMs = $HoldMs
    BeforeSceneId = [string]$beforeState.sceneDetection.sceneId
    AfterSceneId = [string]$afterState.sceneDetection.sceneId
    BeforeScreenshot = (Join-Path $runDirectory $beforeRelative)
    AfterScreenshot = (Join-Path $runDirectory $afterRelative)
    Neutralized = $true
} | ConvertTo-Json -Compress
