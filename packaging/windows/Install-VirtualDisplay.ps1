[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
try {
    if (-not [Environment]::Is64BitProcess) { throw 'Windows x64 is required.' }
    $admin = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $admin.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Administrator permission is required.' }
    function Find-VirtualDisplay {
        Get-PnpDevice -Class Display -ErrorAction SilentlyContinue | Where-Object {
            $ids = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_HardwareIds -ErrorAction SilentlyContinue).Data
            $ids -contains 'Root\MttVDD'
        }
    }
    $existing = @(Find-VirtualDisplay)
    if ($existing.Count -gt 0) {
        if ($existing[0].Status -ne 'OK') { throw 'Virtual Display Driver is installed but not ready. Check Device Manager or restart Windows.' }
        'OK' | Set-Content (Join-Path $PSScriptRoot 'result.txt')
        exit 0
    }
    $archive = Join-Path $PSScriptRoot 'driver.zip'
    Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.7.23/VirtualDisplayDriver-x86.Driver.Only.zip' -OutFile $archive
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne 'E24210692B442B39AF763536330CE78B423F19342B7A7792C26DE3944E418B3A') { throw 'Virtual display package SHA256 mismatch.' }
    $unpacked = Join-Path $PSScriptRoot 'driver'
    Expand-Archive -LiteralPath $archive -DestinationPath $unpacked -Force
    $package = Join-Path $unpacked 'VirtualDisplayDriver'
    foreach ($file in @('MttVDD.dll', 'mttvdd.cat')) {
        $sig = Get-AuthenticodeSignature -LiteralPath (Join-Path $package $file)
        if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notlike '*CN=SignPath Foundation,*') { throw "Invalid driver signature: $file" }
    }
    $settings = 'C:\VirtualDisplayDriver'
    New-Item -ItemType Directory -Path $settings -Force | Out-Null
    $settingsFile = Join-Path $settings 'vdd_settings.xml'
    if (-not (Test-Path -LiteralPath $settingsFile)) {
        Copy-Item -LiteralPath (Join-Path $package 'vdd_settings.xml') -Destination $settingsFile
    }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class MauseDisplayInstall {
    [StructLayout(LayoutKind.Sequential)] struct DeviceInfo {
        public uint Size; public Guid ClassGuid; public uint DevInst; public UIntPtr Reserved;
    }
    [DllImport("setupapi.dll", SetLastError=true)] static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid guid, IntPtr parent);
    [DllImport("setupapi.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool SetupDiCreateDeviceInfoW(IntPtr set, string name, ref Guid guid, string description, IntPtr parent, uint flags, ref DeviceInfo info);
    [DllImport("setupapi.dll", SetLastError=true)] static extern bool SetupDiSetDeviceRegistryPropertyW(IntPtr set, ref DeviceInfo info, uint property, byte[] buffer, uint size);
    [DllImport("setupapi.dll", SetLastError=true)] static extern bool SetupDiCallClassInstaller(uint function, IntPtr set, ref DeviceInfo info);
    [DllImport("setupapi.dll", SetLastError=true)] static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);
    [DllImport("newdev.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool UpdateDriverForPlugAndPlayDevicesW(IntPtr parent, string id, string inf, uint flags, out bool reboot);
    static void Check(bool ok) { if (!ok) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    public static bool Install(string inf) {
        Guid guid = new Guid("4D36E968-E325-11CE-BFC1-08002BE10318");
        IntPtr set = SetupDiCreateDeviceInfoList(ref guid, IntPtr.Zero);
        if (set == new IntPtr(-1)) throw new Win32Exception(Marshal.GetLastWin32Error());
        DeviceInfo info = new DeviceInfo(); info.Size = (uint)Marshal.SizeOf(typeof(DeviceInfo));
        bool registered = false;
        try {
            Check(SetupDiCreateDeviceInfoW(set, "Display", ref guid, "Virtual Display Driver", IntPtr.Zero, 1, ref info));
            byte[] id = System.Text.Encoding.Unicode.GetBytes("Root\\MttVDD\0\0");
            Check(SetupDiSetDeviceRegistryPropertyW(set, ref info, 1, id, (uint)id.Length));
            Check(SetupDiCallClassInstaller(0x19, set, ref info)); registered = true;
            bool reboot;
            Check(UpdateDriverForPlugAndPlayDevicesW(IntPtr.Zero, "Root\\MttVDD", inf, 0, out reboot));
            return reboot;
        } catch {
            if (registered) SetupDiCallClassInstaller(5, set, ref info);
            throw;
        } finally { SetupDiDestroyDeviceInfoList(set); }
    }
}
'@
    $inf = Join-Path $package 'MttVDD.inf'
    & pnputil.exe /add-driver $inf
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 3010) { throw "Driver staging failed: $LASTEXITCODE" }
    $reboot = [MauseDisplayInstall]::Install($inf)
    if ($reboot) { throw 'Driver installed. Restart Windows to finish setup.' }
    $ready = @()
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        $ready = @(Find-VirtualDisplay | Where-Object Status -eq 'OK')
        if ($ready.Count -gt 0) { break }
        Start-Sleep -Milliseconds 500
    }
    if ($ready.Count -eq 0) { throw 'Driver installed but not ready. Restart Windows and check Device Manager.' }
    'OK' | Set-Content (Join-Path $PSScriptRoot 'result.txt')
} catch {
    $_.ToString() | Set-Content (Join-Path $PSScriptRoot 'result.txt')
    exit 1
}
