{ config, lib, pkgs, ... }:
with lib;
let
  cfgLaunchers = config.games.launchers;
  cfgSession = config.games.gamesession;

in {
  options.games.launchers = {
    enable = mkEnableOption "Enable launcher module";

    minecraft = mkOption {
      type = types.bool;
      default = false;
      description = "Install PrismLauncher";
    };
  };

  options.games.gamesession.enable = mkOption {
    type = types.bool;
    default = false;
    description = "Enable a Gamescope login session.";
  };

  config = mkMerge [
    (mkIf (cfgLaunchers.enable && cfgLaunchers.minecraft) {
    home.packages = with pkgs; [ prismlauncher ];
    })

    (mkIf cfgSession.enable {
      home.packages = with pkgs; [ gamescope ];

    })
  ];
}

