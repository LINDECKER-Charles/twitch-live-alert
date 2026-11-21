$repoRoot = Split-Path -Parent $PSScriptRoot
$watcherExe = Join-Path $repoRoot 'target\release\twitch-live-alert.exe'
$dataDir = Join-Path $repoRoot 'data'
$processRecord = Join-Path $dataDir 'watcher-process.json'
$controlLock = Join-Path $dataDir 'watcher-control.lock'
$stdoutLog = Join-Path $dataDir 'watcher.stdout.log'
$stderrLog = Join-Path $dataDir 'watcher.stderr.log'
$startupLog = Join-Path $dataDir 'watcher.startup.log'
$watcherUrl = 'http://127.0.0.1:37219'

function Assert-WatcherBinary {
    if (-not (Test-Path -LiteralPath $watcherExe -PathType Leaf)) {
        throw 'Binaire introuvable. Exécutez cargo build --locked --release à la racine du dépôt.'
    }
}

function Enter-WatcherControl {
    # Le verrou de fichiers sérialise les scripts même entre sessions Windows.
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while ($true) {
        try {
            return [IO.File]::Open($controlLock, 'OpenOrCreate', 'ReadWrite', 'None')
        } catch [IO.IOException] {
            $win32Error = $_.Exception.HResult -band 0xffff
            if ($win32Error -notin 32, 33 -or [DateTime]::UtcNow -ge $deadline) {
                throw
            }
            Start-Sleep -Milliseconds 100
        }
    }
}

function Get-ManagedWatcher {
    if (-not (Test-Path -LiteralPath $processRecord -PathType Leaf)) {
        return
    }

    $record = Get-Content -LiteralPath $processRecord -Raw | ConvertFrom-Json
    $processId = 0
    $startTicks = 0L
    if (
        -not [int]::TryParse([string]$record.ProcessId, [ref]$processId) -or $processId -le 0 -or
        -not [long]::TryParse([string]$record.StartUtcTicks, [ref]$startTicks) -or $startTicks -le 0
    ) {
        throw "Fiche de processus invalide dans $processRecord."
    }

    return Get-VerifiedWatcherProcess $processId -StartUtcTicks $startTicks
}

function Get-VerifiedWatcherProcess([int]$ProcessId, [long]$StartUtcTicks, [switch]$IncludeDebug) {
    try {
        $process = [Diagnostics.Process]::GetProcessById($processId)
    } catch [ArgumentException] {
        return
    }

    $verified = $false
    try {
        if ($process.ProcessName -ine [IO.Path]::GetFileNameWithoutExtension($watcherExe)) {
            return
        }

        # Conserver le handle exact jusqu'à l'arrêt évite une réutilisation du PID.
        $null = $process.Handle
        if ($StartUtcTicks -gt 0 -and $process.StartTime.ToUniversalTime().Ticks -ne $StartUtcTicks) {
            return
        }
        $allowedPaths = @($watcherExe)
        if ($IncludeDebug) {
            $allowedPaths += Join-Path $repoRoot 'target\debug\twitch-live-alert.exe'
        }
        if ($process.MainModule.FileName -notin $allowedPaths) {
            return
        }
        if ($process.SessionId -ne (Get-Process -Id $PID).SessionId) {
            throw "Le processus $processId appartient à une autre session Windows."
        }

        $verified = $true
        return $process
    } catch {
        if (-not $process.HasExited) {
            throw
        }
    } finally {
        if (-not $verified) {
            $process.Dispose()
        }
    }
}

function Get-WatcherListenerId {
    $listeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object {
        $_.LocalPort -eq 37219 -and $_.LocalAddress -in '127.0.0.1', '0.0.0.0'
    })
    if ($listeners.Count -gt 1) {
        throw "Plusieurs applications écoutent sur le port 37219."
    }
    if ($listeners.Count -eq 1) {
        return $listeners[0].OwningProcess
    }
}

