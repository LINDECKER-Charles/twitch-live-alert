[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Installer,
    [ValidateRange(1, 65535)][int]$PortNumber = 37219,
    [string]$WorkDirectory = [IO.Path]::GetTempPath()
)

$ErrorActionPreference = 'Stop'
$origin = "http://127.0.0.1:$PortNumber"
$installerPath = (Resolve-Path -LiteralPath $Installer).Path
$publisherKey = 'HKCU:\Software\Twitch Live Alert'
$productKey = Join-Path $publisherKey 'Twitch Live Alert'
$publisherExisted = Test-Path -LiteralPath $publisherKey
$registered = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -eq 'Twitch Live Alert' }
if ($registered -or (Test-Path -LiteralPath $productKey) -or (Get-Process -Name 'twitch-live-alert-desktop','twitch-live-alert' -ErrorAction SilentlyContinue)) {
    throw 'Utilisez un compte de test sans installation ni processus Twitch Live Alert existant.'
}
$port = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $PortNumber)
$port.ExclusiveAddressUse = $true
$port.Start()
$port.Stop()

$smokeRoot = Join-Path ([IO.Path]::GetFullPath($WorkDirectory)) ('TwitchLiveAlert-smoke-' + [Guid]::NewGuid().ToString('N'))
$installDir = Join-Path $smokeRoot 'installation'
$dataDir = Join-Path $smokeRoot 'profil'
$desktopExe = Join-Path $installDir 'twitch-live-alert-desktop.exe'
$serviceExe = Join-Path $installDir 'twitch-live-alert.exe'
$uninstaller = Join-Path $installDir 'uninstall.exe'
$desktop = $null
[void][IO.Directory]::CreateDirectory($smokeRoot)

function Start-TestDesktop([string]$Arguments) {
    $start = [Diagnostics.ProcessStartInfo]::new($desktopExe, $Arguments)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WorkingDirectory = $installDir
    foreach ($name in 'TWITCH_CLIENT_ID','TWITCH_CLIENT_SECRET','DATABASE_PATH','TWITCH_LIVE_ALERT_DESKTOP_EXE') {
        $start.EnvironmentVariables.Remove($name)
    }
    $start.EnvironmentVariables['TWITCH_LIVE_ALERT_DATA_DIR'] = $dataDir
    $start.EnvironmentVariables['TWITCH_LIVE_ALERT_PORT'] = [string]$PortNumber
    return [Diagnostics.Process]::Start($start)
}

function Wait-TestCondition([scriptblock]$Condition, [string]$Failure) {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        if (& $Condition) { return }
        Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $deadline)
    throw $Failure
}

function Open-TestDashboard {
    $second = Start-TestDesktop ''
    try {
        if (-not $second.WaitForExit(20000) -or $second.ExitCode -ne 0) {
            throw 'Le deuxième lancement ne rejoint pas la première instance.'
        }
        Wait-TestCondition {
            $desktop.Refresh()
            $desktop.MainWindowHandle -ne [IntPtr]::Zero -and $desktop.MainWindowTitle -like '*Twitch Live Alert*'
        } "La fenêtre WebView2 ne s’est pas ouverte."
    } finally {
        if (-not $second.HasExited) { $second.Kill() }
        $second.Dispose()
    }
}

