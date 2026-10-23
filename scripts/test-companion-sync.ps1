$ErrorActionPreference = 'Stop'
$pythonCommand = Get-Command python, python3 -ErrorAction SilentlyContinue |
  Where-Object { $_.Source -notlike '*WindowsApps*' } | Select-Object -First 1
if (!$pythonCommand) { throw 'Python 3 is required for server protocol tests.' }
& $pythonCommand.Source (Join-Path $PSScriptRoot '../tests/test_companion_sync.py')
if ($LASTEXITCODE -ne 0) { throw "Server protocol tests failed: $LASTEXITCODE" }