function Get-WatcherWebStatus([Diagnostics.Process]$Process) {
    if ($Process.HasExited -or (Get-WatcherListenerId) -ne $Process.Id) {
        return
    }
    try {
        $status = Invoke-RestMethod -UseBasicParsing -Uri "$watcherUrl/api/status" -TimeoutSec 2 -MaximumRedirection 0
        if ($status.app_id -ceq 'twitch-live-alert' -and $status.process_id -eq $Process.Id -and -not $Process.HasExited) {
            return $status
        }
    } catch {
        # Le serveur local peut être encore en cours de démarrage.
    }
}

function Get-ExistingWatcherWeb {
    $listenerId = Get-WatcherListenerId
    if (-not $listenerId) {
        return
    }
    $process = Get-VerifiedWatcherProcess $listenerId -IncludeDebug
    if ($null -eq $process) {
        throw "Le port 37219 est occupé par une autre application (PID $listenerId)."
    }
    return $process
}

function Wait-WatcherWeb([Diagnostics.Process]$Process) {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        if ($Process.HasExited) {
            throw "Le binaire s'est arrêté (code $($Process.ExitCode)). Consultez les journaux dans $dataDir."
        }

        $status = Get-WatcherWebStatus $Process
        if ($status.process_id -eq $Process.Id) {
            return
        }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)

    throw "L'interface locale est indisponible sur $watcherUrl. Consultez les journaux dans $dataDir."
}

function Request-WatcherShutdown([Diagnostics.Process]$Process) {
    try {
        $status = Get-WatcherWebStatus $Process
        if ($status.process_id -ne $Process.Id) {
            return $false
        }

        $headers = @{ Origin = $watcherUrl; 'X-Twitch-Live-Alert' = 'local' }
        $response = Invoke-WebRequest -UseBasicParsing -Uri "$watcherUrl/api/shutdown" -Method Post `
            -ContentType 'application/json' -Headers $headers -Body '{}' -TimeoutSec 3 -MaximumRedirection 0
        return $response.StatusCode -eq 202
    } catch {
        return $false
    }
}

function Get-WatcherShortcutPath([Environment+SpecialFolder]$Folder) {
    $shortcutDir = [Environment]::GetFolderPath($Folder, 'DoNotVerify')
    if ([string]::IsNullOrEmpty($shortcutDir)) {
        throw 'Dossier du raccourci introuvable pour ce compte Windows.'
    }

    return Join-Path $shortcutDir 'Twitch Live Alert.lnk'
}

function Get-WatcherShortcutArguments([switch]$NoBrowser) {
    $launcher = Join-Path $PSScriptRoot 'start.ps1'
    $arguments = '-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy RemoteSigned -File "{0}"' -f $launcher
    if ($NoBrowser) {
        $arguments += ' -NoBrowser'
    }

    return $arguments
}

function Get-WatcherShortcut([Environment+SpecialFolder]$Folder, [switch]$NoBrowser) {
    $shortcutPath = Get-WatcherShortcutPath $Folder
    $powerShell = Join-Path ([Environment]::SystemDirectory) 'WindowsPowerShell\v1.0\powershell.exe'
    $arguments = Get-WatcherShortcutArguments -NoBrowser:$NoBrowser
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($shortcutPath)
    if (
        (Test-Path -LiteralPath $shortcutPath -PathType Leaf) -and
        ($shortcut.TargetPath -ine $powerShell -or $shortcut.Arguments -ine $arguments)
    ) {
        throw "Le raccourci existant $shortcutPath pointe vers un autre lanceur."
    }

    $shortcut.TargetPath = $powerShell
    $shortcut.Arguments = $arguments
    $shortcut.WorkingDirectory = $repoRoot
    $shortcut.WindowStyle = 7
    $shortcut.Description = 'Surveillance des directs Twitch selon les filtres configurés'
    return $shortcut
}

function Install-WatcherShortcut([Environment+SpecialFolder]$Folder, [switch]$NoBrowser) {
    Assert-WatcherBinary
    $shortcutPath = Get-WatcherShortcutPath $Folder
    $null = [IO.Directory]::CreateDirectory((Split-Path -Parent $shortcutPath))
    $shortcut = Get-WatcherShortcut $Folder -NoBrowser:$NoBrowser
    $shortcut.Save()
}
