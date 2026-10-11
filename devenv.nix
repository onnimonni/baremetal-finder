{ pkgs, ... }:

{
  packages = [
    pkgs.git
    pkgs.duckdb
  ];

  languages.rust.enable = true;

  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
  };
}
