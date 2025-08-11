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
                "libxml2-2.13.8"
              ];
              
  home.stateVersion = "25.11"; # Please read the comment before changing.
  
  # Stylix configuration (inherits base16 scheme from system)
  # Stylix is handled at the NixOS level and automatically applies to Home Manager

  # The home.packages option allows you to install Nix packages into your
  # environment.

  # System choices
  imports = [
    (folders.hm + "/shells/quickshell.nix")
    (folders.hm + "/shells/ignis.nix")
    (folders.hm + "/wallpapers.nix")
    (folders.hm + "/terminal-tools.nix")
    (folders.hm + "/music.nix")
    (folders.hm + "/yazi.nix")
    (folders.hm + "/utilities.nix")
    (folders.hm + "/maoamaowm.nix")
    inputs.textfox.homeManagerModules.default
  ];


  # Packages without additional preconfigured modules (e.g. nvim)
  home.packages =[
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
    pkgs.citrix_workspace
  ];

  programs.git = {
    enable = true;
    userName = "Tristan Verbeken";
    userEmail = "tristan.verbeken@ugent.be";

  };
  # programs.firefox = {
  #     enable = true;
  #     profiles = {
  #       triverprofile = {
  #         isDefault = true;
  #         # settings = {
  #         #   "browser.startup.homepage" = "https://nixos.org";
  #         # };
  #       };
  #     };
  #   };

    stylix.targets.firefox.enable = true;

  # textfox = {
  #     enable = true;
  #     profile = "triverprofile";
  #     config = {
  #       displayHorizontalTabs = true;
  #       displayWindowControls = true;
  #       displayNavButtons = true;
  #       displayUrlbarIcons = true;
  #       displaySidebarTools = false;
  #       displayTitles = true;
  #       font = { 
  #         family = "Fira Code";
  #         size = "15px";
  #         accent = "#31748f";
  #       };
  #       border = {
  #         color = "#6e6a86";
  #       };
  #     };
  # };

  
  # Let Home Manager install and manage itself.
  programs.home-manager.enable = true;
  
  # Must enable each program in order to be themed by Stylix
  programs.ghostty.enable = true;
  programs.kitty.enable = true;


}
