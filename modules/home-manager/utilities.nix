{ config, pkgs, lib, ... }:
{
  options.triverhome.utilities.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable utilities (hyprpicker, btop).";
  };

  home.packages = with pkgs; lib.optionals (config.triverhome.utilities.enable or true) [
    hyprpicker
    btop
  ];

  programs.btop = lib.mkIf (config.triverhome.utilities.enable or true) {
    enable = true;
    settings = {
      vim_keys = true;
    };
  };
}
