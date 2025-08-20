
{ inputs, pkgs, config, lib, ... }:
{
  options.triverhome.shells.quickshell.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable Quickshell environment and dependencies.";
  };

  config = {
    home.packages = lib.mkIf (config.triverhome.shells.quickshell.enable or true) (with pkgs; [
    inputs.quickshell.packages.${pkgs.system}.default
    kdePackages.qt5compat
    libsForQt5.qt5.qtgraphicaleffects
    qt5.qtsvg
    ]);
  };
}
