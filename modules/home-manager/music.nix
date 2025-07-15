{ config, pkgs, lib, inputs, ... }:
let
  spicePkgs = inputs.spicetify-nix.packages.${pkgs.system}.default;
in
{
  imports = [ inputs.spicetify-nix.homeManagerModules.spicetify ];

  # Spicetify also installs spotify, so no need to install this (would make a collision)
  programs.spicetify = 
    {
      enable = true;
      # theme = spicePkgs.themes.catppuccin;
      # colorScheme = "mocha";

    };

}
