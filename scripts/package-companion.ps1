#Requires -Version 7.0
param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $repo
try {
  $binary = Join-Path $repo 'src-tauri/target/release/penguin-pal.exe'
  if (!(Test-Path -LiteralPath $binary -PathType Leaf)) {
    throw 'Build the companion release first: npm exec tauri build -- --no-bundle --config src-tauri/tauri.companion-installer.conf.json'
  }
  $running = @(Get-CimInstance Win32_Process -Filter "name = 'penguin-pal.exe'" | Where-Object { $_.ExecutablePath -eq $binary })
  if ($running.Count) { throw 'Exit the release-directory test application before packaging' }
  $sourceHash = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
  $version = (Get-Content -LiteralPath (Join-Path $repo 'package.json') -Raw | ConvertFrom-Json).version
  $binaryVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($binary)
  if ($binaryVersion.ProductVersion -ne $version -or $binaryVersion.ProductName -ne 'PenguinPal 企鹅桌宠') {
    throw 'The binary version/product does not match the companion installer configuration; rebuild before packaging'
  }
  $originalBytes = [IO.File]::ReadAllBytes($binary)
  # Tauri records NSIS bundle type by replacing this fixed-size marker.
  # The bundler may patch a temporary copy, leaving the source unchanged.
  # Restore the original release if needed; record the expected installed hash.
  $encoding = [Text.Encoding]::GetEncoding(28591)
  $expectedBytes = $encoding.GetBytes($encoding.GetString($originalBytes).Replace('__TAURI_BUNDLE_TYPE_VAR_UNK', '__TAURI_BUNDLE_TYPE_VAR_NSS'))
  $hasher = [Security.Cryptography.SHA256]::Create()
  try { $expectedHash = [Convert]::ToHexString($hasher.ComputeHash($expectedBytes)) } finally { $hasher.Dispose() }
  try {
    & (Join-Path $repo 'node_modules/.bin/tauri.cmd') bundle --bundles nsis --config src-tauri/tauri.companion-installer.conf.json --ci
    if ($LASTEXITCODE -ne 0) { throw "Installer bundling failed: $LASTEXITCODE" }
    $packagedHash = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
    if ($packagedHash -ne $sourceHash -and $packagedHash -ne $expectedHash) {
      throw 'Bundling changed bytes beyond the expected NSIS marker'
    }
  } finally {
    if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash -ne $sourceHash) {
      [IO.File]::WriteAllBytes($binary, $originalBytes)
    }
  }
  $installers = @(Get-ChildItem -LiteralPath (Join-Path $repo 'src-tauri/target/release/bundle/nsis') -Filter "*_$($version)_x64-setup.exe")
  if ($installers.Count -ne 1) { throw "Expected one installer for $version, found $($installers.Count)" }
  if ($OutputDirectory) {
    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
    $destination = Join-Path $OutputDirectory "PenguinPal-Companion-$version-windows-x64-setup.exe"
    if ((Test-Path -LiteralPath $destination) -and ((Get-FileHash -LiteralPath $destination).Hash -ne (Get-FileHash -LiteralPath $installers[0].FullName).Hash)) {
      throw 'Output already contains a different installer'
    }
    Copy-Item -LiteralPath $installers[0].FullName -Destination $destination -Force
  } else { $destination = $installers[0].FullName }
  $checksum = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
  [IO.File]::WriteAllText("$destination.sha256", "$checksum  $([IO.Path]::GetFileName($destination))`n", [Text.UTF8Encoding]::new($false))
  [ordered]@{version=$version;installer=$destination;sha256=(Get-FileHash -LiteralPath $destination).Hash.ToLowerInvariant();applicationSha256=$sourceHash.ToLowerInvariant();applicationAfterBundleSha256=$packagedHash.ToLowerInvariant();expectedInstalledApplicationSha256=$expectedHash.ToLowerInvariant()} | ConvertTo-Json
} finally { Pop-Location }
