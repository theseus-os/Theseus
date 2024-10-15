{
  pkgs ? import <nixpkgs> {
    overlays = [
      (import (builtins.fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz"))
    ];
  },
  lib ? pkgs.lib,
  withDevPkgs ? true,
  additionalComponents ? [ ],
}:
let
  toolchainFile = ./rust-toolchain.toml;
  toolchainDesc = (builtins.fromTOML (builtins.readFile toolchainFile)).toolchain;
  finalToolchain = toolchainDesc // {
    components = toolchainDesc.components ++ lib.lists.optionals withDevPkgs [
      "rust-analyzer"
    ] ++ additionalComponents;
  };
in
(pkgs.rust-bin.fromRustupToolchain finalToolchain)
