# Third-party notices

Mause depends on third-party software distributed under its own license terms.

## Vendored eframe

The source under `vendor/eframe` is derived from eframe 0.37 by Emil Ernerfeldt and contributors. It is available under the Apache License 2.0 or MIT License, at your option. The corresponding license files are stored in that directory.

## Performance monitoring: sysinfo

sysinfo 0.40.3, https://github.com/GuillaumeGomez/sysinfo

The MIT License (MIT)

Copyright (c) 2015 Guillaume Gomez

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Fonts and icons

- Poppins is distributed under the SIL Open Font License 1.1. Its license is in `assets/fonts/OFL-Poppins.txt`.
- Phosphor Icons is distributed under the MIT License. Its license is in `assets/fonts/MIT-Phosphor.txt`.
- Font Awesome Free 6.5.2 Brands by Fonticons, Inc. (https://fontawesome.com) is distributed under the SIL Open Font License 1.1. It provides the Windows, Apple, Linux and Android logos. Its license is in `assets/fonts/OFL-FontAwesome.txt`.

## Windows installer components

The Windows setup can bundle Microsoft Visual C++ Runtime and selected USB support components. Their notices, versions, sources and checksums are described in `packaging/windows/THIRD_PARTY.md` and `packaging/windows/dependencies.json`. License files downloaded from the verified upstream locations are included in the setup payload.

## Rust, Android and build dependencies

The Cargo and Android dependency graphs include additional open-source components. Their names and pinned versions are recorded in `Cargo.lock`, `mobile/android/rust/Cargo.lock` and the Gradle configuration. Each component remains subject to its own license and copyright notices.

