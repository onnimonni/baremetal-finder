{ pkgs, ... }:

{
  packages = [
    pkgs.git
    pkgs.duckdb
    # BoringSSL (browser TLS fingerprints via wreq) builds with cmake
    pkgs.cmake
  ];

  languages.rust.enable = true;

  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
  };
}
