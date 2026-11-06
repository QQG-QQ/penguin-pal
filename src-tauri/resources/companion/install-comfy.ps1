param([string]$Root = '', [string]$StatusFile = '')
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$utf8 = New-Object System.Text.UTF8Encoding($false)
$configDir = Join-Path $env:LOCALAPPDATA 'com.penguinpal.app'
if (!$StatusFile) { $StatusFile = Join-Path $configDir 'comfy-install-status.json' }
[IO.Directory]::CreateDirectory($configDir) | Out-Null
function Save-Json($Path, $Value) {
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path)) | Out-Null
    $tmp = "$Path.tmp"
    [IO.File]::WriteAllText($tmp, ($Value | ConvertTo-Json -Depth 8), $utf8)
    Move-Item -LiteralPath $tmp -Destination $Path -Force
}
function Status($State, $Message, $Progress) {
    Save-Json $StatusFile @{state=$State;message=$Message;progress=$Progress;root=$Root}
    Write-Output $Message
}
function Download-ModelParts($Url, $Part, $Bytes) {
    $parts = "$Part.chunks"
    [IO.Directory]::CreateDirectory($parts) | Out-Null
    $jobs = @()
    $files = @()
    $chunkBytes = 64MB
    try {
        for ($start = 0L; $start -lt $Bytes; $start += $chunkBytes) {
            $index = [int]($start / $chunkBytes)
            $end = [Math]::Min($Bytes - 1, $start + $chunkBytes - 1)
            $length = $end - $start + 1
            $chunk = Join-Path $parts ("{0:d3}.bin" -f $index)
            $files += $chunk
            if ((Test-Path -LiteralPath $chunk) -and (Get-Item -LiteralPath $chunk).Length -eq $length) { continue }
            $args = @('--fail','--silent','--show-error','--location','--proto','=https','--proto-redir','=https','--retry','1','--retry-delay','5','--connect-timeout','30','--max-time','1800','--speed-time','60','--speed-limit','1024','--max-filesize',"$length",'--range',"$start-$end",'--output',('"' + $chunk + '"'),('"' + $Url + "&pet_chunk=$index" + '"'))
            $process = Start-Process -FilePath 'curl.exe' -ArgumentList $args -WindowStyle Hidden -PassThru -RedirectStandardError "$chunk.log"
            $null = $process.Handle # Keep a handle before exit so Windows PowerShell retains ExitCode.
            $jobs += @{process=$process;file=$chunk;length=$length}
            while ($jobs.Count -ge 8) {
                $done = @($jobs | Where-Object {$_.process.HasExited})
                foreach ($job in $done) {
                    $job.process.WaitForExit()
                    if ($job.process.ExitCode -ne 0 -or (Get-Item -LiteralPath $job.file).Length -ne $job.length) { throw "Model chunk failed; partial chunks retained: $($job.file)" }
                }
                $jobs = @($jobs | Where-Object {$_ -notin $done})
                if ($jobs.Count -ge 8) { Start-Sleep -Milliseconds 250 }
            }
        }
        foreach ($job in $jobs) {
            $job.process.WaitForExit()
            if ($job.process.ExitCode -ne 0 -or (Get-Item -LiteralPath $job.file).Length -ne $job.length) { throw "Model chunk failed; partial chunks retained: $($job.file)" }
        }
        $stream = [IO.File]::Open($Part, 'Create', 'Write', 'None')
        try { foreach ($file in $files) { $input = [IO.File]::OpenRead($file); try { $input.CopyTo($stream) } finally { $input.Dispose() } } } finally { $stream.Dispose() }
    } finally {
        foreach ($job in $jobs) { if (!$job.process.HasExited) { $job.process.Kill(); $job.process.WaitForExit() } }
    }
}
function Verified-Download($Url, $Name, $Sha256, $Bytes = 0) {
    $file = Join-Path $cache $Name
    if ((Test-Path -LiteralPath $file) -and (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -eq $Sha256) { return $file }
    $part = "$file.part"
    if ($Bytes -gt 64MB) { Download-ModelParts $Url $part $Bytes } else {
        & curl.exe --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --retry 1 --retry-delay 5 --connect-timeout 30 --max-time 7200 --continue-at - --output $part $Url
        if ($LASTEXITCODE -ne 0) { throw "Download failed: $Name (curl $LASTEXITCODE); partial file retained." }
    }
    if ((Get-FileHash -LiteralPath $part -Algorithm SHA256).Hash -ne $Sha256) {
        $invalid = [Guid]::NewGuid()
        Move-Item -LiteralPath $part -Destination "$part.invalid-$invalid"
        if (Test-Path -LiteralPath "$part.chunks") { Move-Item -LiteralPath "$part.chunks" -Destination "$part.chunks.invalid-$invalid" }
        throw "SHA-256 verification failed: $Name"
    }
    Move-Item -LiteralPath $part -Destination $file -Force
    return $file
}
function Expand-Verified($Archive, $Destination) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $base = [IO.Path]::GetFullPath($Destination).TrimEnd('\') + '\'
    [IO.Directory]::CreateDirectory($base) | Out-Null
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        foreach ($entry in $zip.Entries) {
            $target = [IO.Path]::GetFullPath((Join-Path $base $entry.FullName))
            if (!$target.StartsWith($base, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid archive path.' }
            if (!$entry.Name) { [IO.Directory]::CreateDirectory($target) | Out-Null; continue }
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
        }
    } finally { $zip.Dispose() }
}
function Python($Arguments) {
    & $python @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Python step failed (exit $LASTEXITCODE); see installation log." }
}
$lock = $null
try {
    $pointer = Join-Path $configDir 'comfy-runtime.json'
    if (!$Root -and (Test-Path -LiteralPath $pointer)) { $Root = (Get-Content -LiteralPath $pointer -Raw | ConvertFrom-Json).root }
    if (!$Root) {
        $drive = Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | Where-Object {$_.FreeSpace -gt 12GB} | Sort-Object FreeSpace -Descending | Select-Object -First 1
        if (!$drive) { throw 'A local fixed disk with at least 12 GiB free is required.' }
        $Root = Join-Path ($drive.DeviceID + '\') 'PenguinPalRuntime\ComfyUI'
    }
    $Root = [IO.Path]::GetFullPath($Root)
    if ($Root.StartsWith('\\') -or $Root -notmatch '^[A-Za-z]:\\') { throw 'Runtime must use a local absolute path.' }
    [IO.Directory]::CreateDirectory($Root) | Out-Null
    $lock = [IO.File]::Open((Join-Path $Root 'install.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
    $ownerFile = Join-Path $Root '.penguinpal-runtime.json'
    $manifestFile = Join-Path $Root 'runtime.json'
    $owner = if (Test-Path -LiteralPath $ownerFile) { Get-Content -LiteralPath $ownerFile -Raw | ConvertFrom-Json } elseif (Test-Path -LiteralPath $manifestFile) { Get-Content -LiteralPath $manifestFile -Raw | ConvertFrom-Json } else { $null }
    if ($owner) {
        if ($owner.schema -ne 1 -or $owner.provider -ne 'penguinpal-comfyui' -or [IO.Path]::GetFullPath($owner.root) -ne $Root) { throw 'Directory is not owned by the pet runtime.' }
    } elseif (@(Get-ChildItem -LiteralPath $Root -Force | Where-Object {$_.Name -ne 'install.lock'}).Count) {
        throw 'Runtime directory is not empty and has no pet ownership marker; choose an empty local directory.'
    }
    Save-Json $ownerFile @{schema=1;provider='penguinpal-comfyui';root=$Root}
    $cache = Join-Path $Root 'cache'
    foreach ($dir in @('cache','temp','logs','user','input','output','licenses')) { [IO.Directory]::CreateDirectory((Join-Path $Root $dir)) | Out-Null }
    $env:TEMP = Join-Path $Root 'temp'; $env:TMP = $env:TEMP
    $env:PIP_CACHE_DIR = Join-Path $cache 'pip'
    $env:HF_HOME = Join-Path $cache 'huggingface'
    $env:TORCH_HOME = Join-Path $cache 'torch'
    $env:PIP_DISABLE_PIP_VERSION_CHECK = '1'
    Status 'installing' 'Preparing private Python 3.13.17' 5
    $pythonArchive = Verified-Download 'https://www.python.org/ftp/python/3.13.17/python-3.13.17-embed-amd64.zip' 'python.zip' '9e35319bffedab8b36ad44d8f377cb4f1733bcec257a0b6d04d94f01486ddb38'
    $pythonDir = Join-Path $Root 'python'
    if (!(Test-Path -LiteralPath (Join-Path $pythonDir 'python.exe'))) { Expand-Verified $pythonArchive $pythonDir }
    [IO.File]::WriteAllText((Join-Path $pythonDir 'python313._pth'), "python313.zip`n.`nLib/site-packages`n../app`nimport site`n", $utf8)
    $python = Join-Path $pythonDir 'python.exe'
    $pip = Verified-Download 'https://files.pythonhosted.org/packages/07/25/ee8c38ae72d66b844ce8eb9711ba905dba21ca1a8a0c960057c41cd23337/pip-26.3-py3-none-any.whl' 'pip.whl' 'f3b60b20ffaadd4415ed7957212a38ac63fa4c5fa375bd0f4a797777940fad13'
    if (!(Test-Path -LiteralPath (Join-Path $pythonDir 'Lib/site-packages/pip/__init__.py'))) { Expand-Verified $pip (Join-Path $pythonDir 'Lib/site-packages') }
    $commit = '2c75dc1b379a59e0880619d029607af5169a62ef'
    $archive = Verified-Download "https://codeload.github.com/Comfy-Org/ComfyUI/zip/$commit" 'comfy.zip' '1fec46fc59ab42aaf06d507ce75b9c5bee8e7d17eb4c3bb35079295a6c386cda'
    $app = Join-Path $Root 'app'
    if (!(Test-Path -LiteralPath (Join-Path $app 'main.py'))) {
        $staging = Join-Path $cache 'comfy-source'
        Expand-Verified $archive $staging
        Move-Item -LiteralPath (Join-Path $staging "ComfyUI-$commit") -Destination $app
    }
    Status 'installing' 'Installing CPU PyTorch and ComfyUI dependencies' 20
    Python @('-m','pip','install','torch==2.10.0+cpu','torchvision==0.25.0+cpu','--index-url','https://download.pytorch.org/whl/cpu','--only-binary=:all:')
    $constraints = Join-Path $Root 'cpu-constraints.txt'
    [IO.File]::WriteAllText($constraints, "torch==2.10.0+cpu`ntorchvision==0.25.0+cpu`n", $utf8)
    $dependencyArgs = @('-m','pip','install','-r',(Join-Path $app 'requirements.txt'),'-c',$constraints,'--only-binary=:all:')
    $freeze = Join-Path $Root 'requirements-installed.txt'
    if ((Test-Path -LiteralPath $manifestFile) -and (Test-Path -LiteralPath $freeze)) { $dependencyArgs += @('-r',$freeze) }
    Python $dependencyArgs
    Python @('-m','pip','check')
    Python @('-c','import torch; assert torch.version.cuda is None; print(torch.__version__); print(torch.ones(2)+1)')
    Status 'installing' 'Downloading DreamShaper 8 LCM model (2.13 GB)' 65
    $modelSha = 'a4f3e1526c5dc4fcbe342f5c410d83ae202c7a415fcefcbb92e0f93fcd0a87c3'
    $modelName = 'DreamShaper8_LCM.safetensors'
    $model = Join-Path $app "models/checkpoints/$modelName"
    if (!(Test-Path -LiteralPath $model) -or (Get-FileHash -LiteralPath $model -Algorithm SHA256).Hash -ne $modelSha) {
        $download = Verified-Download 'https://huggingface.co/Lykon/dreamshaper-8-lcm/resolve/4645d8bc6a8e6b106d21606d63e8460cdad4f1a6/DreamShaper8_LCM.safetensors?download=true' $modelName $modelSha 2133804992
        [IO.Directory]::CreateDirectory((Split-Path $model -Parent)) | Out-Null
        Move-Item -LiteralPath $download -Destination $model -Force
    }
    Invoke-WebRequest -UseBasicParsing -Uri 'https://huggingface.co/Lykon/dreamshaper-8-lcm/raw/4645d8bc6a8e6b106d21606d63e8460cdad4f1a6/README.md' -OutFile (Join-Path $Root 'licenses/model-card.md') -TimeoutSec 60
    & $python -m pip freeze | Set-Content -LiteralPath (Join-Path $Root 'requirements-installed.txt') -Encoding UTF8
    if ($LASTEXITCODE -ne 0) { throw 'Cannot record installed dependencies.' }
    $manifest = @{schema=1;provider='penguinpal-comfyui';root=$Root;comfyVersion='0.44.0';comfyCommit=$commit;pythonVersion='3.13.17';model=$modelName;modelSha256=$modelSha;mode='cpu';port=8198;installedAt=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();pythonSha256=(Get-FileHash -LiteralPath $python -Algorithm SHA256).Hash}
    Save-Json (Join-Path $Root 'runtime.json') $manifest
    Save-Json $pointer @{schema=1;provider='penguinpal-comfyui';root=$Root}
    Status 'ready' 'Managed drawing runtime installed' 100
} catch {
    Status 'failed' $_.Exception.Message 0
    Write-Error $_
    exit 1
} finally { if ($lock) { $lock.Dispose() } }
