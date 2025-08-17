{ config, pkgs, lib, inputs, ... }:
let
  spicePkgs = inputs.spicetify-nix.packages.${pkgs.system}.default;
in
{
  imports = [ inputs.spicetify-nix.homeManagerModules.spicetify ];
  programs.spicetify = 
    {
      enable = true;
    };

}
