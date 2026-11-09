param([ValidateRange(1024,65535)][int]$Port = 48736)
$ErrorActionPreference = 'Stop'
$record = Join-Path $PSScriptRoot "build/server-$Port.json"
if (-not (Test-Path -LiteralPath $record)) { Write-Output 'No launcher record for this port.'; return }
$saved = Get-Content -LiteralPath $record -Raw | ConvertFrom-Json
$serverProcess = Get-Process -Id $saved.pid -ErrorAction SilentlyContinue
if ($serverProcess -and "$($serverProcess.StartTime.ToUniversalTime().Ticks)" -eq $saved.startedTicks) {
    $origin = "http://127.0.0.1:$Port"
    $health = Invoke-RestMethod "$origin/live/capabilities" -TimeoutSec 3
    if ($health.app -ne 'HX Unbound' -or $health.instance -ne "$($saved.pid)") { throw 'Server identity does not match the launcher record.' }
    $null = Invoke-RestMethod "$origin/live/shutdown" -Method Post -ContentType 'application/json' -Headers @{Origin=$origin} -Body '{}' -TimeoutSec 8
    if (-not $serverProcess.WaitForExit(5000)) { throw 'Shutdown has not completed; check the local server log. No process was force-killed.' }
    Write-Output 'HX Unbound server stopped.'
} elseif ($serverProcess) { throw 'The saved process ID has been reused; refusing to stop it.' }
Remove-Item -LiteralPath $record
