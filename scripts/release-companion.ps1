#Requires -Version 5.1
<#
.SYNOPSIS
  一条命令发布 Companion 正式版：按发行配置构建 → 打包 NSIS → 上传 GitHub Release。

.DESCRIPTION
  为什么必须用这个脚本：
  1. 应用内“软件更新”只接受
     PenguinPal-Companion-<版本>-windows-x64-setup.exe（见 src-tauri/src/companion_updates.rs）；
  2. 发行包必须用 src-tauri/tauri.companion-installer.conf.json 构建，
     产品名是“PenguinPal 企鹅桌宠”。用默认 tauri.conf.json 打出来的包产品名是
     “PenguinPal Companion”，Windows 会把它们当成两个软件并排安装。

  构建和打包交给 scripts/package-companion.ps1（它负责产品名校验、改名和 .sha256），
  本脚本只负责版本/标签校验、构建、调用打包、上传和上传后摘要复核。

.EXAMPLE
  pwsh -File scripts/release-companion.ps1 -SkipUpload   # 只构建打包，本地验证
  pwsh -File scripts/release-companion.ps1               # 构建 + 打包 + 上传
#>
param(
  [string]$Tag = "",
  [string]$Owner = "QQG-QQ",
  [string]$Repo = "penguin-pal",
  [string]$Name = "",
  [switch]$SkipBuild,
  [switch]$SkipUpload
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repoRoot
$utf8 = New-Object System.Text.UTF8Encoding($false)

function Fail([string]$message) { throw $message }

$version = (Get-Content -LiteralPath (Join-Path $repoRoot "package.json") -Raw | ConvertFrom-Json).version
if ([string]::IsNullOrWhiteSpace($Tag)) { $Tag = "companion-v$version" }
if ($Tag -ne "companion-v$version") {
  Fail "标签 '$Tag' 与 package.json 版本 '$version' 不一致，应为 companion-v$version"
}
if ([string]::IsNullOrWhiteSpace($Name)) { $Name = "PenguinPal Companion $version" }

$bundleConfig = "src-tauri/tauri.companion-installer.conf.json"
$tauri = Join-Path $repoRoot "node_modules\.bin\tauri.cmd"

if (-not $SkipBuild) {
  Write-Host "Building with $bundleConfig ..."
  & $tauri build --no-bundle --config $bundleConfig
  if ($LASTEXITCODE -ne 0) { Fail "构建失败，退出码 $LASTEXITCODE" }
}

$staging = Join-Path $env:TEMP "penguinpal-release-$version"
New-Item -ItemType Directory -Force -Path $staging | Out-Null
$assetName = "PenguinPal-Companion-$version-windows-x64-setup.exe"
$assetPath = Join-Path $staging $assetName
if (Test-Path -LiteralPath $assetPath) { Remove-Item -LiteralPath $assetPath -Force }
if (Test-Path -LiteralPath "$assetPath.sha256") { Remove-Item -LiteralPath "$assetPath.sha256" -Force }

Write-Host "Packaging (validates product name, renames, writes .sha256) ..."
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "package-companion.ps1") -OutputDirectory $staging
if ($LASTEXITCODE -ne 0) { Fail "打包失败，退出码 $LASTEXITCODE" }
if (-not (Test-Path -LiteralPath $assetPath -PathType Leaf)) {
  Fail "打包脚本没有生成 $assetName"
}

$hash = (Get-FileHash -LiteralPath $assetPath -Algorithm SHA256).Hash.ToLowerInvariant()
$sidecar = "$assetPath.sha256"
if (-not (Test-Path -LiteralPath $sidecar -PathType Leaf)) {
  [IO.File]::WriteAllText($sidecar, "$hash  $assetName`n", $utf8)
}
Write-Host ("Installer: {0} ({1:N1} MB)" -f $assetName, ((Get-Item -LiteralPath $assetPath).Length / 1MB))
Write-Host "SHA256:    $hash"

if ($SkipUpload) {
  Write-Host "SkipUpload set; artifacts are in $staging"
  return
}

