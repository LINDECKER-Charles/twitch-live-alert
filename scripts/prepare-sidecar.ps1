[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$project = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $project
try {
    $rustInfo = @(rustc -Vv)
    if ($LASTEXITCODE -ne 0) { throw 'rustc est indisponible.' }
    $target = ($rustInfo | Select-String '^host: (.+)$').Matches.Groups[1].Value
    if ($target -notmatch '^(x86_64|aarch64|i686)-pc-windows-msvc$') {
        throw "La distribution Windows requiert une cible MSVC native (cible détectée : $target)."
    }
    # Un répertoire explicite garde le chemin cohérent même avec CARGO_TARGET_DIR.
    $sidecarTarget = Join-Path $project 'target'
    cargo build --locked --release --bin twitch-live-alert --target-dir $sidecarTarget
    if ($LASTEXITCODE -ne 0) { throw 'Échec de compilation du service.' }
    $binaryDir = Join-Path $project 'src-tauri/binaries'
    [void][IO.Directory]::CreateDirectory($binaryDir)
    Copy-Item -LiteralPath (Join-Path $sidecarTarget 'release/twitch-live-alert.exe') -Destination (Join-Path $binaryDir "twitch-live-alert-$target.exe") -Force
} finally {
    Pop-Location
}
