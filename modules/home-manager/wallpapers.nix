{ pkgs, config, lib, ... }:
{
  options.triverhome.wallpapers.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable wallpaper service (wpaperd).";
  };

  services.wpaperd = lib.mkIf (config.triverhome.wallpapers.enable or true) {
    enable = true;
    settings = {
      DP-3 = {
        path = "/home/drizzy/NEWFLAKE/assets/wallpapers/wqhd/";
        sorting = "random";
        duration = "30m";
      };
      eDP-1 = {
        path = "/home/drizzy/NEWFLAKE/assets/wallpapers/laptop/";
        sorting = "random";
        duration = "30m";
      };
    };
  };
}