try {
    # NSIS requires /D= to be last and the directory itself to be unquoted.
    $setup = Start-Process -FilePath $installerPath -ArgumentList "/S /NS /D=$installDir" -WindowStyle Hidden -PassThru -Wait
    if ($setup.ExitCode -ne 0) { throw "Échec de l’installation NSIS (code $($setup.ExitCode))." }
    foreach ($file in $desktopExe, $serviceExe, $uninstaller) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Fichier installé manquant : $file." }
    }

    # Hold the product port with this PowerShell process. The launcher must reject it
    # before contacting HTTP, showing a dashboard or terminating the foreign listener.
    $port.Start()
    try {
        $conflict = Start-TestDesktop '--background'
        try {
            if (-not $conflict.WaitForExit(15000) -or $conflict.ExitCode -eq 0) {
                throw 'Le lanceur a accepté un port occupé par un autre binaire.'
            }
            $diagnostic = Join-Path $dataDir 'desktop-startup.log'
            if ([IO.File]::ReadAllText($diagnostic) -notmatch [string]$PortNumber) { throw 'Diagnostic du port occupé absent.' }
            if ($port.Pending()) { throw 'Le lanceur a contacté le service inconnu.' }
        } finally {
            if (-not $conflict.HasExited) { $conflict.Kill() }
            $conflict.Dispose()
        }
    } finally {
        $port.Stop()
    }

    $desktop = Start-TestDesktop '--background'
    Wait-TestCondition {
        if ($desktop.HasExited) { throw "Le bureau s’est arrêté (code $($desktop.ExitCode))." }
        try {
            $status = Invoke-RestMethod -Uri "$origin/api/status" -TimeoutSec 2 -MaximumRedirection 0
            if ($status.app_id -ne 'twitch-live-alert') { return $false }
            $service = Get-Process -Id $status.process_id
            return $service.Path -eq $serviceExe
        } catch {
            return $false
        }
    } "Le bureau installé n’a pas démarré son service."
    $page = Invoke-WebRequest -UseBasicParsing -Uri "$origin/" -MaximumRedirection 0 -TimeoutSec 5
    if ($page.StatusCode -ne 200 -or $page.Content -notmatch 'Twitch Live Alert') {
        throw 'Le tableau de bord navigateur est inaccessible.'
    }
    Open-TestDashboard
    if (-not $desktop.CloseMainWindow()) { throw 'Impossible de fermer la fenêtre du bureau.' }
    Wait-TestCondition {
        $desktop.Refresh()
        -not $desktop.HasExited -and $desktop.MainWindowTitle -notlike '*Twitch Live Alert*'
    } "La fermeture de fenêtre ne ramène pas l’application en zone de notification."
    Open-TestDashboard
    $headers = @{Origin = $origin; 'X-Twitch-Live-Alert' = 'local'}
    $stop = Invoke-WebRequest -UseBasicParsing -Uri "$origin/api/shutdown" -Method Post -ContentType 'application/json' -Body '{}' -Headers $headers -MaximumRedirection 0 -TimeoutSec 5
    if ($stop.StatusCode -ne 202 -or -not $desktop.WaitForExit(15000) -or $desktop.ExitCode -ne 0) {
        throw "Le bureau est resté actif après l’arrêt du service."
    }
} finally {
    if ($null -ne $desktop) {
        if (-not $desktop.HasExited) { $desktop.Kill(); $null = $desktop.WaitForExit(10000) }
        $desktop.Dispose()
    }
    if (Test-Path -LiteralPath $uninstaller -PathType Leaf) {
        $remove = Start-Process -FilePath $uninstaller -ArgumentList '/S' -WindowStyle Hidden -Wait -PassThru
        if ($remove.ExitCode -ne 0) { throw "Échec de la désinstallation (code $($remove.ExitCode))." }
        Wait-TestCondition {
            -not (Test-Path -LiteralPath $desktopExe) -and -not (Test-Path -LiteralPath $serviceExe)
        } 'Les binaires installés sont toujours présents.'
        # NSIS normally remembers the location for a future reinstall. Only remove
        # keys absent before this smoke and still pointing to its private directory.
        $record = Get-Item -LiteralPath $productKey -ErrorAction SilentlyContinue
        if ($record -and $record.GetValue('') -eq $installDir -and $record.SubKeyCount -eq 0) {
            Remove-Item -LiteralPath $productKey
        }
        $publisher = Get-Item -LiteralPath $publisherKey -ErrorAction SilentlyContinue
        if (-not $publisherExisted -and $publisher -and $publisher.SubKeyCount -eq 0 -and $publisher.ValueCount -eq 0) {
            Remove-Item -LiteralPath $publisherKey
        }
    }
}

Write-Output "Installation NSIS, collision de port, navigateur, fenêtre, réouverture et désinstallation vérifiés. Profil de test : $dataDir"
