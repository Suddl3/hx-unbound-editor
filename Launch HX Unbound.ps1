param(
    [ValidateRange(1024,65535)][int]$Port = 48736,
    [switch]$ExperimentalDevice,
    [switch]$NoBrowser,
    [string]$Python
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
if (-not $Python) {
    $Python = Join-Path $root '.venv\Scripts\python.exe'
    if (-not (Test-Path -LiteralPath $Python)) { $Python = (Get-Command python -ErrorAction Stop).Source }
}
$logDirectory = Join-Path $root 'build'
New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
# A successful TCP connection is sufficient to refuse the port; do not inspect
# or stop another editor's process, and do not reuse an unverified server.
$client = [System.Net.Sockets.TcpClient]::new()
try {
    $pending = $client.ConnectAsync('127.0.0.1', $Port)
    try { $null = $pending.Wait(1000) } catch {}
    if ($client.Connected) { throw "Port $Port is already in use. Choose -Port with another local port." }
} finally { $client.Dispose() }
$entryScript = '"' + (Join-Path $root 'floor_live_editor.py') + '"'
$serverArguments = @('-B','-X','utf8',$entryScript,'--no-browser','--port',"$Port")
if ($ExperimentalDevice) { $serverArguments += '--experimental-device' }
$serverProcess = Start-Process -FilePath $Python -ArgumentList $serverArguments -WorkingDirectory $root -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $logDirectory "server-$Port.out.log") -RedirectStandardError (Join-Path $logDirectory "server-$Port.err.log")
$origin = "http://127.0.0.1:$Port"
$ready = $false
try {
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        $serverProcess.Refresh()
        if ($serverProcess.HasExited) { break }
        try {
            $health = Invoke-RestMethod "$origin/live/capabilities" -TimeoutSec 2
            if ($health.app -ne 'HX Unbound' -or $health.instance -ne "$($serverProcess.Id)") { throw 'Unexpected server response' }
            $ready = $true
            break
        } catch { Start-Sleep -Milliseconds 250 }
    }
    if (-not $ready) { throw "Server did not start; inspect build/server-$Port.err.log." }
    @{pid=$serverProcess.Id;startedTicks="$($serverProcess.StartTime.ToUniversalTime().Ticks)";port=$Port} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $logDirectory "server-$Port.json") -Encoding UTF8
    if (-not $NoBrowser) { Start-Process "$origin/live?offline=1" }
    Write-Output "HX Unbound: $origin/live?offline=1 (PID $($serverProcess.Id)). Device access: $ExperimentalDevice"
} catch {
    if (-not $serverProcess.HasExited) { $serverProcess.Kill(); $null = $serverProcess.WaitForExit(3000) }
    throw
}
