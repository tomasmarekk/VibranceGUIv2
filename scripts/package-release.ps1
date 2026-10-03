# Stages a fixed allowlist of Windows release assets and their SHA-256 checksums.
# Neither the repository tree nor driver DLLs are copied into release artifacts.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    node scripts/check-repository.mjs
    if ($LASTEXITCODE -ne 0) { throw 'repository guard failed' }
    $version = (Get-Content -LiteralPath package.json -Raw | ConvertFrom-Json).version
    $portableSource = 'target/release/vibrance-gui-v2.exe'
    if (-not (Test-Path -LiteralPath $portableSource -PathType Leaf)) { throw 'portable release executable is missing' }
    $installers = @(Get-ChildItem -LiteralPath 'target/release/bundle/nsis' -File -Filter "*_${version}_x64-setup.exe")
    if ($installers.Count -ne 1) { throw 'expected exactly one x64 NSIS installer for the checked-in version' }
    New-Item -ItemType Directory -Path artifacts -Force | Out-Null
    node scripts/collect-licenses.mjs
    if ($LASTEXITCODE -ne 0) { throw 'dependency notice collection failed' }
    $portableName = "VibranceGUIv2_${version}_x64-portable.exe"
    $installerName = "VibranceGUIv2_${version}_x64-setup.exe"
    Copy-Item -LiteralPath $portableSource -Destination "artifacts/$portableName" -Force
    Copy-Item -LiteralPath $installers[0].FullName -Destination "artifacts/$installerName" -Force
    Copy-Item -LiteralPath LICENSE -Destination artifacts/LICENSE.txt -Force
    $assetNames = @($portableName, $installerName, 'LICENSE.txt', 'THIRD-PARTY-LICENSES.txt')
    $checksums = foreach ($assetName in $assetNames) {
        $hash = (Get-FileHash -LiteralPath "artifacts/$assetName" -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $assetName"
    }
    Set-Content -LiteralPath artifacts/SHA256SUMS.txt -Value $checksums -Encoding utf8NoBOM
    Write-Output "Staged $($assetNames.Count) release files and SHA256SUMS.txt for $version"
} finally {
    Pop-Location
}
