{
  description = "OpenDeck AKP05 plugin builds and release packaging";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      crane,
      flake-utils,
      rust-overlay,
      advisory-db,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        inherit (pkgs) lib;
        rustTargets =
          if pkgs.stdenv.isDarwin then
            [
              "x86_64-apple-darwin"
              "aarch64-apple-darwin"
            ]
          else
            [
              "x86_64-unknown-linux-musl"
              "x86_64-pc-windows-gnu"
            ];
        mkCrane =
          targetPkgs:
          (crane.mkLib targetPkgs).overrideToolchain (
            p: p.pkgsBuildHost.rust-bin.stable.latest.default.override { targets = rustTargets; }
          );
        craneLib = mkCrane pkgs;
        src = craneLib.cleanCargoSource ./.;
        common = {
          inherit src;
          strictDeps = true;
        };
        cargoArtifacts = craneLib.buildDepsOnly common;
        native = craneLib.buildPackage (common // { inherit cargoArtifacts; });

        # Nix supplies the target linker and libraries; Rust supplies the target stdlib.
        crossPackage =
          targetPkgs: extra:
          let
            crossCrane = mkCrane targetPkgs;
          in
          crossCrane.buildPackage (
            common
            // {
              doCheck = false; # Tests run in the native build, never under an emulator.
            }
            // extra
          );

        linuxStatic = crossPackage pkgs.pkgsCross.musl64 {
          postInstall = ''
            if ${pkgs.binutils}/bin/readelf -l "$out/bin/opendeck-akp05" | grep -q INTERP; then
              echo "Linux release binary has a dynamic interpreter" >&2
              exit 1
            fi
            if ${pkgs.binutils}/bin/readelf -d "$out/bin/opendeck-akp05" | grep -q NEEDED; then
              echo "Linux release binary has shared-library dependencies" >&2
              exit 1
            fi
          '';
        };
        windows = crossPackage pkgs.pkgsCross.mingwW64 {
          # Avoid pulling Wine into the build merely to set an unused test runner.
          __CRANE_EXPORT_CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER = "${pkgs.coreutils}/bin/false";
          CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = "-C target-feature=+crt-static -L native=${pkgs.pkgsCross.mingwW64.windows.pthreads}/lib";
          postInstall = ''
            if ${pkgs.binutils}/bin/objdump -p "$out/bin/opendeck-akp05.exe" | grep -iE 'DLL Name:.*(libgcc|libstdc|libwinpthread)'; then
              echo "Windows release binary depends on a MinGW runtime DLL" >&2
              exit 1
            fi
          '';
        };
        # Match the SDK used by the former Docker build, now fetched and hash-pinned by Nix.
        macSdk = pkgs.fetchzip {
          url = "https://github.com/phracker/MacOSX-SDKs/releases/download/11.3/MacOSX11.3.sdk.tar.xz";
          hash = "sha256-BoFWhRSHaD0j3dzDOFtGJ6DiRrdzMJhkjxztxCluFKo=";
        };
        macCrossCrane = (crane.mkLib pkgs).overrideToolchain (
          p:
          p.pkgsBuildHost.rust-bin.stable.latest.default.override {
            targets = [
              "x86_64-apple-darwin"
              "aarch64-apple-darwin"
            ];
          }
        );
        macZigbuild = pkgs.callPackage ./nix/cargo-zigbuild.nix { };
        macSlice =
          arch:
          if pkgs.stdenv.isDarwin then
            crossPackage pkgs.pkgsCross.${arch + "-darwin"} {
              MACOSX_DEPLOYMENT_TARGET = "11.3";
            }
          else
            macCrossCrane.buildPackage (
              common
              // {
                doCheck = false;
                nativeBuildInputs = [ macZigbuild ];
                CARGO_BUILD_TARGET = "${arch}-apple-darwin";
                cargoBuildCommand = "cargoWithProfile zigbuild --target ${arch}-apple-darwin";
                SDKROOT = macSdk;
                MACOSX_DEPLOYMENT_TARGET = "11.3";
                # Zig needs a writable cache even in an isolated Nix build.
                preBuild = ''
                  export ZIG_GLOBAL_CACHE_DIR="$TMPDIR/zig-cache"
                  export CARGO_ZIGBUILD_CACHE_DIR="$TMPDIR/zigbuild-cache"
                '';
              }
            );
        macLipo = if pkgs.stdenv.isDarwin then "lipo" else "llvm-lipo";
        macOtool = if pkgs.stdenv.isDarwin then "otool" else "llvm-otool";
        macUniversal =
          pkgs.runCommand "opendeck-akp05-macos-universal"
            {
              nativeBuildInputs = [ (if pkgs.stdenv.isDarwin then pkgs.cctools else pkgs.llvmPackages.llvm) ];
            }
            ''
              mkdir -p "$out/bin"
              ${macLipo} -create \
                ${macSlice "x86_64"}/bin/opendeck-akp05 \
                ${macSlice "aarch64"}/bin/opendeck-akp05 \
                -output "$out/bin/opendeck-akp05"
              ${macLipo} "$out/bin/opendeck-akp05" -verify_arch x86_64 arm64
              for arch in x86_64 arm64; do
                if ! ${macOtool} -arch "$arch" -l "$out/bin/opendeck-akp05" | grep -E 'minos[[:space:]]+11\.3(\.0)?$' > /dev/null; then
                  echo "macOS $arch release binary does not target macOS 11.3" >&2
                  exit 1
                fi
              done
              if ${macOtool} -L "$out/bin/opendeck-akp05" | grep -qE '^[[:space:]]+/nix/store/'; then
                echo "macOS release binary depends on a Nix store library" >&2
                exit 1
              fi
            '';
        packagePlugin = pkgs.writeShellApplication {
          name = "package-plugin";
          runtimeInputs = [
            pkgs.coreutils
            pkgs.zip
            pkgs.jq
            pkgs.file
            pkgs.gnugrep
          ];
          text = ''
            manifest_path=${./manifest.json}
            assets_path=${./assets}
          ''
          + builtins.readFile ./nix/package-plugin.sh;
        };
      in
      {
        packages = {
          default = native;
          package-plugin = packagePlugin;
          macos-universal = macUniversal;
        }
        // lib.optionalAttrs pkgs.stdenv.isLinux {
          linux-static = linuxStatic;
          inherit windows;
        };

        checks = {
          build = native; # Includes the regression tests.
          clippy = craneLib.cargoClippy (
            common
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            }
          );
          fmt = craneLib.cargoFmt { inherit src; };
          toml-fmt = craneLib.taploFmt {
            src = lib.sources.sourceFilesBySuffices ./. [ ".toml" ];
          };
          # Vulnerability data is pinned. Only the sandbox disables registry-dependent checks.
          audit = craneLib.cargoAudit {
            inherit src advisory-db;
            cargoAuditExtraArgs = "";
            preBuild = ''
              mkdir -p .cargo
              cp ${pkgs.writeText "sandbox-audit.toml" "[yanked]\nenabled = false\n"} .cargo/audit.toml
            '';
          };
          deny = craneLib.cargoDeny common;
        };

        apps = {
          default = flake-utils.lib.mkApp { drv = native; };
          package-plugin = flake-utils.lib.mkApp { drv = packagePlugin; };
        };
        devShells.default = craneLib.devShell {
          packages = [
            pkgs.just
            pkgs.git-cliff
            pkgs.zip
            pkgs.unzip
            pkgs.nixfmt
          ];
        };
        formatter = pkgs.nixfmt;
      }
    );
}
