{ config, pkgs, lib, inputs, ... }:
let
  spicePkgs = inputs.spicetify-nix.packages.${pkgs.system}.default;
in
{
  options.triverhome.music.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Enable music tooling (spicetify).";
  };

  imports = [ inputs.spicetify-nix.homeManagerModules.spicetify ];

  programs.spicetify = lib.mkIf (config.triverhome.music.enable or true) {
    enable = true;
  };
}
