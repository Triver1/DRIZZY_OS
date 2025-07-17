
{ inputs, pkgs, config, ... }:
{
  home.packages = with pkgs; [
    inputs.quickshell.packages.${pkgs.system}.default
    kdePackages.qt5compat
    libsForQt5.qt5.qtgraphicaleffects
    qt5.qtsvg
  ];

}
