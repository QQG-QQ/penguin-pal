param(
  [string]$Owner = "QQG-QQ",
  [string]$Repo = "penguin-pal",
  [string]$Tag = "v0.2.1",
  [string]$Name = "",
  [switch]$CreateRepoIfMissing
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$version = ($Tag -split '-v')[-1].TrimStart('v')
$product = "PenguinPal Companion"
if ([string]::IsNullOrWhiteSpace($Name)) {
  $Name = "$product $version"
}
# The in-app updater only accepts PenguinPal-Companion-<version>-windows-<arch>-setup.exe
# (see src-tauri/src/companion_updates.rs), so stage build output under that name.
$staging = Join-Path $env:TEMP "penguinpal-release-$version"
New-Item -ItemType Directory -Force -Path $staging | Out-Null
$sources = @{
  (Join-Path $root "src-tauri\target\release\bundle\nsis\${product}_${version}_x64-setup.exe") = "PenguinPal-Companion-$version-windows-x64-setup.exe"
  (Join-Path $root "src-tauri\target\release\bundle\msi\${product}_${version}_x64_en-US.msi") = "PenguinPal-Companion-$version-windows-x64.msi"
}
$assets = @()
foreach ($src in $sources.Keys) {
  if (-not (Test-Path -LiteralPath $src)) {
    throw "Missing release asset: $src"
  }
  $dst = Join-Path $staging $sources[$src]
  Copy-Item -LiteralPath $src -Destination $dst -Force
  $hash = (Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash.ToLower()
  "$hash  $($sources[$src])" | Set-Content -LiteralPath "$dst.sha256" -Encoding ascii -NoNewline
  $assets += $dst
  $assets += "$dst.sha256"
  Write-Host "Staged: $($sources[$src]) ($([math]::Round((Get-Item -LiteralPath $dst).Length / 1MB, 1)) MB)"
}

function Get-GithubToken {
  if ($env:GH_TOKEN) { return $env:GH_TOKEN }
  if ($env:GITHUB_TOKEN) { return $env:GITHUB_TOKEN }

  $query = "protocol=https`nhost=github.com`n`n"
  $credential = $query | git credential fill
  $password = $credential | Where-Object { $_ -like "password=*" } | Select-Object -First 1
  if (-not $password) {
    throw "No GitHub token found in GH_TOKEN/GITHUB_TOKEN or Git Credential Manager."
  }

  return $password.Substring("password=".Length)
}

function Invoke-GithubJson {
  param(
    [ValidateSet("GET", "POST", "PATCH", "PUT", "DELETE")]
    [string]$Method,
    [string]$Uri,
    [object]$Body = $null
  )

  $headers = @{
    Authorization = "Bearer $script:Token"
    Accept = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent" = "penguin-pal-release-script"
  }

  $params = @{
    Method = $Method
    Uri = $Uri
    Headers = $headers
  }

  if ($null -ne $Body) {
    $params.ContentType = "application/json"
    $params.Body = ($Body | ConvertTo-Json -Depth 10)
  }

  Invoke-RestMethod @params
}

if ($env:PENGUIN_RELEASE_DIAGNOSTICS -eq "1") {
  $script:Token = Get-GithubToken
  try {
    $viewer = Invoke-GithubJson -Method GET -Uri "https://api.github.com/user"
    Write-Host "Authenticated as: $($viewer.login)"
  } catch {
    Write-Host "Could not authenticate with GitHub API."
    throw
  }

  try {
    $repoInfo = Invoke-GithubJson -Method GET -Uri "https://api.github.com/repos/$Owner/$Repo"
    Write-Host "Can access repo: $($repoInfo.full_name)"
  } catch {
    if ($_.Exception.Response) {
      Write-Host "Repo access check failed: HTTP $([int]$_.Exception.Response.StatusCode)"
    } else {
      Write-Host "Repo access check failed."
    }
    throw
  }

  return
}

function Get-ReleaseByTag {
  param([string]$TagName)

  try {
    Invoke-GithubJson -Method GET -Uri "https://api.github.com/repos/$Owner/$Repo/releases/tags/$TagName"
  } catch {
    if ($_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404) {
      return $null
    }
    throw
  }
}

function Ensure-RepositoryHasCommit {
  try {
    Invoke-GithubJson -Method GET -Uri "https://api.github.com/repos/$Owner/$Repo/contents/README.md" | Out-Null
    return
  } catch {
    if (-not ($_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404)) {
      throw
    }
  }

  Write-Host "Initializing repository README..."
  $readme = "# PenguinPal Releases`n`nRelease artifacts for PenguinPal Assistant.`n"
  $content = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($readme))
  Invoke-GithubJson -Method PUT -Uri "https://api.github.com/repos/$Owner/$Repo/contents/README.md" -Body @{
    message = "Initialize release repository"
    content = $content
  } | Out-Null
}

$script:Token = Get-GithubToken

try {
  Invoke-GithubJson -Method GET -Uri "https://api.github.com/repos/$Owner/$Repo" | Out-Null
} catch {
  if (-not ($_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404 -and $CreateRepoIfMissing)) {
    throw
  }

  $viewer = Invoke-GithubJson -Method GET -Uri "https://api.github.com/user"
  if ($viewer.login -ne $Owner) {
    throw "Cannot create $Owner/$Repo while authenticated as $($viewer.login)."
  }

  Write-Host "Creating repository $Owner/$Repo..."
  Invoke-GithubJson -Method POST -Uri "https://api.github.com/user/repos" -Body @{
    name = $Repo
    description = "Release artifacts for PenguinPal Assistant."
    private = $false
    has_issues = $false
    has_projects = $false
    has_wiki = $false
  } | Out-Null
}

Ensure-RepositoryHasCommit

$release = Get-ReleaseByTag -TagName $Tag
if ($null -eq $release) {
  Write-Host "Creating release $Tag in $Owner/$Repo..."
  $release = Invoke-GithubJson -Method POST -Uri "https://api.github.com/repos/$Owner/$Repo/releases" -Body @{
    tag_name = $Tag
    name = $Name
    body = "Windows installer release for PenguinPal Assistant $($Tag.TrimStart('v'))."
    draft = $false
    prerelease = $false
  }
} else {
  Write-Host "Using existing release $Tag in $Owner/$Repo..."
}

$release = Invoke-GithubJson -Method GET -Uri "https://api.github.com/repos/$Owner/$Repo/releases/$($release.id)"

foreach ($assetPath in $assets) {
  $assetName = Split-Path -Leaf $assetPath
  $existing = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
  if ($existing) {
    Write-Host "Deleting existing asset $assetName..."
    Invoke-GithubJson -Method DELETE -Uri "https://api.github.com/repos/$Owner/$Repo/releases/assets/$($existing.id)" | Out-Null
  }

  Write-Host "Uploading $assetName..."
  $uploadUri = "https://uploads.github.com/repos/$Owner/$Repo/releases/$($release.id)/assets?name=$([uri]::EscapeDataString($assetName))"
  $headers = @{
    Authorization = "Bearer $script:Token"
    Accept = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent" = "penguin-pal-release-script"
  }
  Invoke-RestMethod -Method POST -Uri $uploadUri -Headers $headers -ContentType "application/octet-stream" -InFile $assetPath | Out-Null
}

Write-Host "Release uploaded: https://github.com/$Owner/$Repo/releases/tag/$Tag"
