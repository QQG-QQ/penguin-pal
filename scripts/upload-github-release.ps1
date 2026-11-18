#Requires -Version 5.1
<#
.SYNOPSIS
  兼容入口：只上传已构建好的 Companion 安装包。

.DESCRIPTION
  以前这个脚本会直接上传 bundle 目录里的原始产物，导致资产名不符合应用内更新器约定
  （PenguinPal-Companion-<版本>-windows-x64-setup.exe），并且可能上传用错配置构建的包。
  现在它只是 scripts/release-companion.ps1 -SkipBuild 的薄封装：仍然做产品名校验、
  改名、生成 .sha256 并复核上传摘要。
#>
param(
  [string]$Owner = "QQG-QQ",
  [string]$Repo = "penguin-pal",
  [string]$Tag = "",
  [string]$Name = ""
)

$ErrorActionPreference = "Stop"
$arguments = @("-Owner", $Owner, "-Repo", $Repo, "-SkipBuild")
if (-not [string]::IsNullOrWhiteSpace($Tag)) { $arguments += @("-Tag", $Tag) }
if (-not [string]::IsNullOrWhiteSpace($Name)) { $arguments += @("-Name", $Name) }

& (Join-Path $PSScriptRoot "release-companion.ps1") @arguments
exit $LASTEXITCODE
