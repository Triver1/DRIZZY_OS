{ config, pkgs, lib, inputs, ... }: {
  home.packages = [
    (inputs.ignis.packages.${pkgs.stdenv.hostPlatform.system}.ignis.override {
      extraPackages = [
        # Add extra dependencies here
        # For example:
        pkgs.python313Packages.rapidfuzz
      ];
    })
  ];
}
