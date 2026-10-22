param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$Output,
    [ValidateRange(10,120)][int]$Seconds = 30,
    [switch]$Preview
)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $Executable).Path
if ([IO.Path]::GetFileName($exe) -notin @('penguin-pal.exe','PenguinPal-Companion.exe')) { throw 'Expected the Companion executable.' }
$already = Get-Process -Name 'penguin-pal','PenguinPal-Companion' -ErrorAction SilentlyContinue
if ($already -and -not $Preview) { throw 'Quit the existing pet or use -Preview for a separate test state.' }
$priorSmoke = $env:PENGUINPAL_SMOKE_SECONDS
try {
    $env:PENGUINPAL_SMOKE_SECONDS = [string]($Seconds + 15)
    $launchArgs = if ($Preview) { @('--smoke','--smoke-preview') } else { @('--smoke') }
    $app = Start-Process -FilePath $exe -ArgumentList $launchArgs -WindowStyle Hidden -PassThru
} finally { $env:PENGUINPAL_SMOKE_SECONDS = $priorSmoke }
Start-Sleep -Seconds 5
if ($app.HasExited) { throw 'The pet exited before measurement.' }
$singleInstance = $null
if (-not $Preview) {
    $second = Start-Process -FilePath $exe -ArgumentList '--smoke' -WindowStyle Hidden -PassThru
    $singleInstance = $second.WaitForExit(5000) -and $second.ExitCode -eq 0
}
$samples = @()
for ($index = 0; $index -le $Seconds; $index++) {
    $inventory = @(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name)
    $ids = @([int]$app.Id)
    do {
        $children = @($inventory | Where-Object { [int]$_.ParentProcessId -in $ids -and [int]$_.ProcessId -notin $ids } | ForEach-Object { [int]$_.ProcessId })
        $ids += $children
    } while ($children.Count)
    $processes = @(Get-Process -Id $ids -ErrorAction SilentlyContinue)
    if (-not ($processes | Where-Object Id -eq $app.Id)) { throw 'Pet stopped during measurement.' }
    $samples += [pscustomobject]@{
        at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        processes = @($processes | ForEach-Object { [pscustomobject]@{
            id=$_.Id;name=$_.ProcessName;cpuSeconds=$_.TotalProcessorTime.TotalSeconds
            workingSetBytes=$_.WorkingSet64;privateBytes=$_.PrivateMemorySize64
        } })
    }
    if ($index -lt $Seconds) { Start-Sleep -Seconds 1 }
}
$first = $samples[0]; $last = $samples[-1]
$cpu = 0.0
foreach ($process in $last.processes) {
    $baseline = $first.processes | Where-Object id -eq $process.id | Select-Object -First 1
    if ($baseline) { $cpu += [Math]::Max(0.0,$process.cpuSeconds - $baseline.cpuSeconds) }
}
$wall = ($last.at - $first.at)/1000
$working = @($samples | ForEach-Object { ($_.processes | Measure-Object workingSetBytes -Sum).Sum })
$private = @($samples | ForEach-Object { ($_.processes | Measure-Object privateBytes -Sum).Sum })
$result = [pscustomobject]@{
    executable=$exe;sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
    sampledSeconds=$wall;logicalProcessors=[Environment]::ProcessorCount;singleInstancePassed=$singleInstance
    cpuPercentMachine=100*$cpu/$wall/[Environment]::ProcessorCount
    meanWorkingSetMiB=($working | Measure-Object -Average).Average/1MB
    peakWorkingSetMiB=($working | Measure-Object -Maximum).Maximum/1MB
    meanPrivateMiB=($private | Measure-Object -Average).Average/1MB
    peakPrivateMiB=($private | Measure-Object -Maximum).Maximum/1MB
    mode=$(if ($Preview) {'Isolated greeting video and open panel; fresh pet preferences; normal singleton check skipped.'} else {'Visible pet, current preferences.'})
    note='Includes descendant WebView2 processes. Working-set sum can double-count shared pages. Processes that exited between snapshots are omitted from CPU delta.'
    samples=$samples
}
$target = [IO.Path]::GetFullPath($Output)
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
$cleanExit = $app.WaitForExit(30000) -and $app.ExitCode -eq 0
$result | Add-Member -NotePropertyName cleanExit -NotePropertyValue $cleanExit
if ($Preview) {
    $probePath = Join-Path $env:LOCALAPPDATA "com.penguinpal.app/companion/smoke-preview-$($app.Id)/preview.json"
    $probe = if (Test-Path -LiteralPath $probePath) { Get-Content -LiteralPath $probePath -Raw | ConvertFrom-Json } else { $null }
    $maxProcesses = ($samples | ForEach-Object { $_.processes.Count } | Measure-Object -Maximum).Maximum
    $valid = $null -ne $probe -and $probe.width -eq 256 -and $probe.height -eq 256 -and $probe.corner -eq 0 -and $probe.center -ge 250 -and $probe.panel -and $maxProcesses -ge 3 -and $cleanExit
    $result | Add-Member -NotePropertyName nativePreviewProbe -NotePropertyValue $probe
    $result | Add-Member -NotePropertyName previewCoveragePassed -NotePropertyValue $valid
    $result | Add-Member -NotePropertyName maxProcesses -NotePropertyValue $maxProcesses
}
$result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $target -Encoding UTF8
$result | Select-Object -ExcludeProperty samples | ConvertTo-Json
if ($Preview -and -not $result.previewCoveragePassed) { throw 'Preview coverage incomplete: require native video alpha proof and independent WebView2 processes.' }
