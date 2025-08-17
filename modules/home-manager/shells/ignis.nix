{ config, pkgs, lib, inputs, ... }: {
  options.triverhome.shells.ignis.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable Ignis shell environment.";
  };

  home.packages = lib.mkIf (config.triverhome.shells.ignis.enable or true) [
    (inputs.ignis.packages.${pkgs.stdenv.hostPlatform.system}.ignis.override {
      extraPackages = [
        # Add extra dependencies here
        # For example:
        pkgs.python313Packages.rapidfuzz
      ];
    })
  ];
}
