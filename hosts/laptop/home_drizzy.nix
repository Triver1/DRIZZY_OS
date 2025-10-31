{ config, pkgs, inputs, ... }:

{
  home.username = "drizzy";
  home.homeDirectory = "/home/drizzy";
  nixpkgs.config.allowUnfree = true;
              
  home.stateVersion = "25.05"; 

  imports = [
    ../../modules/home-manager/triver_home.nix
    inputs.zen-browser.homeModules.beta
  ];
  programs.zen-browser.enable = true;

  triverhome = {
    nvim.enable = true;
    wallpapers.enable = true;
    utilities.enable = true;
    music.enable = true;
    yazi.enable = true;
    terminals.ghostty.enable = true;
    terminals.kitty.enable = true;
    shells = {
      ignis.enable = true;
    };
    sessions = [];
  };

  games = {
    launchers.enable = false;
    launchers.minecraft = false;
    gamesession.enable = true;
    dev = {
      unity.enable = true;
      godot.enable = false;
    };
  };

  # Packages without additional preconfigured modules (left minimal here; handled by triver_home modules)
  # Add items that were explicitly present before and not handled by modules

  home.packages =[
    pkgs.nodejs_24
    pkgs.pnpm
    pkgs.sqlite
    pkgs.jetbrains.idea-ultimate
    pkgs.jdk
    pkgs.maven
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
    pkgs.anki
    pkgs.teams-for-linux
    pkgs.python313Packages.jupyterlab
    pkgs.uv
    pkgs.typst
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

