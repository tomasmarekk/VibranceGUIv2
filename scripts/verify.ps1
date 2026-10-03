# Runs the same fail-fast validation locally and in both Windows workflows.
# Build outputs are never executed here because that could change display settings.
[CmdletBinding()]
param(
    [string] $Version,
    [switch] $Build
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Push-Location (Split-Path -Parent $PSScriptRoot)

function Invoke-Checked {
    <# Invokes a tool and converts its nonzero exit into a terminating verification failure. #>
    param([string] $Tool, [string[]] $Arguments)
    & $Tool @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Tool failed with exit code $LASTEXITCODE"
    }
}

try {
    $guardArguments = @('scripts/check-repository.mjs')
    if ($Version) { $guardArguments += $Version }
    Invoke-Checked node $guardArguments
    Invoke-Checked node @('--test', 'scripts/check-release.test.mjs')
    Invoke-Checked npm @('ci')
    Invoke-Checked npm @('run', 'typecheck')
    Invoke-Checked npm @('test')
    Invoke-Checked npm @('run', 'build')
    Invoke-Checked cargo @('fmt', '--all', '--check')
    Invoke-Checked cargo @('clippy', '--locked', '--workspace', '--all-targets', '--all-features', '--', '-D', 'warnings')
    Invoke-Checked cargo @('test', '--locked', '--workspace', '--all-features')
    $previousRustdocFlags = $env:RUSTDOCFLAGS
    try {
        $env:RUSTDOCFLAGS = "$previousRustdocFlags -D warnings".Trim()
        Invoke-Checked cargo @('doc', '--locked', '--workspace', '--all-features', '--no-deps')
    } finally {
        $env:RUSTDOCFLAGS = $previousRustdocFlags
    }
    if ($Build) {
        Invoke-Checked npm @('run', 'tauri', '--', 'build', '--ci', '--bundles', 'nsis', '--', '--locked')
        & "$PSScriptRoot/package-release.ps1"
    }
} finally {
    Pop-Location
}
