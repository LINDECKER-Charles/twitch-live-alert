$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-common.ps1')

$shortcutPath = Get-WatcherShortcutPath 'Startup'
if (Test-Path -LiteralPath $shortcutPath -PathType Leaf) {
    $null = Get-WatcherShortcut 'Startup' -NoBrowser
    Remove-Item -LiteralPath $shortcutPath
}

Write-Output "Démarrage automatique désactivé. Pour arrêter la surveillance en cours, lancez scripts\stop.ps1."
