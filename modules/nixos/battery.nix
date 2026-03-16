{ config, lib, pkgs, ... }:

{
  # Enable TLP for laptop battery management
  services.tlp = {
    enable = true;
    settings = {
      # Battery charge thresholds to preserve battery health
      START_CHARGE_THRESH_BAT1 = 40;
      STOP_CHARGE_THRESH_BAT1 = 95;
  
      # CPU scaling governor
      CPU_SCALING_GOVERNOR_ON_AC = "powersave";
      CPU_SCALING_GOVERNOR_ON_BAT = "powersave";

    };
  };

  services.upower = {
      enable = true;
      # Optional: additional configuration
      percentageLow = 10;
      percentageCritical = 5;
      percentageAction = 2;
      criticalPowerAction = "Hibernate";
    };
  # Disable power-profiles-daemon (conflicts with TLP)
  services.power-profiles-daemon.enable = false;

  # Basic power management
  powerManagement.enable = true;
} 
