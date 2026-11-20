# Interstitial N4 Pro

A Rust device plugin for OpenDeck, based on [ambiso/opendeck-akp05 v0.11.1](https://github.com/ambiso/opendeck-akp05/tree/v0.11.1). It incorporates the [upstream sleep, reconnect and OpenAction 2.8 updates](https://github.com/ambiso/opendeck-akp05/commit/d0a5a795304c5f36ae3eb21686bf5a5c1385f588). It supports the upstream AKP05/N4 device family and adds an optional continuous Windows display for the **VSDinside N4 Pro** (USB `5548:1023`). The four encoder zones remain independently editable in OpenDeck.

## Getting started on Windows

Use OpenDeck 2.5.0 or newer. Build the Windows x64 ZIP using the instructions below. Install the ZIP in OpenDeck with **Plugins → Install from file**. The Interstitial fork has its own plugin identity and keeps the upstream device namespace, `n4`. Use one active device plugin for the controller at a time. Save a copy of existing OpenDeck profiles before switching device plugins.

Assign actions to the ten square keys and four knobs in OpenDeck. For a continuous strip, enable it in `%APPDATA%\Interstitial\config.toml`. For sensor graphs or metric-driven ring colors, also install Interstitial Hardware Graphs. Both public plugins use the Interstitial ring channel. See the [continuous strip guide](docs/continuous-strip.md) for alignment, user-provided themes, lock behavior and light controls.

## Build and test

The Windows build uses stable Rust, the x64 MSVC target, Visual Studio C++ Build Tools, CMake, NASM and PowerShell 7. Put the build tools on PATH. Supply the directory containing the release x64 VC runtime DLLs from your Visual Studio installation.

```powershell
cargo test --release --locked --all-targets --target x86_64-pc-windows-msvc
cargo clippy --release --locked --all-targets --target x86_64-pc-windows-msvc -- -D warnings
./scripts/package-windows.ps1 -CrtDirectory '<path to x64 Microsoft.VC*.CRT directory>'
```

The Windows ZIP is written to `build/`. The packaged executable accepts `--check-encoder` for an offline full-strip JPEG round trip. Visual output, physical inputs, vibration and reconnect behavior require compatible hardware. For upstream Linux device access, use the included [udev rules](40-opendeck-akp05.rules). Consult [upstream](https://github.com/ambiso/opendeck-akp05) for the inherited device family and platform build conventions.

On Linux, `just build-linux` produces the static Linux executable and `just build-mac` cross-compiles the universal macOS executable using the pinned Nix flake. Run `just package <windows-zip>` after those builds to combine them with the complete MSVC package. This preserves the vendor transport, SIMD encoder notices and app-local runtime in the combined ZIP. CI uploads artifacts for inspection.

## Attribution

The original plugin credits the [elgato-streamdeck contributors](https://github.com/streamduck-org/elgato-streamdeck) and the [naerschhersch](https://github.com/naerschhersch/opendeck-akp05), [GrauBlitz](https://github.com/GrauBlitz/opendeck-akp05) and [maillota](https://github.com/maillota/opendeck-akp05) forks. The icon comes from the naerschhersch project. This fork retains the upstream [GPL license](LICENSE).

The Windows display uses the [pinned MiraBox Device SDK](https://github.com/MiraboxSpace/StreamDock-Device-SDK/tree/fc124cb5bc8fe0cd6c72d3f2af80588adc7dcffb) and its [notice](vendor/mirabox/LICENSE). This software is based in part on the work of the Independent JPEG Group. The [JPEG notices](vendor/jpeg-notices) accompany the SIMD encoder.

The upstream project also has a [Signal community group](https://signal.group/#CjQKIHyUkzhTbUGZIeMoFhrnPfTglV_zx8x2wfO7oyA6uthqEhA7ZNnwF_EUd5XmaNIVPtEN).
