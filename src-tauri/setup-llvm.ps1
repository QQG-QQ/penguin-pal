# LLVM 本地安装脚本
# 将 LLVM 安装到项目目录下，不影响全局环境

$ErrorActionPreference = "Stop"

$LLVM_VERSION = "18.1.8"
$LLVM_DIR = "$PSScriptRoot\.llvm"
$LLVM_BIN = "$LLVM_DIR\bin"
$LIBCLANG_DLL = "$LLVM_BIN\libclang.dll"

# 检查是否已安装
if (Test-Path $LIBCLANG_DLL) {
    Write-Host "[OK] LLVM 已安装: $LLVM_BIN" -ForegroundColor Green
    exit 0
}

Write-Host "=== LLVM 本地安装脚本 ===" -ForegroundColor Cyan
Write-Host "版本: $LLVM_VERSION"
Write-Host "安装目录: $LLVM_DIR"
Write-Host ""

# 创建目录
if (-not (Test-Path $LLVM_DIR)) {
    New-Item -ItemType Directory -Path $LLVM_DIR -Force | Out-Null
}

# 下载 LLVM
$LLVM_URL = "https://github.com/llvm/llvm-project/releases/download/llvmorg-$LLVM_VERSION/LLVM-$LLVM_VERSION-win64.exe"
$LLVM_INSTALLER = "$LLVM_DIR\llvm-installer.exe"

Write-Host "[1/3] 下载 LLVM $LLVM_VERSION ..." -ForegroundColor Yellow
Write-Host "URL: $LLVM_URL"

try {
    # 使用 BITS 或 WebClient 下载
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $LLVM_URL -OutFile $LLVM_INSTALLER -UseBasicParsing
    $ProgressPreference = 'Continue'
} catch {
    Write-Host "[ERROR] 下载失败: $_" -ForegroundColor Red
    Write-Host "请手动下载并解压到: $LLVM_DIR" -ForegroundColor Yellow
    exit 1
}

Write-Host "[2/3] 解压 LLVM (使用 7z 或 innounp) ..." -ForegroundColor Yellow

# LLVM 的 exe 是 NSIS 或 Inno Setup 安装包，需要特殊处理
# 尝试使用 7z 解压
$7zPaths = @(
    "C:\Program Files\7-Zip\7z.exe",
    "C:\Program Files (x86)\7-Zip\7z.exe",
    "$env:ProgramFiles\7-Zip\7z.exe"
)

$7z = $null
foreach ($path in $7zPaths) {
    if (Test-Path $path) {
        $7z = $path
        break
    }
}

if ($7z) {
    Write-Host "使用 7-Zip 解压..."
    & $7z x $LLVM_INSTALLER -o"$LLVM_DIR" -y | Out-Null

    # 7z 解压 NSIS 包会产生 `$PLUGINSDIR` 等目录，bin 在根目录
    # 如果 bin 在子目录，需要移动
    if (Test-Path "$LLVM_DIR\`$PLUGINSDIR") {
        Remove-Item "$LLVM_DIR\`$PLUGINSDIR" -Recurse -Force -ErrorAction SilentlyContinue
    }
} else {
    # 没有 7z，尝试静默安装到指定目录
    Write-Host "未找到 7-Zip，使用静默安装模式..."
    Write-Host "这可能需要几分钟..."

    $installArgs = "/S /D=$LLVM_DIR"
    $process = Start-Process -FilePath $LLVM_INSTALLER -ArgumentList $installArgs -Wait -PassThru

    if ($process.ExitCode -ne 0) {
        Write-Host "[ERROR] 安装失败，退出码: $($process.ExitCode)" -ForegroundColor Red
        exit 1
    }
}

# 清理安装包
Remove-Item $LLVM_INSTALLER -Force -ErrorAction SilentlyContinue

Write-Host "[3/3] 验证安装 ..." -ForegroundColor Yellow

if (Test-Path $LIBCLANG_DLL) {
    Write-Host ""
    Write-Host "=== 安装成功 ===" -ForegroundColor Green
    Write-Host "LLVM 已安装到: $LLVM_DIR"
    Write-Host "libclang.dll: $LIBCLANG_DLL"
    Write-Host ""
    Write-Host "现在可以运行 'cargo build' 了"
} else {
    Write-Host "[ERROR] 安装验证失败，未找到 libclang.dll" -ForegroundColor Red
    Write-Host "请检查 $LLVM_DIR 目录" -ForegroundColor Yellow
    exit 1
}
