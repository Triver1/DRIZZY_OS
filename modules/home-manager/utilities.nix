{ config, pkgs, ... }:
{ 
  home.packages = [
    pkgs.hyprpicker
    pkgs.btop
  ];
  programs.btop = {
    enable = true;
    settings = {
      vim_keys = true;
    };
  };
}
