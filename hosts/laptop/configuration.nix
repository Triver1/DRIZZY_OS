# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, inputs,... }:
let 
folders = {
  m = ../../modules/nixos;
};
in
{
  imports =
    [ # Include the results of the hardware scan.
      ./hardware-configuration.nix
      # (folders.m + "/hypr.nix")
      (folders.m + "/niri.nix")
      (folders.m + "/basics.nix")
      (folders.m + "/stylix.nix")
      # (folders.m + "/nvidia.nix")
      (folders.m + "/network.nix")
      (folders.m + "/bootloader.nix")
      (folders.m + "/battery.nix")
      (folders.m + "/fonts.nix")
      inputs.home-manager.nixosModules.default
    ];
  # Experimental features
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
  # Fix the bin/batch issue
  services.envfs.enable = true;

  # Use latest kernel.
  boot.kernelPackages = pkgs.linuxPackages_latest;

  networking.hostName = "nixos"; # Define your hostname.

  # Enable networking
  networking.networkmanager.enable = true;

  # Set your time zone.
  time.timeZone = "Europe/Brussels";

  # Select internationalisation properties.
  i18n.defaultLocale = "en_US.UTF-8";

  # Enable the X11 windowing system.
  services.xserver.enable = true;

  # Enable the GNOME Desktop Environment.
  services.displayManager.gdm.enable = true;
  # services.desktopManager.gnome.enable = true;

  # Configure keymap in X11
  services.xserver.xkb = {
    layout = "us";
    variant = "";
  };

  # Enable CUPS to print documents.
  services.printing.enable = true;

  # Enable sound with pipewire.
  services.pulseaudio.enable = false;
  security.rtkit.enable = true;
  services.pipewire = {
    enable = false;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;
  };


  # Define a user account. Don't forget to set a password with ‘passwd’.
  users.users.drizzy = {
    isNormalUser = true;
    description = "drizzy";
    extraGroups = [ "networkmanager" "wheel" ];
  };

  # Install firefox (default browser)
  programs.firefox.enable = true;

  # Allow unfree packages
  nixpkgs.config.allowUnfree = true;



  # List services that you want to enable:
  # Enable the OpenSSH daemon.
  services.openssh.enable = true;
  system.stateVersion = "25.05"; # Did you read the comment?
   
  home-manager = { 
    extraSpecialArgs = { inherit inputs; };
    users = {
       "drizzy" = import ./home_drizzy.nix;
    };


  };
}
