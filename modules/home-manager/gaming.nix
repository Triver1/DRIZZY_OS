{ config, lib, pkgs, ... }:

with lib;

let cfg = config.games.launchers;
in {
  options.games.launchers = {
    enable = mkEnableOption "Enable launcher module";

    minecraft = mkOption {
      type = types.bool;
      default = false;
      description = "Install PrismLauncher";
    };
  };

  config = mkIf cfg.enable {
    home.packages = with pkgs; [ prismlauncher ];
  };
}

