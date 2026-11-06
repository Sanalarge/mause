# Third-party notices

Mause depends on third-party software distributed under its own license terms.

## Vendored eframe

The source under `vendor/eframe` is derived from eframe 0.37 by Emil Ernerfeldt and contributors. It is available under the Apache License 2.0 or MIT License, at your option. The corresponding license files are stored in that directory.

## Fonts and icons

- Poppins is distributed under the SIL Open Font License 1.1. Its license is in `assets/fonts/OFL-Poppins.txt`.
- Phosphor Icons is distributed under the MIT License. Its license is in `assets/fonts/MIT-Phosphor.txt`.

## Windows installer components

The Windows setup can bundle Microsoft Visual C++ Runtime and selected USB support components. Their notices, versions, sources and checksums are described in `packaging/windows/THIRD_PARTY.md` and `packaging/windows/dependencies.json`. License files downloaded from the verified upstream locations are included in the setup payload.

## Rust, Android and build dependencies

The Cargo and Android dependency graphs include additional open-source components. Their names and pinned versions are recorded in `Cargo.lock`, `mobile/android/rust/Cargo.lock` and the Gradle configuration. Each component remains subject to its own license and copyright notices.

