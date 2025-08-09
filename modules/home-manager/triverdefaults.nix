{ config, lib, pkgs, ... }:

{
   imports = [
    (folders.hm + "/shells/quickshell.nix")
    (folders.hm + "/shells/ignis.nix")
    (folders.hm + "/wallpapers.nix")
    (folders.hm + "/terminal-tools.nix")
    (folders.hm + "/music.nix")
    (folders.hm + "/yazi.nix")
    (folders.hm + "/utilities.nix")
    inputs.textfox.homeManagerModules.default
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