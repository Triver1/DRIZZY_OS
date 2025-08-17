{ config, lib, pkgs, ... }:

let
  types = lib.types;
in
{
  options.triverhome = {
    sessions = lib.mkOption {
      type = types.listOf (types.enum [ "hyprland" "niri" "mango" "gnome" ]);
      default = [];
      description = "List of preferred desktop sessions.";
    };
  };

  imports = [
    ("./shells/quickshell.nix")
    ("./shells/ignis.nix")
    ("./gaming.nix")
    ("./wallpapers.nix")
    ("./terminal-tools.nix")
    ("./music.nix")
    ("./yazi.nix")
    ("./utilities.nix")
  ];

  config.triverhome = {
    nvim.enable = lib.mkDefault true;
    wallpapers.enable = lib.mkDefault true;
    utilities.enable = lib.mkDefault true;
    music.enable = lib.mkDefault true;
    yazi.enable = lib.mkDefault true;
    terminals.ghostty.enable = lib.mkDefault true;
    terminals.kitty.enable = lib.mkDefault true;
    shells = {
      ignis.enable = lib.mkDefault true;
      quickshell.enable = lib.mkDefault false;
    };
  };
}
