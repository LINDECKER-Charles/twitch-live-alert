$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-common.ps1')

Install-WatcherShortcut 'DesktopDirectory'
Write-Output "Raccourci Bloodborne FR Live installé sur le Bureau."
