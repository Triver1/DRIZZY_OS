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

      # Wayland session desktop entry
      home.file.".local/share/wayland-sessions/gamescope-session.desktop".text = ''
        [Desktop Entry]
        Name=Gamescope Session (Steam)
        Comment=Launch a gamescope Wayland session running Steam Big Picture
        Exec=${pkgs.gamescope}/bin/gamescope -f -- steam -tenfoot
        Type=Application
      '';

      # X11 session desktop entry (fallback)
      home.file.".local/share/xsessions/gamescope-session.desktop".text = ''
        [Desktop Entry]
        Name=Gamescope Session (Steam)
        Comment=Launch a gamescope X session running Steam Big Picture
        Exec=${pkgs.gamescope}/bin/gamescope -f -- steam -tenfoot
        Type=Application
      '';
    })
  ];
}