# ---------------------------------------------------------------- upload
function Get-GithubToken {
  if ($env:GH_TOKEN) { return $env:GH_TOKEN }
  if ($env:GITHUB_TOKEN) { return $env:GITHUB_TOKEN }
  $query = "protocol=https`nhost=github.com`n`n"
  $credential = $query | git credential fill
  $line = $credential | Where-Object { $_ -like "password=*" } | Select-Object -First 1
  if (-not $line) { Fail "没有可用的 GitHub 令牌（GH_TOKEN / GITHUB_TOKEN / Git Credential Manager）" }
  return $line.Substring("password=".Length)
}

$token = Get-GithubToken
$headerFile = Join-Path $staging "headers.txt"
$headers = @(
  "Authorization: Bearer $token",
  "Accept: application/vnd.github+json",
  "X-GitHub-Api-Version: 2022-11-28",
  "User-Agent: penguin-pal-release"
)
[IO.File]::WriteAllLines($headerFile, $headers, $utf8)

function Invoke-Curl([string[]]$curlArgs) {
  $result = & curl.exe -sS @curlArgs
  if ($LASTEXITCODE -ne 0) { Fail "curl 失败（退出码 $LASTEXITCODE）：$($curlArgs -join ' ')" }
  return $result
}

try {
  $release = $null
  try {
    $json = Invoke-Curl @("-H", "@$headerFile", "https://api.github.com/repos/$Owner/$Repo/releases/tags/$Tag")
    $release = $json | ConvertFrom-Json
  } catch { $release = $null }

  if (-not $release -or -not $release.id) {
    Write-Host "Creating release $Tag ..."
    $bodyFile = Join-Path $staging "release.json"
    $payload = @{ tag_name = $Tag; name = $Name; draft = $false; prerelease = $false; body = "Windows installer for PenguinPal Companion $version." } | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText($bodyFile, $payload, $utf8)
    $json = Invoke-Curl @("-X", "POST", "-H", "@$headerFile", "-H", "Content-Type: application/json", "--data-binary", "@$bodyFile", "https://api.github.com/repos/$Owner/$Repo/releases")
    $release = $json | ConvertFrom-Json
  } else {
    Write-Host "Using existing release $Tag"
  }

  foreach ($a in $release.assets) {
    if ($a.name -eq $assetName -or $a.name -eq "$assetName.sha256") {
      Write-Host "Replacing existing asset $($a.name)"
      $null = Invoke-Curl @("-o", "NUL", "-X", "DELETE", "-H", "@$headerFile", "https://api.github.com/repos/$Owner/$Repo/releases/assets/$($a.id)")
    }
  }

  foreach ($file in @($sidecar, $assetPath)) {
    $name = [IO.Path]::GetFileName($file)
    $url = "https://uploads.github.com/repos/$Owner/$Repo/releases/$($release.id)/assets?name=" + [uri]::EscapeDataString($name)
    $uploaded = $false
    for ($attempt = 1; $attempt -le 20; $attempt++) {
      $status = & curl.exe -sS -o NUL -w "%{http_code}" -H "@$headerFile" -H "Content-Type: application/octet-stream" -H "Expect:" --data-binary "@$file" $url
      if ($status -eq "201") {
        Write-Host "Uploaded $name (attempt $attempt)"
        $uploaded = $true
        break
      }
      Write-Host "Upload $name attempt $attempt failed (http=$status), retrying ..."
      Start-Sleep -Seconds 2
    }
    if (-not $uploaded) { Fail "上传 $name 失败：20 次重试均未成功" }
  }

  $final = Invoke-Curl @("-H", "@$headerFile", "https://api.github.com/repos/$Owner/$Repo/releases/$($release.id)") | ConvertFrom-Json
  $published = $final.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
  if (-not $published) { Fail "上传后未在 release 中找到 $assetName" }
  $digest = $published.digest
  if ($digest -and $digest -ne "sha256:$hash") {
    Fail "上传后的摘要 $digest 与本地 $hash 不一致"
  }
  $sizeMb = [math]::Round($published.size / 1MB, 1)
  Write-Host "Release ready: $($final.html_url)"
  Write-Host "Asset: $assetName ($sizeMb MB)"
} finally {
  if (Test-Path -LiteralPath $headerFile) { Remove-Item -LiteralPath $headerFile -Force }
}
