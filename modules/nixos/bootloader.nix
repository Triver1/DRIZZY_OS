{ config, pkgs, ... }:

{
  # Bootloader configuration using GRUB
  boot.loader = {
    # Disable systemd-boot
    systemd-boot.enable = false;
    
    # Enable GRUB
    grub = {
      enable = true;
      efiSupport = true;
      efiInstallAsRemovable = false;
      device = "nodev";
      useOSProber = true;
      timeout = 5;
      copyKernels = true;
    };
    
    # EFI configuration
    efi = {
      canTouchEfiVariables = true;
      efiSysMountPoint = "/boot";
    };
  };
  
  # Enable os-prober service for detecting other operating systems
  environment.systemPackages = with pkgs; [
    os-prober
  ];
} 
