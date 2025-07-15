
{ inputs, pkgs, ... }:
{
  home.packages = with pkgs; [
    inputs.quickshell.packages.${pkgs.system}.default
  ];
 # TODO Link this config
}
