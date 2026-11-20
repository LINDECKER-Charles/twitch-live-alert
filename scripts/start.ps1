param([switch]$NoBrowser)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-common.ps1')

$null = [IO.Directory]::CreateDirectory($dataDir)
$process = $null
try {
    $lock = Enter-WatcherControl
    try {
        $process = Get-ManagedWatcher
        if ($null -eq $process) {
            $process = Get-ExistingWatcherWeb
        }
        if ($null -eq $process) {
            Assert-WatcherBinary
            $process = Start-Process -FilePath $watcherExe -ArgumentList '--background' `
                -WorkingDirectory $repoRoot -WindowStyle Hidden -PassThru `
                -RedirectStandardOutput $stdoutLog -RedirectStandardError $stderrLog
            try {
                if ($process.HasExited) {
                    throw "Arrêt immédiat du binaire (code $($process.ExitCode)). Consultez les journaux dans $dataDir."
                }

                $record = @{
                    ProcessId = $process.Id
                    StartUtcTicks = [string]$process.StartTime.ToUniversalTime().Ticks
                }
                $record | ConvertTo-Json -Compress | Set-Content -LiteralPath $processRecord -Encoding ASCII
            } catch {
                if (-not $process.HasExited) {
                    $process.Kill()
                    $process.WaitForExit()
                }
                throw
            }
        }
    } finally {
        $lock.Dispose()
    }

    Wait-WatcherWeb $process
    if (-not $NoBrowser) {
        Start-Process -FilePath $watcherUrl
    }
    Write-Output "Interface disponible sur $watcherUrl (PID $($process.Id))."
} catch {
    # Un raccourci de démarrage masque la console, y compris les diagnostics.
    try {
        $_.Exception.Message | Set-Content -LiteralPath $startupLog -Encoding UTF8
    } catch {
        Write-Warning "Impossible d'écrire $startupLog."
    }
    throw
} finally {
    if ($null -ne $process) {
        $process.Dispose()
    }
}
