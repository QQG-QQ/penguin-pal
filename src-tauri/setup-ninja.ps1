# Ninja Local Installation Script
# Install Ninja to project directory

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$NINJA_VERSION = "1.11.1"
$NINJA_DIR = "$PSScriptRoot\.ninja"
$NINJA_EXE = "$NINJA_DIR\ninja.exe"

# Check if already installed
if (Test-Path $NINJA_EXE) {
    Write-Host "[OK] Ninja already installed: $NINJA_DIR" -ForegroundColor Green
    exit 0
}

Write-Host "=== Ninja Local Installation ===" -ForegroundColor Cyan
Write-Host "Version: $NINJA_VERSION"
Write-Host "Install path: $NINJA_DIR"
Write-Host ""

# Create directory
if (-not (Test-Path $NINJA_DIR)) {
    New-Item -ItemType Directory -Path $NINJA_DIR -Force | Out-Null
}

# Download Ninja
$NINJA_URL = "https://github.com/ninja-build/ninja/releases/download/v$NINJA_VERSION/ninja-win.zip"
$NINJA_ZIP = "$NINJA_DIR\ninja.zip"

Write-Host "[1/2] Downloading Ninja $NINJA_VERSION ..." -ForegroundColor Yellow

try {
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $NINJA_URL -OutFile $NINJA_ZIP -UseBasicParsing
    $ProgressPreference = 'Continue'
    Write-Host "[OK] Download complete" -ForegroundColor Green
}
catch {
    Write-Host "[ERROR] Download failed: $_" -ForegroundColor Red
    exit 1
}

Write-Host "[2/2] Extracting Ninja ..." -ForegroundColor Yellow

try {
    Expand-Archive -Path $NINJA_ZIP -DestinationPath $NINJA_DIR -Force
    Write-Host "[OK] Extract complete" -ForegroundColor Green
}
catch {
    Write-Host "[ERROR] Extract failed: $_" -ForegroundColor Red
    exit 1
}

# Cleanup zip
Remove-Item $NINJA_ZIP -Force -ErrorAction SilentlyContinue

if (Test-Path $NINJA_EXE) {
    $version = & $NINJA_EXE --version 2>&1
    Write-Host ""
    Write-Host "=== Installation Successful ===" -ForegroundColor Green
    Write-Host "Ninja installed to: $NINJA_DIR"
    Write-Host "Version: $version"
    exit 0
}
else {
    Write-Host "[ERROR] Verification failed, ninja.exe not found" -ForegroundColor Red
    exit 1
}
