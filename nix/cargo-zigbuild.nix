{
  cargo-zigbuild,
  fetchCrate,
  rustPlatform,
}:

# 0.24 forwards MACOSX_DEPLOYMENT_TARGET to Zig; the pinned nixpkgs version does not.
cargo-zigbuild.overrideAttrs (
  final: _: {
    version = "0.24.0";
    src = fetchCrate {
      pname = "cargo-zigbuild";
      inherit (final) version;
      hash = "sha256-bHSjSCQHah1IDnv8XNkfarz7JSqgZR92A0+/6Cih6Po=";
    };
    cargoDeps = rustPlatform.fetchCargoVendor {
      inherit (final) src;
      hash = "sha256-QBsZEP8er3tTF9zGBD2UPiZwsXBf6Hy0t+du56gQprg=";
    };
    preCheck = ''
      export CARGO_ZIGBUILD_CACHE_DIR="$TMPDIR/zigbuild-check-cache"
    '';
  }
)
