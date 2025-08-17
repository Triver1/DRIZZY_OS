{ config, lib, pkgs, ... }:
in
{
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

    /*
    Sane nixos settings using the following options/sections, lots of options are actually from the modules themselves
    Music:
        - Enable
        - Spotify
        - Tidal
        - ...
    Wallpapers:
        - Enable
        - Directory?
    Terminal:
        - Enable
        - Shell
        - Tools
    Utilities:
        - Enable
    Basics:
        - Filebrowser
        - Clipboard
        - Notifications
        - ...
    */
   

}
