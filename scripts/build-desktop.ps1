[CmdletBinding()]
param([switch]$PublicRelease)

$ErrorActionPreference = 'Stop'
$project = Split-Path -Parent $PSScriptRoot
$previousFlags = $env:CARGO_ENCODED_RUSTFLAGS
Push-Location -LiteralPath $project
try {
    if ($PublicRelease) {
        # Rust embeds source paths in diagnostics. Keep the host profile and checkout out of a public binary.
        $separator = [char]31
        $flags = @()
        if ($previousFlags) {
            $flags += $previousFlags.Split($separator)
        } elseif ($env:RUSTFLAGS) {
            $flags += ($env:RUSTFLAGS -split '\s+' | Where-Object { $_ })
        }
        $flags += "--remap-path-prefix=$([Environment]::GetFolderPath('UserProfile'))=/build/user"
        if ($env:CARGO_HOME) { $flags += "--remap-path-prefix=$env:CARGO_HOME=/build/cargo" }
        if ($env:RUSTUP_HOME) { $flags += "--remap-path-prefix=$env:RUSTUP_HOME=/build/rustup" }
        $flags += "--remap-path-prefix=$project=/build/twitch-live-alert"
        $flags += '-Clink-arg=/PDBALTPATH:%_PDB%'
        $env:CARGO_ENCODED_RUSTFLAGS = $flags -join $separator
    }
    & (Join-Path $PSScriptRoot 'prepare-sidecar.ps1')
    if (Get-Command cargo-tauri -ErrorAction SilentlyContinue) {
        cargo tauri build --ci --bundles nsis -- --locked
    } else {
        npx --yes '@tauri-apps/cli@2.12.0' build --ci --bundles nsis -- --locked
    }
    if ($LASTEXITCODE -ne 0) { throw "Échec de construction de l’installeur NSIS." }
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $previousFlags
    Pop-Location
}
