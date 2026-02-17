{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, crane, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        rust = pkgs.rust-bin.stable.latest.default.override {
          targets = [
            "x86_64-unknown-linux-gnu"
            "aarch64-unknown-linux-gnu"
            "x86_64-pc-windows-gnu"
            "aarch64-pc-windows-gnullvm" # aarch64 windows: use zigbuild via justfile (no nixpkgs cross toolchain)
            "x86_64-apple-darwin"
            "aarch64-apple-darwin"
          ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rust;
        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          pname = "opendeck-akp05";
          strictDeps = true;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        mkCross = { target, depsBuildBuild ? [], env ? {} }:
          let
            crossArgs = commonArgs // {
              CARGO_BUILD_TARGET = target;
              inherit depsBuildBuild;
              HOST_CC = "${pkgs.stdenv.cc}/bin/cc";
              doCheck = false;
            } // env;
          in craneLib.buildPackage (crossArgs // {
            cargoArtifacts = craneLib.buildDepsOnly crossArgs;
          });

      in {
        packages = {
          default = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
          });
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux (
          let
            ccAarch64 = pkgs.pkgsCross.aarch64-multiplatform.stdenv.cc;
            ccMingw = pkgs.pkgsCross.mingwW64.stdenv.cc;
          in {
            aarch64-linux = mkCross {
              target = "aarch64-unknown-linux-gnu";
              depsBuildBuild = [ ccAarch64 ];
              env.CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER =
                "${ccAarch64}/bin/${ccAarch64.targetPrefix}gcc";
            };

            x86_64-windows = mkCross {
              target = "x86_64-pc-windows-gnu";
              depsBuildBuild = [ ccMingw ];
              env = {
                CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER =
                  "${ccMingw}/bin/${ccMingw.targetPrefix}gcc";
                CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS =
                  "-L native=${pkgs.pkgsCross.mingwW64.windows.pthreads}/lib";
              };
            };


          }
        );

        devShells.default = craneLib.devShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = [
            pkgs.cargo-zigbuild
            pkgs.zig
          ];
        };
      }
    );
}
