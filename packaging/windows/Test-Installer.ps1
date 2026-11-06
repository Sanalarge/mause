[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Setup,
    [switch]$AllowLocalInstallation
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:CI -ne 'true' -and -not $AllowLocalInstallation) {
    throw 'This installs/uninstalls Mause and shared prerequisites. Run on CI or explicitly opt in with -AllowLocalInstallation.'
}
$key = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{B840A882-6C48-4E4D-98A9-469B86A19FC2}_is1'
if (Test-Path $key) { throw 'An existing Mause setup installation must not be replaced by the smoke test.' }
$temporaryRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$base = Join-Path $temporaryRoot "mause-installer-smoke-$([guid]::NewGuid().ToString('N'))"
$app = Join-Path $base 'Installed App'
New-Item -ItemType Directory -Force $base | Out-Null
$Setup = (Resolve-Path -LiteralPath $Setup).Path
function Invoke-Setup([string]$LogName) {
    $process = Start-Process -FilePath $Setup -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS', '/COMPONENTS=core', '/TASKS=', "/DIR=`"$app`"", "/LOG=`"$(Join-Path $base $LogName)`"") -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -notin @(0, 3010)) { throw "Setup failed: $($process.ExitCode). Logs: $base" }
}
try {
    Invoke-Setup 'install.log'
    foreach ($file in @('mause.exe', 'mause.ico', 'licenses/THIRD_PARTY.md', 'licenses/OFL-Poppins.txt', 'licenses/MIT-Phosphor.txt', 'licenses/GPL-usbipd-win.md', 'licenses/BSD-usbip-win2.txt', 'licenses/usbipd-win-5.3.0-source.zip')) {
        if (-not (Test-Path -LiteralPath (Join-Path $app $file))) { throw "Missing installed file: $file" }
    }
    if (-not (Test-Path $key)) { throw 'Apps & Features uninstall registration is missing.' }
    $marker = Join-Path $app 'user-kept.txt'
    'keep' | Set-Content -LiteralPath $marker
    Invoke-Setup 'upgrade.log'
    if ((Get-Content -LiteralPath $marker -Raw).Trim() -ne 'keep') { throw 'Upgrade removed unrelated data.' }
    $process = Start-Process -FilePath (Join-Path $app 'mause.exe') -ArgumentList '--invalid-installer-smoke-argument' -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 2) { throw "Installed application failed to load: $($process.ExitCode)" }
} finally {
    $uninstall = Join-Path $app 'unins000.exe'
    if (Test-Path -LiteralPath $uninstall) {
        $process = Start-Process -FilePath $uninstall -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$(Join-Path $base 'uninstall.log')`"") -Wait -PassThru -WindowStyle Hidden
        if ($process.ExitCode -ne 0) { throw "Uninstall failed: $($process.ExitCode). Logs: $base" }
    }
}
if (Test-Path -LiteralPath (Join-Path $app 'mause.exe')) { throw 'Uninstall left the application installed.' }
if (Test-Path $key) { throw 'Uninstall left its Apps & Features registration.' }
if (-not (Test-Path -LiteralPath (Join-Path $app 'user-kept.txt'))) { throw 'Uninstall removed unrelated user data.' }
Write-Host "Install, upgrade, application load and uninstall passed. Logs: $base"
