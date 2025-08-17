{ config, pkgs, inputs, ... }:

{
  home.username = "drizzy";
  nixpkgs.config.allowUnfree = true;
              nixpkgs.config.permittedInsecurePackages = [
                "ventoy-1.1.05"
                "libxml2-2.13.8"
              ];
              
  home.stateVersion = "25.11"; 

  imports = [
    ../../modules/home-manager/triver_home.nix
    inputs.mango.hmModules.mango
  ];

  triverhome = {
    nvim.enable = true;
    wallpapers.enable = true;
    utilities.enable = true;
    music.enable = true;
    yazi.enable = true;
    terminals.ghostty.enable = true;
    terminals.kitty.enable = true;
    shells = {
      quickshell.enable = true;
      ignis.enable = true;
    };
    sessions = [];
  };

  # Packages without additional preconfigured modules (left minimal here; handled by triver_home modules)
  # Add items that were explicitly present before and not handled by modules
  home.packages =[
    pkgs.ventoy
    pkgs.citrix_workspace
    pkgs.lutris
    pkgs.wpaperd
    pkgs.code-cursor-fhs
    pkgs.remnote
    pkgs.conda
    pkgs.openssl
    pkgs.rar
    pkgs.vim
    pkgs.tmux
    pkgs.ripgrep
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
    pkgs.heroic
    pkgs.discord
    pkgs.flutter
  ];

  programs.git = {
    enable = true;
    userName = "Tristan Verbeken";
    userEmail = "tristan.verbeken@ugent.be";
  };

  stylix.targets.firefox.enable = true;

  # Let Home Manager install and manage itself.
  programs.home-manager.enable = true;
}

