# Windows setup components

The full installation includes Mause, Microsoft Visual C++ v14 Runtime (x64), usbipd-win 5.3.0 and USBip 0.9.8.1 (usbip-win2 client). Select the compact installation to omit USB sharing drivers. Runtime and driver packages are shared system components and remain installed when Mause is removed.

- Microsoft Visual C++ Runtime: Microsoft Corporation. The unmodified Microsoft redistributable is included; its Microsoft license terms apply. https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist
- usbipd-win: Frans van Dorsselaer and contributors, GPL-3.0. Corresponding source and license: https://github.com/dorssel/usbipd-win/tree/v5.3.0 . Source archive: https://github.com/dorssel/usbipd-win/archive/refs/tags/v5.3.0.zip
- usbip-win2: Vadym Hrynchyshyn and contributors, BSD-2-Clause. Source, copyright and license: https://github.com/vadimgrn/usbip-win2/tree/v.0.9.8.1
- Embedded font and icon licenses are also included.

The original USB component license texts and the usbipd-win 5.3.0 source archive are installed alongside these notices in the `licenses` directory.

All installer payload versions and SHA256 hashes are recorded in packaging/windows/dependencies.json. No dependency is downloaded during installation. USB drivers may require a restart; Mause is not launched automatically before that restart. The installer never enables test-signing or disables Windows driver signature checks.

A private-network firewall exception for Mause can be selected during setup. It applies only to the installed application; public-network exceptions are not added by Mause. USB driver packages manage their own services and configuration.
