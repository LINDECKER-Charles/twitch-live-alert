[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$project = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $project
try {
    & (Join-Path $PSScriptRoot 'prepare-sidecar.ps1')
    if (Get-Command cargo-tauri -ErrorAction SilentlyContinue) {
        cargo tauri build --ci --bundles nsis -- --locked
    } else {
        npx --yes '@tauri-apps/cli@2.12.0' build --ci --bundles nsis -- --locked
    }
    if ($LASTEXITCODE -ne 0) { throw "Échec de construction de l’installeur NSIS." }
} finally {
    Pop-Location
}
