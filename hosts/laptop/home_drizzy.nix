{ config, pkgs, inputs, ... }:
let 
folders = {
  hm = ../../modules/home-manager;
};
in
{
  # Home Manager needs a bit of information about you and the paths it should
  # manage.
 

  home.username = "drizzy";
  nixpkgs.config.allowUnfree = true;
              nixpkgs.config.permittedInsecurePackages = [
                "ventoy-1.1.05"
              ];
              
  home.stateVersion = "25.11"; # Please read the comment before changing.
  
  # Stylix configuration (inherits base16 scheme from system)
  # Stylix is handled at the NixOS level and automatically applies to Home Manager

  # The home.packages option allows you to install Nix packages into your
  # environment.

  # System choices
  imports = [
    (folders.hm + "/shells/quickshell.nix")
    (folders.hm + "/wallpapers.nix")
    (folders.hm + "/terminal-tools.nix")
    (folders.hm + "/music.nix")
  ];


  # Packages without additional preconfigured modules (e.g. nvim)
  home.packages =[
    # Utils
    pkgs.ventoy
    # UI
    pkgs.ripgrep
    pkgs.lutris
    pkgs.wpaperd
    # Productivity
    pkgs.code-cursor-fhs
    pkgs.remnote
    pkgs.conda
    pkgs.openssl
    pkgs.rar
    # Productivity - terminal
    pkgs.ghostty
    pkgs.vim
    pkgs.tmux
    # Development
    pkgs.rustc
    pkgs.cargo
    pkgs.rustfmt
    pkgs.rust-analyzer
    pkgs.gcc
    pkgs.lldb
    pkgs.ninja
    pkgs.meson
    pkgs.cairo
    pkgs.buildPackages.pkg-config
    pkgs.waybar
    # Gaming
    pkgs.heroic
    pkgs.discord
    pkgs.flutter
    pkgs.vivaldi
  ];

  programs.git = {
    enable = true;
    userName = "Tristan Verbeken";
    userEmail = "tristan.verbeken@ugent.be";

  };
  


  programs.zsh = {
   enable = true;  # Enable ZSH theming integration with Stylix
   oh-my-zsh = {
     enable = true;
     theme = "minimal";
   };
   shellAliases = {
    hm = "cd ~/NEWFLAKE/modules/home-manager/";
    ms = "sudo nixos-rebuild switch";
   };
  };
  
  # Let Home Manager install and manage itself.
  programs.home-manager.enable = true;
  
  # Must enable each program in order to be themed by Stylix
  programs.ghostty.enable = true;
  programs.kitty.enable = true;


}
