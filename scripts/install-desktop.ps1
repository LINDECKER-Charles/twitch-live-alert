$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-common.ps1')

Install-WatcherShortcut 'DesktopDirectory'
Write-Output "Raccourci Twitch Live Alert installé sur le Bureau."
