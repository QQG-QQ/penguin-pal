param(
  [Parameter(Mandatory = $true)]
  [string]$SourceDir
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$targetRoot = Join-Path $repoRoot 'src-tauri\resources\codex\windows-x64'

if (-not (Test-Path $SourceDir)) {
  throw "SourceDir 不存在: $SourceDir"
}

$codexCmd = Join-Path $SourceDir 'node_modules\.bin\codex.cmd'
if (-not (Test-Path $codexCmd)) {
  throw "未找到 $codexCmd。SourceDir 必须包含 node_modules/.bin/codex.cmd。"
}

if (Test-Path $targetRoot) {
  Remove-Item $targetRoot -Recurse -Force
}

New-Item -ItemType Directory -Path $targetRoot | Out-Null
Copy-Item -Path (Join-Path $SourceDir '*') -Destination $targetRoot -Recurse -Force

Write-Host "[done] embedded Codex runtime copied to $targetRoot"
