{
  # Nix options:
  pkgs ? import <nixpkgs> {
    overlays = [
      (import (builtins.fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz"))
    ];
  },
  lib ? pkgs.lib,

  # Dependency options:

  # qemu_kvm is a heavy package so we should probably allow users to avoid it if they want to.
  run ? true,
  # Adds extra components to the defaulttoolchain for developing Theseus. See rust-toolchain.nix and
  # NIX.md.
  extraRustDevelopmentTools ? true,
  # Adds a custom set of components to the default toolchain.
  extraRustToolchainComponents ? [ ],
  # Allows you to completely override the default toolchain.
  rustToolchain ? import ./rust-toolchain.nix {
    inherit pkgs;
    withDevPkgs = extraRustDevelopmentTools;
    additionalComponents = extraRustToolchainComponents;
  },
  # The prefered bootloader. This controls both dependencies in terms of bootloaders as well as the
  # `bootloader` env var, which sets which bootloader make will use if an explicit arg is not specified
  # to make at runtime.
  bootloader ? "grub",
  # By default, this shell exports the `bootloader` env var as mentioned for the above arg. If set to
  # `false`, it will _not_ set the `bootloader` env var.
  setMakeBootloaderPreference ? true,

  # Limine-Specific options:
  
  # Whether or not to preserve old Limine clones when entering the shell.
  preserveOldLimine ? true,
  # This should generially not be overriden as Theseus relies on a specific commit of Limine and other
  # commits do not have guarenteed support. Use at your own risk.
  requiredLimineCommit ? "3f6a330",
}:
let
  inherit (lib.strings) optionalString;

  gitOutputHandler = "while read line; do echo \" > $line\"; done";

  bootloaderPackageMap = rec {
    any = grub ++ limine;
    grub = [ pkgs.grub2 ];
    limine = [ ]; # This is provided by manually cloning.
  };
in
assert pkgs.lib.asserts.assertMsg
  (builtins.hasAttr bootloader bootloaderPackageMap)
  "\"${bootloader}\" is not a valid value for argument \"bootloader\". Please see the Theseus \
    Nix shell documentation in `Theseus/NIX.md`.";
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    # See NIX.md and rust-toolchain.nix.
    rustToolchain
    # Misc dependancies mentioned in the README
    gnumake gcc nasm pkg-config mtools xorriso wget
  ] ++ (
    lib.lists.optionals run [ pkgs.qemu_kvm ]
  ) ++ bootloaderPackageMap.${bootloader};

  shellHook = let
    limineGit = gitArgs: "${pkgs.git}/bin/git -C limine-prebuilt ${gitArgs} |& ${gitOutputHandler}";
    prepareLimine = if preserveOldLimine
      && builtins.pathExists ./limine-prebuilt
      && (builtins.readFileType ./limine-prebuilt) == "directory"
    then ''
      echo "Detected possible old Limine clone, $(realpath limine-prebuilt). Using that."
    '' else ''
      ${optionalString (builtins.pathExists ./limine-prebuilt) ''
        echo "Removing old ./limine-prebuilt..."
        rm -rf limine-prebuilt
      ''}
      echo "Cloning Limine..."
      ${pkgs.git}/bin/git clone https://github.com/limine-bootloader/limine.git limine-prebuilt \
        |& ${gitOutputHandler}
      echo "Checking out required Limine commit (${requiredLimineCommit})..."
      ${limineGit "checkout ${requiredLimineCommit}"}
    '';
  in
    ''
      ${optionalString (bootloader == "limine" || bootloader == "any") ''
        echo "Detected using Limine bootloader. Preparing Limine..."
        # TODO: Firgure out a way to do this using Nix with Limine instead of doing this cloning the repo
        # thing here.
        ${prepareLimine}
      ''}
      ${optionalString (bootloader != "any" && setMakeBootloaderPreference) ''
        export bootloader=${bootloader}
      ''}
    '';
}
