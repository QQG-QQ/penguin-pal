# CMake Local Installation Script
# Install CMake to project directory without affecting global environment

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$CMAKE_VERSION = "3.28.3"
$CMAKE_DIR = "$PSScriptRoot\.cmake"
$CMAKE_BIN = "$CMAKE_DIR\bin"
$CMAKE_EXE = "$CMAKE_BIN\cmake.exe"

# Check if already installed
if (Test-Path $CMAKE_EXE) {
    Write-Host "[OK] CMake already installed: $CMAKE_BIN" -ForegroundColor Green
    exit 0
}

Write-Host "=== CMake Local Installation ===" -ForegroundColor Cyan
Write-Host "Version: $CMAKE_VERSION"
Write-Host "Install path: $CMAKE_DIR"
Write-Host ""

# Create directory
if (-not (Test-Path $CMAKE_DIR)) {
    New-Item -ItemType Directory -Path $CMAKE_DIR -Force | Out-Null
}

# Download CMake portable (zip)
$CMAKE_URL = "https://github.com/Kitware/CMake/releases/download/v$CMAKE_VERSION/cmake-$CMAKE_VERSION-windows-x86_64.zip"
$CMAKE_ZIP = "$CMAKE_DIR\cmake.zip"

Write-Host "[1/3] Downloading CMake $CMAKE_VERSION ..." -ForegroundColor Yellow
Write-Host "URL: $CMAKE_URL"

try {
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $CMAKE_URL -OutFile $CMAKE_ZIP -UseBasicParsing
    $ProgressPreference = 'Continue'
    Write-Host "[OK] Download complete" -ForegroundColor Green
}
catch {
    Write-Host "[ERROR] Download failed: $_" -ForegroundColor Red
    exit 1
}

Write-Host "[2/3] Extracting CMake ..." -ForegroundColor Yellow

try {
    # Extract to temp location first
    $tempDir = "$CMAKE_DIR\temp"
    Expand-Archive -Path $CMAKE_ZIP -DestinationPath $tempDir -Force

    # Move contents from nested folder to CMAKE_DIR
    $extractedFolder = Get-ChildItem -Path $tempDir -Directory | Select-Object -First 1
    Get-ChildItem -Path $extractedFolder.FullName | Move-Item -Destination $CMAKE_DIR -Force

    # Cleanup temp folder
    Remove-Item $tempDir -Recurse -Force

    Write-Host "[OK] Extract complete" -ForegroundColor Green
}
catch {
    Write-Host "[ERROR] Extract failed: $_" -ForegroundColor Red
    exit 1
}

# Cleanup zip
Remove-Item $CMAKE_ZIP -Force -ErrorAction SilentlyContinue

Write-Host "[3/3] Verifying installation ..." -ForegroundColor Yellow

if (Test-Path $CMAKE_EXE) {
    # Test cmake version
    $version = & $CMAKE_EXE --version 2>&1 | Select-Object -First 1
    Write-Host ""
    Write-Host "=== Installation Successful ===" -ForegroundColor Green
    Write-Host "CMake installed to: $CMAKE_DIR"
    Write-Host "cmake.exe: $CMAKE_EXE"
    Write-Host "Version: $version"
    Write-Host ""
    Write-Host "You can now run 'cargo build'"
    exit 0
}
else {
    Write-Host "[ERROR] Verification failed, cmake.exe not found" -ForegroundColor Red
    Write-Host "Please check directory: $CMAKE_DIR" -ForegroundColor Yellow
    exit 1
}
