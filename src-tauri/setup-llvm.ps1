# LLVM Local Installation Script
# Install LLVM to project directory without affecting global environment

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$LLVM_VERSION = "18.1.8"
$LLVM_DIR = "$PSScriptRoot\.llvm"
$LLVM_BIN = "$LLVM_DIR\bin"
$LIBCLANG_DLL = "$LLVM_BIN\libclang.dll"

# Check if already installed
if (Test-Path $LIBCLANG_DLL) {
    Write-Host "[OK] LLVM already installed: $LLVM_BIN" -ForegroundColor Green
    exit 0
}

Write-Host "=== LLVM Local Installation ===" -ForegroundColor Cyan
Write-Host "Version: $LLVM_VERSION"
Write-Host "Install path: $LLVM_DIR"
Write-Host ""

# Create directory
if (-not (Test-Path $LLVM_DIR)) {
    New-Item -ItemType Directory -Path $LLVM_DIR -Force | Out-Null
}

# Download LLVM
$LLVM_URL = "https://github.com/llvm/llvm-project/releases/download/llvmorg-$LLVM_VERSION/LLVM-$LLVM_VERSION-win64.exe"
$LLVM_INSTALLER = "$LLVM_DIR\llvm-installer.exe"

Write-Host "[1/3] Downloading LLVM $LLVM_VERSION ..." -ForegroundColor Yellow
Write-Host "URL: $LLVM_URL"
Write-Host "This may take a few minutes..."

try {
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $LLVM_URL -OutFile $LLVM_INSTALLER -UseBasicParsing
    $ProgressPreference = 'Continue'
    Write-Host "[OK] Download complete" -ForegroundColor Green
}
catch {
    Write-Host "[ERROR] Download failed: $_" -ForegroundColor Red
    exit 1
}

Write-Host "[2/3] Installing LLVM (silent mode) ..." -ForegroundColor Yellow
Write-Host "This may take several minutes, please wait..."

# Use silent install
try {
    $process = Start-Process -FilePath $LLVM_INSTALLER -ArgumentList "/S", "/D=$LLVM_DIR" -Wait -PassThru -NoNewWindow
    if ($process.ExitCode -ne 0) {
        Write-Host "[ERROR] Install failed with exit code: $($process.ExitCode)" -ForegroundColor Red
        exit 1
    }
    Write-Host "[OK] Install complete" -ForegroundColor Green
}
catch {
    Write-Host "[ERROR] Install failed: $_" -ForegroundColor Red
    exit 1
}

# Cleanup installer
Remove-Item $LLVM_INSTALLER -Force -ErrorAction SilentlyContinue

Write-Host "[3/3] Verifying installation ..." -ForegroundColor Yellow

if (Test-Path $LIBCLANG_DLL) {
    Write-Host ""
    Write-Host "=== Installation Successful ===" -ForegroundColor Green
    Write-Host "LLVM installed to: $LLVM_DIR"
    Write-Host "libclang.dll: $LIBCLANG_DLL"
    Write-Host ""
    Write-Host "You can now run 'cargo build'"
    exit 0
}
else {
    Write-Host "[ERROR] Verification failed, libclang.dll not found" -ForegroundColor Red
    Write-Host "Please check directory: $LLVM_DIR" -ForegroundColor Yellow
    exit 1
}
