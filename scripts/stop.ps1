$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-common.ps1')

if (-not (Test-Path -LiteralPath $dataDir -PathType Container)) {
    Write-Output "Aucun processus de surveillance géré par ce dépôt."
    return
}

$lock = Enter-WatcherControl
try {
    $process = Get-ManagedWatcher
    if ($null -ne $process) {
        try {
            if ((-not $process.HasExited) -and (Request-WatcherShutdown $process)) {
                $null = $process.WaitForExit(10000)
            }
            if (-not $process.HasExited) {
                try {
                    $process.Kill()
                } catch [InvalidOperationException] {
                    if (-not $process.HasExited) {
                        throw
                    }
                }
                if (-not $process.WaitForExit(5000)) {
                    throw "Le processus $($process.Id) ne s'est pas arrêté."
                }
            }
            Write-Output "Surveillance arrêtée (PID $($process.Id))."
        } finally {
            $process.Dispose()
        }
    } else {
        Write-Output "Aucun processus de surveillance géré par ce dépôt."
    }

    if (Test-Path -LiteralPath $processRecord -PathType Leaf) {
        Remove-Item -LiteralPath $processRecord
    }
} finally {
    $lock.Dispose()
}
