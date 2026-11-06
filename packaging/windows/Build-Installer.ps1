[CmdletBinding()]
param(
    [ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
    [string]$OutputDirectory,
    [string]$Repository = $env:GITHUB_REPOSITORY,
    [switch]$SkipBuild,
    [switch]$LegacyZip
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$work = Join-Path $repo 'target/windows-installer'
$deps = Join-Path $work 'deps'
$payload = Join-Path $work 'payload'
if (-not $Version) {
    $Version = [regex]::Match((Get-Content (Join-Path $repo 'Cargo.toml') -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'A numeric major.minor.patch version is required.' }
if ($Repository -and $Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { throw 'Repository must use owner/name format.' }
$repositoryUrl = if ($Repository) { "https://github.com/$Repository" } else { 'https://github.com' }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $work 'output' }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force $deps, $payload, $OutputDirectory | Out-Null
$manifest = Get-Content (Join-Path $PSScriptRoot 'dependencies.json') -Raw | ConvertFrom-Json

function Get-VerifiedDownload($Entry) {
    $destination = Join-Path $deps $Entry.file
    if (-not (Test-Path -LiteralPath $destination) -or (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $Entry.sha256) {
        $partial = "$destination.download"
        Invoke-WebRequest -Uri $Entry.url -OutFile $partial
        if ((Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $Entry.sha256) {
            throw "SHA256 mismatch: $($Entry.file). Update dependencies.json only after verifying the upstream release."
        }
        Move-Item -LiteralPath $partial -Destination $destination -Force
    }
    if ($Entry.PSObject.Properties.Name -contains 'publisher') {
        $signature = Get-AuthenticodeSignature -LiteralPath $destination
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike "*CN=$($Entry.publisher),*") {
            throw "Unexpected Authenticode publisher: $($Entry.file)"
        }
    }
    return $destination
}

foreach ($entry in $manifest.prerequisites) { Get-VerifiedDownload $entry | Write-Host }
foreach ($entry in $manifest.notices) { Get-VerifiedDownload $entry | Write-Host }
$compilerInstaller = Get-VerifiedDownload $manifest.compiler
$compilerDir = Join-Path $work "tools/InnoSetup-$($manifest.compiler.version)"
$iscc = Join-Path $compilerDir 'ISCC.exe'
if (-not (Test-Path -LiteralPath $iscc)) {
    $process = Start-Process -FilePath $compilerInstaller -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER', '/NOICONS', '/TASKS=', "/DIR=`"$compilerDir`"") -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) { throw "Inno Setup compiler installation failed: $($process.ExitCode)" }
}
if (-not (Test-Path -LiteralPath $iscc)) { throw 'ISCC.exe was not installed.' }

function Reset-StagingDirectory([string]$Directory) {
    $resolved = [IO.Path]::GetFullPath($Directory)
    if (-not $resolved.StartsWith($work + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Staging directory must stay inside $work"
    }
    if (Test-Path -LiteralPath $resolved) {
        if ((Get-Item -LiteralPath $resolved -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Refusing a staging directory reparse point: $resolved"
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    New-Item -ItemType Directory -Force $resolved | Out-Null
}

Push-Location $repo
try {
    if (-not $SkipBuild) {
        cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'Cargo release build failed.' }
    }
    $binary = Join-Path $repo 'target/release/mause.exe'
    if (-not (Test-Path -LiteralPath $binary)) { throw 'Build target/release/mause.exe first.' }
    $bytes = [IO.File]::ReadAllBytes($binary)
    $pe = [BitConverter]::ToInt32($bytes, 0x3c)
    if ([BitConverter]::ToUInt16($bytes, $pe + 4) -ne 0x8664) { throw 'This installer requires an x86_64 Windows application.' }
    Reset-StagingDirectory $payload
    Copy-Item -LiteralPath $binary -Destination (Join-Path $payload 'mause.exe') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'THIRD_PARTY.md') -Destination $payload -Force
    New-Item -ItemType Directory -Force (Join-Path $payload 'licenses') | Out-Null
    Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination (Join-Path $payload 'licenses') -Force
    Copy-Item -LiteralPath (Join-Path $repo 'NOTICE') -Destination (Join-Path $payload 'licenses') -Force
    Copy-Item -LiteralPath (Join-Path $repo 'THIRD_PARTY_NOTICES.md') -Destination (Join-Path $payload 'licenses') -Force
    Copy-Item (Join-Path $repo 'assets/fonts/*.txt') -Destination (Join-Path $payload 'licenses') -Force
    foreach ($entry in $manifest.notices) {
        Copy-Item -LiteralPath (Join-Path $deps $entry.file) -Destination (Join-Path $payload 'licenses') -Force
    }
    rustc --edition 2024 (Join-Path $PSScriptRoot 'icon.rs') -o (Join-Path $work 'make-icon.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Icon generator build failed.' }
    & (Join-Path $work 'make-icon.exe') (Join-Path $payload 'mause.ico')
    if ($LASTEXITCODE -ne 0) { throw 'Icon generation failed.' }
    & $iscc "/DAppVersion=$Version" "/DRepositoryUrl=$repositoryUrl" "/DPayloadDir=$payload" "/DDepsDir=$deps" "/DOutputDir=$OutputDirectory" "/DVcVersion=$($manifest.prerequisites[0].version)" (Join-Path $PSScriptRoot 'mause.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed.' }
    $setup = Join-Path $OutputDirectory "mause-v$Version-windows-x86_64-setup.exe"
    if (-not (Test-Path -LiteralPath $setup)) { throw 'Setup output is missing.' }
    $hash = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $([IO.Path]::GetFileName($setup))" | Set-Content -Encoding ascii "$setup.sha256"
    Copy-Item (Join-Path $PSScriptRoot 'dependencies.json') (Join-Path $OutputDirectory "mause-v$Version-windows-dependencies.json") -Force
    if ($LegacyZip) {
        $legacy = Join-Path $work 'legacy/mause'
        Reset-StagingDirectory $legacy
        Copy-Item (Join-Path $payload '*') $legacy -Recurse -Force
        Compress-Archive -Path $legacy -DestinationPath (Join-Path $OutputDirectory "mause-v$Version-windows-x86_64.zip") -Force
    }
    Write-Host "Installer: $setup"
} finally { Pop-Location }
