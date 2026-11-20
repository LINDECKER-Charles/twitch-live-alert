$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-common.ps1')

Install-WatcherShortcut 'Startup' -NoBrowser
Write-Output "Démarrage automatique activé à la prochaine ouverture de session."
