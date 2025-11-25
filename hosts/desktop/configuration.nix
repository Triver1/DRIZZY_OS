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
      (folders.m + "/nvidia.nix")
      (folders.m + "/network.nix")
      (folders.m + "/bootloader.nix")
      (folders.m + "/fonts.nix")
      (folders.m + "/nix-ld.nix")
       (folders.m + "/maomaowm.nix")
       (folders.m + "/gamescope.nix")
       (folders.m + "/docker.nix")
      inputs.home-manager.nixosModules.default
    ];
  # Experimental features
  nix.settings.substituters = [ 
  "https://nixpkgs-wayland.cachix.org"
  "https://cache.nixos.org/"
  "https://nix-community.cachix.org"
  "https://nixpkgs-unfree.cachix.org"
  ];


  nix.settings.experimental-features = [ "nix-command" "flakes" ];
  # Fix the bin/batch issue
  services.envfs.enable = true;

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

  # Use LY display manager for better niri compatibility
  services.displayManager.ly.enable = true;
  
  # Enable GNOME Desktop Environment (accessible via LY)
  services.desktopManager.gnome.enable = true;

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
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;
  };


  # Define a user account. Don't forget to set a password with 'passwd'.
  users.users.drizzy = {
    isNormalUser = true;
    description = "drizzy";
    extraGroups = [ "networkmanager" "wheel" "libvirtd" ];
  };

  # Install firefox (default browser)
  programs.firefox.enable = true;

  # Allow unfree packages
  nixpkgs.config.allowUnfree = true;


  hardware.graphics.enable = true;
  
  hardware.bluetooth.enable = true;
  hardware.bluetooth.powerOnBoot = true;
  
  
  services.dbus.enable = true;
  

  # virtualisation = {
  #   libvirtd = {
  #     enable = true;
  #     qemu = {
  #       package = pkgs.qemu_kvm;
  #       swtpm.enable = true;
  #       ovmf.enable = true;
  #     };
  #   };
  #   spiceUSBRedirection.enable = true;
  # };


  # List services that you want to enable:
  # Enable the OpenSSH daemon.
  services.openssh.enable = true;
   
  programs.steam = {
    enable = false;
    remotePlay.openFirewall = true; # Open ports in the firewall for Steam Remote Play
    dedicatedServer.openFirewall = true; # Open ports in the firewall for Source Dedicated Server
    localNetworkGameTransfers.openFirewall = true; # Open ports in the firewall for Steam Local Network Game Transfers
    gamescopeSession.enable = true;
  };
   
  home-manager = { 
    extraSpecialArgs = { inherit inputs; };
    users = {
       "drizzy" = import ./home_drizzy.nix;
    };


  };
}
